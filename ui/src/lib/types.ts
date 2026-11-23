// Types copied from docs/ipc-final.md (`gone`), docs/ipc-m3.md (worktrees, the restore of the team, `[permissions]`), docs/ipc-m2.md (tasks),
// docs/ipc-m1.md ("Types" and "Events"), docs/ipc-visuals-init.md (section 3) and docs/ipc-settings.md (section 3).
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
    resumable: string[]; // employees that were live at the last close and can resume now, managers first (M3)
    resumeProgress: ResumeProgress | null; // the background resume of the last team, null when none runs (M3)
    gone: boolean; // the folder of the floor is gone (docs/ipc-final.md). false in normal use
    requests: OperatorRequest[]; // the open requests for the operator, oldest first (docs/ipc-final.md). A missing field is an empty list
}

/**
 * A request of a blocked employee for the operator (docs/ipc-final.md, "Requests for the operator"). The employee calls
 * the MCP tool `ask_operator`. The request stays until the operator clears it. There is one request for each employee.
 */
export interface OperatorRequest {
    id: number; // unique in this app run
    slug: string; // the employee that asks
    name: string;
    role: string;
    text: string; // one to 500 characters, no control character except newline
    at: number; // Unix ms
}

/** Payload of the `operator-request` event: sent one time when a request is made, after `floor-changed`. */
export type OperatorRequestPayload = { path: string } & OperatorRequest;

/** How far the resume of the last team is. `done` counts the starts that were tried (docs/ipc-m3.md). */
export interface ResumeProgress {
    done: number;
    total: number;
}

/** `SessionView.slug` of the setup session. The slug is reserved: `employee_save` refuses it. */
export const SETUP_SLUG = "setup";

/** The stored state of the setting "Codex sandbox". */
export type CodexSandboxMode = "auto" | "on" | "off";

/** What a Codex start gets now (see `Settings.codexSandboxEffective`). */
export type CodexSandboxEffective = "on" | "off" | "refused";

export interface Settings {
    openaiKeySet: boolean;
    openaiKeySource: "config" | "env" | null;
    imageModel: string;
    /** `[interface] notify_on_question` of the app config. The default is true. */
    notifyOnQuestion: boolean;
    /** `[claude] auto_mode` of the app config: Claude employees start in the permission mode `auto`. The default is true. */
    claudeAutoMode?: boolean;
    /**
     * `[codex] sandbox` of the app config as it is stored: `auto` (no key, the default), `on` ("Always") or `off`
     * ("Never"). It applies at the next start of a session.
     */
    codexSandboxMode: CodexSandboxMode;
    /**
     * What a Codex start gets now: `on` (the sandbox profile), `off` (full access: the mode is `off`, or the mode is
     * `auto` and the sandbox does not work) or `refused` (the mode is `on` and the sandbox does not work, so a Codex start
     * is refused). The app takes it from the last probe.
     */
    codexSandboxEffective: CodexSandboxEffective;
    /** The old boolean field. Since 2026-11-21 it is the EFFECTIVE state: true when `codexSandboxEffective` is `on`. */
    codexSandbox: boolean;
    /** The home folder of the app. The "Browse..." dialog opens there when no floor gives a better place. Null when unknown. */
    homeDir: string | null;
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
    /** The optional program `rtk`. Null when the app finds none. It has no override. */
    rtk: string | null;
    /** The path that the operator set for each program. Null: no override (the app searches). */
    overrides: Record<ProgramName, string | null>;
}

/** Result of `program_test`: `<path> --version` with the environment of a session. */
export interface ProgramTestResult {
    ok: boolean;
    /** The path that was tried. Null when the app found no program with this name. */
    path: string | null;
    /** The first line of the output, or the error text. At most 200 characters. */
    output: string;
}

