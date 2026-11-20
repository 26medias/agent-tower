// Types copied from docs/ipc-m1.md ("Types" and "Events") and docs/ipc-visuals-init.md (section 3).
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
}

/** `SessionView.slug` of the setup session. The slug is reserved: `employee_save` refuses it. */
export const SETUP_SLUG = "setup";

export interface Settings {
    openaiKeySet: boolean;
    openaiKeySource: "config" | "env" | null;
    imageModel: string;
}

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

/** Names accepted by `programs_set`. */
export type ProgramName = "claude" | "codex" | "node";

/** A session that the UI tracks, with the floor it belongs to. UI type, not part of the contract. */
export interface TrackedSession extends SessionView {
    path: string;
}

export const ALL_STATUSES: Status[] = ["starting", "working", "idle", "question", "done", "stopped"];
