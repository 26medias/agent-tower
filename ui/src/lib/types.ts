// Types copied from docs/ipc-m1.md ("Types" and "Events"), docs/ipc-visuals-init.md (section 3)
// and docs/ipc-settings.md (section 3).
// Keep in step with those files.

export type Status = "starting" | "working" | "idle" | "question" | "done" | "stopped";
export type CliKind = "claude" | "codex";
export type Policy = "read_only" | "edit_in_worktree" | "ask_always";

export interface FloorEntry {
    path: string;
    name: string | null;
    missing: boolean;
    logoVersion: number | null; // modification time of logo.png in ms, null when there is no logo
}

export interface Employee {
    slug: string; // [a-z0-9][a-z0-9-]*, unique in the floor
    name: string;
    role: string; // "CEO", "Project Manager", "Engineer", ...
    manager: string | null; // slug of the manager
    cli: CliKind;
    model: string | null;
    effort: string | null;
    policy: Policy;
    roleText: string;
}

export interface SessionView {
    sessionId: string; // app session ID
    slug: string; // employee
    status: Status;
    draft: boolean; // the operator has unsent text in this terminal
    messageWaiting: boolean; // a paste was refused and waits
    canResume: boolean; // stopped, and a CLI session ID is stored
}

/** `[defaults]` of `floor.toml`. A null field is not in the file. */
export interface FloorDefaults {
    cli: CliKind | null;
    model: string | null;
    effort: string | null;
    policy: Policy | null;
}

export interface FloorView {
    path: string;
    name: string;
    employees: Employee[];
    employeeErrors: string[]; // employee files that did not load
    sessions: SessionView[]; // one for each employee that has a live or a last session
    description: string | null; // [brand] description
    logoPrompt: string | null; // [brand] logo_prompt
    logoVersion: number | null;
    logoBusy: boolean; // a logo generation runs now for this floor
    logoError: string | null; // the error text of the last failed generation (in memory only)
    setupSessionId: string | null; // the live setup session, if one runs
    defaults: FloorDefaults; // [defaults] of floor.toml
}

/** `SessionView.slug` of the setup session. The slug is reserved: `employee_save` refuses it. */
export const SETUP_SLUG = "setup";

export interface Settings {
    openaiKeySet: boolean;
    openaiKeySource: "config" | "env" | null;
    imageModel: string;
}

export interface ModelOption {
    id: string; // the value that goes to the CLI
    label: string; // text for the list
    efforts: string[]; // empty for an image model
    defaultEffort: string | null;
    custom: boolean; // from the [models] table of the app config
}

export interface ModelCatalog {
    claude: ModelOption[];
    codex: ModelOption[];
    image: ModelOption[];
    codexSource: "cache" | "builtin";
}

/** The lists that take custom models. */
export type ModelKind = "claude" | "codex" | "image";

export interface Programs {
    claude: string | null;
    codex: string | null;
    node: string | null;
    hook: string;
}

/** Message on the output channel of `session_attach`: base64 of the output bytes, in order. */
export interface OutputMessage {
    data: string;
}

/** Result of `session_attach`. `scrollback` is base64. */
export interface AttachResult {
    scrollback: string;
}

/** Payload of the `session-changed` event. */
export interface SessionChangedPayload {
    path: string;
    session: SessionView;
}

/** Payload of the `floor-changed` event. */
export interface FloorChangedPayload {
    path: string;
}

/** Payload of the `employee-started` event (docs/ipc-m1.md, "Events"). */
export interface EmployeeStartedPayload {
    path: string;
    sessionId: string; // the new session. Its `session-changed` events come as for any session
    slug: string; // the employee that was started
    name: string;
    bySlug: string; // the employee that started it
    byName: string;
    resumed: boolean; // the last CLI session was resumed (not a fresh start)
}

/** Names accepted by `programs_set`. */
export type ProgramName = "claude" | "codex" | "node";

/** A session that the UI tracks, with the floor it belongs to. UI type, not part of the contract. */
export interface TrackedSession extends SessionView {
    path: string;
}

export const ALL_STATUSES: Status[] = ["starting", "working", "idle", "question", "done", "stopped"];
