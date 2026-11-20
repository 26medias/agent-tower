// The only module that talks to the Rust side. One typed function for each command of
// docs/ipc-m1.md. Inside Tauri it uses the real invoke/listen/Channel. In a normal browser
// (no `window.__TAURI_INTERNALS__`) it uses the in-memory mock of ./mock.ts.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
    AttachResult,
    CliKind,
    FloorChangedPayload,
    FloorDefaults,
    FloorEntry,
    FloorView,
    Employee,
    ModelCatalog,
    ModelKind,
    OutputMessage,
    ProgramName,
    Programs,
    SessionChangedPayload,
    SessionView,
    Settings,
} from "./types";

export type Unlisten = () => void;

/** What the typed functions need from a transport. The real one and the mock both implement it. */
export interface Backend {
    readonly kind: "tauri" | "mock";
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
    listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten>;
    /** A channel object that goes in the `channel` argument of `session_attach`. */
    newChannel(onMessage: (message: OutputMessage) => void): unknown;
}

export function isTauri(): boolean {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const tauriBackend: Backend = {
    kind: "tauri",
    invoke: <T>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args),
    listen: async <T>(event: string, handler: (payload: T) => void) => {
        return listen<T>(event, (e) => handler(e.payload));
    },
    newChannel(onMessage) {
        const channel = new Channel<OutputMessage>();
        channel.onmessage = onMessage;
        return channel;
    },
};

let backendPromise: Promise<Backend> | null = null;

function backend(): Promise<Backend> {
    if (!backendPromise) {
        backendPromise = isTauri()
            ? Promise.resolve(tauriBackend)
            : import("./mock").then((m) => m.createMockBackend());
    }
    return backendPromise;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return (await backend()).invoke<T>(command, args);
}

/** "tauri" or "mock". */
export async function backendKind(): Promise<Backend["kind"]> {
    return (await backend()).kind;
}

/** A command error as text for a person. Tauri rejects with a string. */
export function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    try {
        return JSON.stringify(error);
    } catch {
        return String(error);
    }
}

// ---- Commands (docs/ipc-m1.md, "Commands") ----

export const floorList = () => call<FloorEntry[]>("floor_list");

export const floorOpen = (path: string) => call<FloorView>("floor_open", { path });

export const floorCreate = (path: string, name: string) => call<FloorView>("floor_create", { path, name });

export const floorForget = (path: string) => call<void>("floor_forget", { path });

export const employeeSave = (path: string, employee: Employee) =>
    call<FloorView>("employee_save", { path, employee });

export const employeeRemove = (path: string, slug: string) => call<FloorView>("employee_remove", { path, slug });

export const sessionStart = (path: string, slug: string, firstMessage: string | null, rows: number, cols: number) =>
    call<SessionView>("session_start", { path, slug, firstMessage, rows, cols });

export const sessionResume = (path: string, slug: string, message: string | null, rows: number, cols: number) =>
    call<SessionView>("session_resume", { path, slug, message, rows, cols });

export const sessionStop = (sessionId: string) => call<void>("session_stop", { sessionId });

/**
 * Attach to the output of a session. Returns the base64 scrollback. After that, each output
 * message goes to `onOutput` in order. The channel can deliver a message before this promise
 * resolves, so the caller must write the scrollback first and then the early messages.
 */
export async function sessionAttach(
    sessionId: string,
    onOutput: (message: OutputMessage) => void,
): Promise<AttachResult> {
    const b = await backend();
    const channel = b.newChannel(onOutput);
    return b.invoke<AttachResult>("session_attach", { sessionId, channel });
}

/** `data` is base64 of the bytes that xterm.js gave. */
export const ptyWrite = (sessionId: string, data: string) => call<void>("pty_write", { sessionId, data });

export const ptyResize = (sessionId: string, rows: number, cols: number) =>
    call<void>("pty_resize", { sessionId, rows, cols });

export const clearDraft = (sessionId: string) => call<void>("clear_draft", { sessionId });

/** Rejects with the refusal text, for example `REFUSED: status question`. */
export const debugPaste = (sessionId: string, text: string) => call<void>("debug_paste", { sessionId, text });

export const programsGet = () => call<Programs>("programs_get");

export const programsSet = (name: ProgramName, path: string) => call<Programs>("programs_set", { name, path });

// ---- Commands (docs/ipc-visuals-init.md, section 4) ----

/** Base64 of `logo.png`, or null when the floor has no logo. */
export const floorLogo = (path: string) => call<string | null>("floor_logo", { path });

/**
 * Returns at once. The generation runs in the background and `floor-changed` follows. A non-null
 * `prompt` is stored as `[brand] logo_prompt` first. Rejects with `NO_KEY:` or `BUSY:` text.
 */
export const logoGenerate = (path: string, prompt: string | null) => call<void>("logo_generate", { path, prompt });

export const setupStart = (path: string, cli: CliKind, rows: number, cols: number) =>
    call<SessionView>("setup_start", { path, cli, rows, cols });

export const settingsGet = () => call<Settings>("settings_get");

/** A null argument means "no change". An empty `openaiKey` removes the key from the app config. */
export const settingsSet = (openaiKey: string | null, imageModel: string | null) =>
    call<Settings>("settings_set", { openaiKey, imageModel });

// ---- Commands (docs/ipc-settings.md, section 4) ----

/** The Rust side reads the Codex file again on each call. */
export const modelsGet = () => call<ModelCatalog>("models_get");

/** Replaces the custom models of one kind. Rejects with a text for a bad name. */
export const modelsSetCustom = (kind: ModelKind, models: string[]) =>
    call<ModelCatalog>("models_set_custom", { kind, models });

/** Writes `[defaults]` of `floor.toml`. A null field goes out of the file. */
export const floorDefaultsSet = (path: string, defaults: FloorDefaults) =>
    call<FloorView>("floor_defaults_set", { path, defaults });

// ---- Events (docs/ipc-m1.md, "Events") ----

export async function onSessionChanged(handler: (payload: SessionChangedPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<SessionChangedPayload>("session-changed", handler);
}

export async function onFloorChanged(handler: (payload: FloorChangedPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<FloorChangedPayload>("floor-changed", handler);
}
