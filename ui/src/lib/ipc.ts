// The only module that talks to the Rust side. One typed function for each command of
// docs/ipc-m1.md. Inside Tauri it uses the real invoke/listen/Channel. In a normal browser
// (no `window.__TAURI_INTERNALS__`) it uses the in-memory mock of ./mock.ts.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
    AttachResult,
    CliKind,
    EmployeeStartedPayload,
    FloorChangedPayload,
    FloorDefaults,
    FloorEntry,
    FloorView,
    Employee,
    MessageDeliveredPayload,
    ModelCatalog,
    ModelKind,
    OperatorRequestPayload,
    OutputMessage,
    PermissionsSaved,
    ProgramName,
    ProgramTestResult,
    SandboxProbe,
    CodexSandboxMode,
    Programs,
    SessionChangedPayload,
    NoticePayload,
    ResumeProgress,
    SessionView,
    Settings,
    TaskDetail,
    TaskDiff,
    TaskMerge,
    TaskStatus,
    TaskView,
    TasksChangedPayload,
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

/**
 * Set the title of the window. Inside Tauri this sets the real window title (the permission
 * `core:window:allow-set-title` is in src-tauri/capabilities/default.json). In a browser it sets
 * `document.title`, which the browser shows in the tab.
 */
export async function setWindowTitle(title: string): Promise<void> {
    if (typeof document !== "undefined") document.title = title;
    if (!isTauri()) return;
    try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().setTitle(title);
    } catch {
        // The title is a help for the operator. A refusal must not break the UI.
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

/** An empty `path` removes the override (the "reset" button): the app then searches again. */
export const programsSet = (name: ProgramName, path: string) => call<Programs>("programs_set", { name, path });

/** Runs `<path> --version` with the environment of a session. It starts no model session. */
export const programTest = (name: ProgramName) => call<ProgramTestResult>("program_test", { name });

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

/**
 * A null argument means "no change". An empty `openaiKey` removes the key from the app config.
 * `notifyOnQuestion` is the switch of the desktop notification (`[interface]` of the app config).
 */
export const settingsSet = (
    openaiKey: string | null,
    imageModel: string | null,
    notifyOnQuestion: boolean | null = null,
    codexSandbox: boolean | null = null,
    codexSandboxMode: CodexSandboxMode | null = null,
) => call<Settings>("settings_set", settingsSetArgs(openaiKey, imageModel, notifyOnQuestion, codexSandbox, codexSandboxMode));

/** The switch "Claude auto mode" (`[claude] auto_mode` of the app config). The other settings stay. */
export const settingsSetClaudeAuto = (on: boolean) =>
    call<Settings>("settings_set", { openaiKey: null, imageModel: null, notifyOnQuestion: null, claudeAutoMode: on });

/**
 * The arguments of `settings_set`. `codexSandbox` (the old boolean: true is "on", false is "off") and `codexSandboxMode`
 * (`auto`, `on` or `off`) are left out when they are null: an old call has no such argument, and the app reads a
 * missing one as "no change". When a call has both, the mode wins. The other three always go, as before.
 */
export function settingsSetArgs(
    openaiKey: string | null,
    imageModel: string | null,
    notifyOnQuestion: boolean | null,
    codexSandbox: boolean | null,
    codexSandboxMode: CodexSandboxMode | null = null,
): Record<string, unknown> {
    return {
        openaiKey,
        imageModel,
        notifyOnQuestion,
        ...(codexSandbox === null ? {} : { codexSandbox }),
        ...(codexSandboxMode === null ? {} : { codexSandboxMode }),
    };
}

/**
 * Runs `bwrap --unshare-user --ro-bind / / true` with the environment of a session (5 s at most). It starts no Codex
 * session. Choosing "Always" (`codexSandboxMode: "on"` or `codexSandbox: true`) in `settingsSet` runs this probe again, and the
 * app refuses it unless it says `works`.
 */
export const codexSandboxProbe = () => call<SandboxProbe>("codex_sandbox_probe");

// ---- Commands (docs/ipc-settings.md, section 4) ----

/** The Rust side reads the Codex file again on each call. */
export const modelsGet = () => call<ModelCatalog>("models_get");

/** Replaces the custom models of one kind. Rejects with a text for a bad name. */
export const modelsSetCustom = (kind: ModelKind, models: string[]) =>
    call<ModelCatalog>("models_set_custom", { kind, models });

/** Writes `[defaults]` of `floor.toml`. A null field goes out of the file. */
export const floorDefaultsSet = (path: string, defaults: FloorDefaults) =>
    call<FloorView>("floor_defaults_set", { path, defaults });

// ---- Commands (docs/ipc-m2.md, the task board) ----

export const taskList = (path: string) => call<TaskView[]>("task_list", { path });

export const taskGet = (path: string, id: number) => call<TaskDetail>("task_get", { path, id });

export const taskCreate = (path: string, title: string, description: string) =>
    call<TaskView>("task_create", { path, title, description });

export const taskAssign = (path: string, id: number, slug: string) => call<TaskView>("task_assign", { path, id, slug });

export const taskUpdate = (path: string, id: number, status: TaskStatus, note: string | null) =>
    call<TaskView>("task_update", { path, id, status, note });

/** The text of the report file, or null when the task has no report. Rejects with a text. */
export const taskReport = (path: string, id: number) => call<string | null>("task_report", { path, id });

// ---- Commands (docs/ipc-m3.md, worktrees, the restore of the team, `[permissions]`) ----

/** Merge the branch of a task. A blocked merge is a result (`merged: false`), not a rejection. */
export const taskMerge = (path: string, id: number) => call<TaskMerge>("task_merge", { path, id });

/** The diffstat and the patch of the branch of a task (the patch is cut at 60000 bytes). */
export const taskDiff = (path: string, id: number) => call<TaskDiff>("task_diff", { path, id });

/** Remove the worktree and the branch of a task that is not done, and set the status `blocked`. */
export const taskDiscard = (path: string, id: number) => call<TaskView>("task_discard", { path, id });

/** The rules of the floor, as stored in the app config (not in the project folder). */
export const floorPermissionsGet = (path: string) => call<string[]>("floor_permissions_get", { path });

/**
 * Writes the rules in the app config (not in the project). A bad rule rejects the whole call. Returns the
 * stored list and one warning for each rule that runs the code of the project.
 */
export const floorPermissionsSet = (path: string, allow: string[]) =>
    call<PermissionsSaved>("floor_permissions_set", { path, allow });

/**
 * The rules of an old `[permissions]` table in the `floor.toml` of the project that pass the check. Empty in
 * normal use. An agent can have written them: the operator must read them and click "Use these rules".
 */
export const floorPermissionsProposed = (path: string) => call<string[]>("floor_permissions_proposed", { path });

/** Resume the employees of `FloorView.resumable` in the background. Returns the progress at the start. */
export const teamResume = (path: string) => call<ResumeProgress>("team_resume", { path });

// ---- Commands (docs/ipc-final.md, "Requests for the operator") ----

/** Remove a request of an employee. An unknown `id` is not an error. `floor-changed` follows. */
export const operatorRequestClear = (path: string, id: number) => call<null>("operator_request_clear", { path, id });

// ---- Events (docs/ipc-m1.md, "Events") ----

export async function onSessionChanged(handler: (payload: SessionChangedPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<SessionChangedPayload>("session-changed", handler);
}

export async function onFloorChanged(handler: (payload: FloorChangedPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<FloorChangedPayload>("floor-changed", handler);
}

/** An employee started the session of another employee with the tools of the app. */
export async function onEmployeeStarted(handler: (payload: EmployeeStartedPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<EmployeeStartedPayload>("employee-started", handler);
}

/** Something the operator must see: a blocked merge, or a start that the team resume could not do. */
export async function onNotice(handler: (payload: NoticePayload) => void): Promise<Unlisten> {
    return (await backend()).listen<NoticePayload>("notice", handler);
}

/** A task of a floor was made or changed (by an employee or by the operator). */
export async function onTasksChanged(handler: (payload: TasksChangedPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<TasksChangedPayload>("tasks-changed", handler);
}

/** The app delivered a message to the session of an employee (a paste, the Stop hook, `inbox` or a first message). */
export async function onMessageDelivered(handler: (payload: MessageDeliveredPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<MessageDeliveredPayload>("message-delivered", handler);
}

/** An employee asked the operator for something (the MCP tool `ask_operator`). `floor-changed` came before it. */
export async function onOperatorRequest(handler: (payload: OperatorRequestPayload) => void): Promise<Unlisten> {
    return (await backend()).listen<OperatorRequestPayload>("operator-request", handler);
}