/** Result of `codex_sandbox_probe`: `bwrap --unshare-user --ro-bind / / true` with the environment of a session. */
export interface SandboxProbe {
    /** `works`, `fails` (bwrap is there but it failed or hung), or `missing` (no bwrap in the PATH). */
    state: "works" | "fails" | "missing";
    /** One line: the path of bwrap, the first line of its error, or a short sentence. */
    detail: string;
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

/** Payload of the `message-delivered` event (docs/ipc-final.md): the app delivered a message to a session. */
export interface MessageDeliveredPayload {
    path: string;
    fromSlug: string; // "operator" for a task that the operator gave
    fromName: string;
    toSlug: string;
    toName: string;
    kind: "message" | "task";
    taskId: number | null; // the task of a "task" message
    /** A task status message: the status it names ("review"). "" when unknown. null: an assignment, or not a task message. */
    taskStatus: string | null;
    preview: string; // the first 80 characters of the body, one line. Never the full text
}

/** Payload of the `tasks-changed` event (docs/ipc-m2.md). */
export interface TasksChangedPayload {
    path: string;
}

/**
 * Kinds of the `notice` event that the UI knows (docs/ipc-m3.md and docs/ipc-final.md). The Rust side
 * can send another kind later: the UI shows it as an information toast.
 */
export type NoticeKind =
    | "task_blocked"
    | "resume_failed"
    | "sandbox_down"
    | "sandbox_auto_off"
    | "task_done_unmerged"
    | "exchange_stopped"
    | "floor_gone"
    | "floor_back";

/** Payload of the `notice` event (docs/ipc-m3.md): something the operator must see. */
export interface NoticePayload {
    path: string;
    kind: NoticeKind | string;
    text: string;
    /** The employee that the notice is about. Only `exchange_stopped` has it: the employee that the app refused. */
    slug?: string;
}

/**
 * Result of `floor_permissions_set` (docs/ipc-m3.md, "Where the rules are"). `allow` is the stored list.
 * Each warning is `<rule>: This rule lets each agent of the floor run the project's own code with no question.`
 */
export interface PermissionsSaved {
    allow: string[];
    warnings: string[];
}

export type TaskStatus = "todo" | "in_progress" | "blocked" | "review" | "done";

/** A task of a floor (docs/ipc-m2.md). */
export interface TaskView {
    id: number; // the number in the floor. The UI shows "T-" + id
    title: string;
    description: string;
    parent: number | null;
    creator: string; // an employee slug, or "operator"
    assignee: string | null;
    assigner: string | null; // an employee slug, or "operator"
    status: TaskStatus;
    report: string | null; // relative to the floor folder: ".swarm/reports/T-12.md"
    createdAt: number; // Unix ms
    updatedAt: number; // Unix ms
    branch: string | null; // the git branch of the task, "task/T-12-add-login" (docs/ipc-m3.md)
    baseBranch: string | null; // the branch of the main checkout when the worktree was made
    baseCommit: string | null; // the commit of the main checkout when the worktree was made
    worktree: string | null; // relative to the floor folder: ".swarm/worktrees/T-12". null when there is none now
}

/** The result of `task_merge`. A merge that is blocked has `merged` false and the task is `blocked`. */
export interface TaskMerge {
    merged: boolean;
    message: string;
    task: TaskView;
}

/** The result of `task_diff`. */
export interface TaskDiff {
    branch: string | null;
    baseBranch: string | null;
    stat: string;
    patch: string;
    truncated: boolean;
    note: string | null;
}

export type TaskEventKind = "created" | "assigned" | "status" | "comment" | "report" | "worktree";

export interface TaskEvent {
    id: number;
    at: number; // Unix ms
    author: string; // an employee slug, or "operator"
    kind: TaskEventKind;
    value: string | null; // status: the new status. assigned: the slug. report: the path
    text: string | null; // status: the note. comment: the comment
}

export interface TaskDetail {
    task: TaskView;
    events: TaskEvent[]; // oldest first
}

/** Names accepted by `programs_set`. */
export type ProgramName = "claude" | "codex" | "node";

/** A session that the UI tracks, with the floor it belongs to. UI type, not part of the contract. */
export interface TrackedSession extends SessionView {
    path: string;
}

export const ALL_STATUSES: Status[] = ["starting", "working", "idle", "question", "done", "stopped"];
