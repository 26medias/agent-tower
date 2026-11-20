// Pure byte and base64 helpers for the terminal data path. No DOM, no Tauri.

const CHUNK = 0x8000;

/** Bytes to base64. Works in chunks so that large pastes do not overflow the call stack. */
export function bytesToBase64(bytes: Uint8Array): string {
    let binary = "";
    for (let i = 0; i < bytes.length; i += CHUNK) {
        binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
    }
    return btoa(binary);
}

/** Base64 to bytes. Output of a session arrives in this form and goes to `term.write` as bytes. */
export function base64ToBytes(b64: string): Uint8Array {
    const binary = atob(b64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
        bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
}

/** A JS string (xterm `onData`) to its UTF-8 bytes. */
export function utf8Encode(text: string): Uint8Array {
    return new TextEncoder().encode(text);
}

/** A binary string (xterm `onBinary`: one char = one byte) to bytes. */
export function binaryStringToBytes(text: string): Uint8Array {
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) {
        bytes[i] = text.charCodeAt(i) & 0xff;
    }
    return bytes;
}

/** Input from xterm `onData` to the base64 that `pty_write` wants. */
export function encodeDataInput(text: string): string {
    return bytesToBase64(utf8Encode(text));
}

/** Input from xterm `onBinary` to the base64 that `pty_write` wants. */
export function encodeBinaryInput(text: string): string {
    return bytesToBase64(binaryStringToBytes(text));
}

/** Join byte chunks into one array. */
export function concatBytes(chunks: Uint8Array[]): Uint8Array {
    let total = 0;
    for (const c of chunks) total += c.length;
    const out = new Uint8Array(total);
    let offset = 0;
    for (const c of chunks) {
        out.set(c, offset);
        offset += c.length;
    }
    return out;
}

/**
 * Sends operator input in order. One `pty_write` is in flight at a time. Input that comes
 * during a call joins into the next call. Async Tauri commands can run in any order, so
 * without this queue two quick keys could arrive swapped.
 */
export class WriteQueue {
    private pending: Uint8Array[] = [];
    private busy = false;

    constructor(
        private readonly send: (base64: string) => Promise<void>,
        private readonly onError: (error: unknown) => void = () => {},
    ) {}

    push(bytes: Uint8Array): void {
        if (bytes.length === 0) return;
        this.pending.push(bytes);
        if (!this.busy) void this.drain();
    }

    private async drain(): Promise<void> {
        this.busy = true;
        try {
            while (this.pending.length > 0) {
                const batch = concatBytes(this.pending);
                this.pending = [];
                try {
                    await this.send(bytesToBase64(batch));
                } catch (error) {
                    this.onError(error);
                }
            }
        } finally {
            this.busy = false;
        }
    }
}
