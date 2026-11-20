import { describe, expect, it } from "vitest";
import {
    base64ToBytes,
    binaryStringToBytes,
    bytesToBase64,
    concatBytes,
    encodeBinaryInput,
    encodeDataInput,
    utf8Encode,
    WriteQueue,
} from "./codec";

const bytes = (...n: number[]) => new Uint8Array(n);
const decodeInput = (b64: string) => Array.from(base64ToBytes(b64));

describe("input encoding (onData)", () => {
    it("encodes ASCII", () => {
        expect(decodeInput(encodeDataInput("ls -l\r"))).toEqual([0x6c, 0x73, 0x20, 0x2d, 0x6c, 0x0d]);
        expect(encodeDataInput("abc")).toBe("YWJj");
    });

    it("encodes UTF-8 multi-byte characters as UTF-8, not as code units", () => {
        // e with acute: 2 bytes. Euro sign: 3 bytes. Emoji (surrogate pair): 4 bytes.
        expect(decodeInput(encodeDataInput("é"))).toEqual([0xc3, 0xa9]);
        expect(decodeInput(encodeDataInput("€"))).toEqual([0xe2, 0x82, 0xac]);
        expect(decodeInput(encodeDataInput("\u{1F600}"))).toEqual([0xf0, 0x9f, 0x98, 0x80]);
    });

    it("passes control keys unchanged", () => {
        expect(decodeInput(encodeDataInput("\x1b[A"))).toEqual([0x1b, 0x5b, 0x41]); // arrow up
        expect(decodeInput(encodeDataInput("\x03"))).toEqual([0x03]); // Ctrl-C
        expect(decodeInput(encodeDataInput("\x7f"))).toEqual([0x7f]); // backspace
    });

    it("passes a bracketed paste sequence unchanged, with a multi-line body", () => {
        const paste = "\x1b[200~line one\nline two é\x1b[201~";
        const out = base64ToBytes(encodeDataInput(paste));
        expect(Array.from(out)).toEqual(Array.from(utf8Encode(paste)));
        expect(new TextDecoder().decode(out)).toBe(paste);
        expect(Array.from(out.slice(0, 6))).toEqual([0x1b, 0x5b, 0x32, 0x30, 0x30, 0x7e]); // ESC[200~
        expect(Array.from(out.slice(-6))).toEqual([0x1b, 0x5b, 0x32, 0x30, 0x31, 0x7e]); // ESC[201~
    });

    it("handles a large paste without stack overflow", () => {
        const big = "x".repeat(300_000);
        expect(base64ToBytes(encodeDataInput(big)).length).toBe(300_000);
    });

    it("gives an empty string for empty input", () => {
        expect(encodeDataInput("")).toBe("");
    });
});

describe("input encoding (onBinary)", () => {
    it("maps one char to one byte, also above 127", () => {
        const binary = String.fromCharCode(0x00, 0x7f, 0x80, 0xa9, 0xff);
        expect(decodeInput(encodeBinaryInput(binary))).toEqual([0x00, 0x7f, 0x80, 0xa9, 0xff]);
    });

    it("is not UTF-8: char 0xE9 is the single byte 0xE9", () => {
        expect(decodeInput(encodeBinaryInput("é"))).toEqual([0xe9]);
        expect(decodeInput(encodeDataInput("é"))).toEqual([0xc3, 0xa9]);
    });

    it("binaryStringToBytes keeps the length", () => {
        expect(binaryStringToBytes("\x1b[M\x20\xe9\xe9").length).toBe(6);
    });
});

describe("base64", () => {
    it("round-trips all 256 byte values", () => {
        const all = new Uint8Array(256).map((_, i) => i);
        expect(Array.from(base64ToBytes(bytesToBase64(all)))).toEqual(Array.from(all));
    });
});

describe("output decoding", () => {
    // U+2713 (check mark) is E2 9C 93. The channel cuts it after 2 bytes.
    const whole = new TextEncoder().encode("ok ✓ done");
    const cut = whole.indexOf(0xe2) + 2;
    const chunk1 = bytesToBase64(whole.slice(0, cut));
    const chunk2 = bytesToBase64(whole.slice(cut));

    it("gives bytes, and the two chunks joined are the original text", () => {
        const a = base64ToBytes(chunk1);
        const b = base64ToBytes(chunk2);
        expect(a).toBeInstanceOf(Uint8Array);
        expect(new TextDecoder().decode(concatBytes([a, b]))).toBe("ok ✓ done");
    });

    it("shows why a string per chunk is wrong: each half alone has a broken character", () => {
        const a = new TextDecoder().decode(base64ToBytes(chunk1));
        const b = new TextDecoder().decode(base64ToBytes(chunk2));
        expect(a + b).not.toBe("ok ✓ done");
        expect(a + b).toContain("�");
    });

    it("a streaming decoder over the byte chunks gives the right text", () => {
        const d = new TextDecoder();
        const text =
            d.decode(base64ToBytes(chunk1), { stream: true }) + d.decode(base64ToBytes(chunk2), { stream: true });
        expect(text).toBe("ok ✓ done");
    });
});

describe("WriteQueue", () => {
    it("sends in order, one call at a time, and joins input that comes during a call", async () => {
        const calls: number[][] = [];
        let release: (() => void) | undefined;
        let inFlight = 0;
        let maxInFlight = 0;
        const queue = new WriteQueue(async (b64) => {
            inFlight++;
            maxInFlight = Math.max(maxInFlight, inFlight);
            calls.push(decodeInput(b64));
            if (calls.length === 1) await new Promise<void>((r) => (release = r));
            inFlight--;
        });
        queue.push(bytes(1)); // starts call 1, which waits
        queue.push(bytes(2));
        queue.push(bytes(3));
        await Promise.resolve();
        release?.();
        await new Promise((r) => setTimeout(r, 0));
        expect(calls).toEqual([[1], [2, 3]]);
        expect(maxInFlight).toBe(1);
    });

    it("reports an error and keeps sending", async () => {
        const sent: number[][] = [];
        const errors: unknown[] = [];
        let n = 0;
        const queue = new WriteQueue(
            async (b64) => {
                n++;
                if (n === 1) throw "boom";
                sent.push(decodeInput(b64));
            },
            (e) => errors.push(e),
        );
        queue.push(bytes(1));
        queue.push(bytes(2));
        await new Promise((r) => setTimeout(r, 0));
        expect(errors).toEqual(["boom"]);
        expect(sent).toEqual([[2]]);
    });

    it("ignores empty pushes", async () => {
        let calls = 0;
        const queue = new WriteQueue(async () => void calls++);
        queue.push(new Uint8Array(0));
        await new Promise((r) => setTimeout(r, 0));
        expect(calls).toBe(0);
    });
});
