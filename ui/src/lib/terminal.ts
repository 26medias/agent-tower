// One xterm.js instance for one session tab. It stays alive while the tab is hidden, so
// no output is lost on a tab change.
//
// Output path: session_attach -> scrollback (base64) -> bytes -> term.write(bytes), then each
//   channel message (base64) -> bytes -> term.write(bytes), in order.
// Input path: term.onData (string, UTF-8 encode) and term.onBinary (binary string, 1 char = 1 byte)
//   -> bytes -> WriteQueue -> base64 -> pty_write. Nothing is filtered or rewritten.
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { base64ToBytes, binaryStringToBytes, utf8Encode, WriteQueue } from "./codec";
import * as ipc from "./ipc";

export interface TerminalHooks {
    onError(text: string): void;
    /** Called with the size after each fit that was applied. */
    onSize(rows: number, cols: number): void;
}

const THEME = {
    background: "#0c0b12",
    foreground: "#e6e1f5",
    cursor: "#ffcf5c",
    cursorAccent: "#0c0b12",
    selectionBackground: "#3a3556",
    black: "#1d1a2b",
    red: "#ff6b81",
    green: "#4ade80",
    yellow: "#ffcf5c",
    blue: "#4aa3ff",
    magenta: "#d68cff",
    cyan: "#2dd4bf",
    white: "#e6e1f5",
    brightBlack: "#6b6785",
    brightRed: "#ff8fa0",
    brightGreen: "#7cf0a6",
    brightYellow: "#ffe08a",
    brightBlue: "#7dbcff",
    brightMagenta: "#e5b3ff",
    brightCyan: "#6ee7d6",
    brightWhite: "#ffffff",
};

export class SessionTerminal {
    readonly term: Terminal;
    private readonly fitAddon = new FitAddon();
    private readonly queue: WriteQueue;
    private readonly observer: ResizeObserver;
    private attachState: "no" | "pending" | "done" = "no";
    private live = true;
    private disposed = false;
    private frame = 0;

    constructor(
        readonly sessionId: string,
        private readonly host: HTMLElement,
        private readonly hooks: TerminalHooks,
    ) {
        this.term = new Terminal({
            cursorBlink: true,
            fontFamily: 'ui-monospace, "JetBrains Mono", "DejaVu Sans Mono", Menlo, Consolas, monospace',
            fontSize: 13,
            scrollback: 5000,
            theme: THEME,
        });
        this.term.loadAddon(this.fitAddon);
        this.term.open(host);

        this.queue = new WriteQueue(
            (data) => ipc.ptyWrite(sessionId, data),
            (e) => hooks.onError(ipc.errorText(e)),
        );
        // Input goes through unchanged. A stopped session has no process, so its input is dropped.
        this.term.onData((text) => {
            if (this.live) this.queue.push(utf8Encode(text));
        });
        this.term.onBinary((text) => {
            if (this.live) this.queue.push(binaryStringToBytes(text));
        });
        this.term.onResize(({ rows, cols }) => {
            if (this.live) {
                ipc.ptyResize(sessionId, rows, cols).catch((e) => hooks.onError(ipc.errorText(e)));
            }
        });

        this.observer = new ResizeObserver(() => this.scheduleFit());
        this.observer.observe(host);
    }

    setLive(live: boolean): void {
        this.live = live;
    }

    /** Fit on the next animation frame. Many resize events give one fit. */
    scheduleFit(): void {
        if (this.disposed || this.frame) return;
        this.frame = requestAnimationFrame(() => {
            this.frame = 0;
            this.fitNow();
        });
    }

    /** Fit to the host. Does nothing while the host is too small to measure. */
    fitNow(): boolean {
        if (this.disposed) return false;
        if (this.host.clientWidth < 40 || this.host.clientHeight < 30) return false;
        try {
            this.fitAddon.fit();
        } catch {
            return false;
        }
        this.hooks.onSize(this.term.rows, this.term.cols);
        return true;
    }

    focus(): void {
        this.term.focus();
    }

    /** Fit, then attach on the first call. Later calls only fit. */
    async show(): Promise<void> {
        this.fitNow();
        if (this.attachState !== "no") return;
        this.attachState = "pending";
        // The channel can deliver before the attach call returns. Hold those messages
        // until the scrollback is written, so that the order stays right.
        let held: Uint8Array[] | null = [];
        try {
            const result = await ipc.sessionAttach(this.sessionId, (message) => {
                const bytes = base64ToBytes(message.data);
                if (held) held.push(bytes);
                else this.term.write(bytes);
            });
            if (this.disposed) return;
            if (result.scrollback) this.term.write(base64ToBytes(result.scrollback));
            for (const bytes of held) this.term.write(bytes);
            held = null;
            this.attachState = "done";
        } catch (e) {
            held = null;
            this.attachState = "no"; // a later show tries again
            this.hooks.onError(ipc.errorText(e));
            return;
        }
        // The process may have started with another size. Tell it the real one.
        if (this.live) {
            ipc.ptyResize(this.sessionId, this.term.rows, this.term.cols).catch((e) =>
                this.hooks.onError(ipc.errorText(e)),
            );
        }
    }

    dispose(): void {
        this.disposed = true;
        if (this.frame) cancelAnimationFrame(this.frame);
        this.observer.disconnect();
        this.term.dispose();
    }
}
