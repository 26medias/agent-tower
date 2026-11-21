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
    /** Called one time, when the first output bytes (scrollback or live) reach the terminal. */
    onOutput?(): void;
    /** Called when the operator types or pastes in this terminal (not for the answers that xterm sends by itself). */
    onInput?(): void;
}

/** The terminals that are alive, by session ID. `focusTerminal` uses it. */
const registry = new Map<string, SessionTerminal>();

/** Put the keyboard focus in the terminal of a session. False when there is no such terminal. */
export function focusTerminal(sessionId: string): boolean {
    const t = registry.get(sessionId);
    if (!t) return false;
    t.focus();
    return true;
}

/**
 * The size goes to the process this long after the last change. A drag of a handle changes the
 * size on each frame. The process gets the last size one time, not each step.
 */
export const RESIZE_SEND_MS = 150;

const THEME = {
    background: "#0b111c",
    foreground: "#e6ecf8",
    cursor: "#5ee0c0",
    cursorAccent: "#0b111c",
    selectionBackground: "#2b3a5c",
    black: "#18233a",
    red: "#ff6b81",
    green: "#4ade80",
    yellow: "#ffcf5c",
    blue: "#4aa3ff",
    magenta: "#d68cff",
    cyan: "#2dd4bf",
    white: "#e6ecf8",
    brightBlack: "#6b7896",
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
    private resizeTimer: ReturnType<typeof setTimeout> | null = null;
    /** The last size that went to the process. */
    private sentSize: { rows: number; cols: number } | null = null;
    private sawOutput = false;

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
        registry.set(sessionId, this);
        // `onData` also carries the answers that xterm gives to the CLI by itself. Only a key press
        // or a paste is the operator.
        this.term.onKey(() => this.hooks.onInput?.());
        this.term.textarea?.addEventListener("paste", () => this.hooks.onInput?.());

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
        this.term.onResize(() => {
            if (!this.live) return;
            if (this.resizeTimer) clearTimeout(this.resizeTimer);
            this.resizeTimer = setTimeout(() => {
                this.resizeTimer = null;
                this.sendSize(false);
            }, RESIZE_SEND_MS);
        });

        this.observer = new ResizeObserver(() => this.scheduleFit());
        this.observer.observe(host);
    }

    /** Send the size of the terminal to the process. `force`: also when it is the size sent last. */
    private sendSize(force: boolean): void {
        if (this.disposed || !this.live) return;
        const { rows, cols } = this.term;
        if (!force && this.sentSize && this.sentSize.rows === rows && this.sentSize.cols === cols) return;
        this.sentSize = { rows, cols };
        ipc.ptyResize(this.sessionId, rows, cols).catch((e) => this.hooks.onError(ipc.errorText(e)));
    }

    /** Write output bytes to the terminal. The first bytes tell the pane that output exists. */
    private write(bytes: Uint8Array): void {
        if (bytes.length > 0 && !this.sawOutput) {
            this.sawOutput = true;
            this.hooks.onOutput?.();
        }
        this.term.write(bytes);
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
                else this.write(bytes);
            });
            if (this.disposed) return;
            if (result.scrollback) this.write(base64ToBytes(result.scrollback));
            for (const bytes of held) this.write(bytes);
            held = null;
            this.attachState = "done";
        } catch (e) {
            held = null;
            this.attachState = "no"; // a later show tries again
            this.hooks.onError(ipc.errorText(e));
            return;
        }
        // The process may have started with another size. Tell it the real one.
        if (this.resizeTimer) {
            clearTimeout(this.resizeTimer);
            this.resizeTimer = null;
        }
        this.sendSize(true);
    }

    dispose(): void {
        this.disposed = true;
        if (registry.get(this.sessionId) === this) registry.delete(this.sessionId);
        if (this.frame) cancelAnimationFrame(this.frame);
        if (this.resizeTimer) clearTimeout(this.resizeTimer);
        this.observer.disconnect();
        this.term.dispose();
    }
}
