// The one store of the UI: floors, the open floor, sessions by ID, the active tab, the selected
// employee, logos, settings, toasts.
// Svelte 5 runes. The pure state changes live in ./reducers.ts.
import { tick } from "svelte";
import {
    ACTIVITY_KEPT,
    appendActivity,
    messageDraft,
    sessionDrafts,
    startedDraft,
    taskDrafts,
    type ActivityDraft,
    type ActivityEntry,
} from "./activity";
import * as ipc from "./ipc";
import { blockedTaskId, describeError, isPathError, noticePlan, tidyError, type ErrorAction } from "./messages";
import { modalOpen, onModalChange } from "./modal";
import { orderEmployees } from "./order";
import { focusTerminal } from "./terminal";
import { taskLabel } from "./tasks";
import {
    applyFloorView,
    applySessionChanged,
    dismissTab,
    emptySessionState,
    employeeLabel,
    employeeStartedText,
    finishedTurns,
    isLive,
    nextAfter,
    pickActive,
    questionCount,
    questionOrder,
    trackQuestions,
    sessionOf,
    tabSessions,
    upsertSession,
    type SessionState,
} from "./reducers";
import {
    SETUP_SLUG,
    type CliKind,
    type Employee,
    type FloorDefaults,
    type FloorEntry,
    type FloorView,
    type MessageDeliveredPayload,
    type ModelCatalog,
    type ModelKind,
    type NoticePayload,
    type ProgramName,
    type ProgramTestResult,
    type Programs,
    type SandboxProbe,
    type CodexSandboxMode,
    type SessionView,
    type Settings,
    type TaskDetail,
    type TaskDiff,
    type TaskMerge,
    type TaskStatus,
    type TaskView,
    type TrackedSession,
} from "./types";

export interface Toast {
    id: number;
    text: string;
    /** `info` is a plain notice. No kind is an error. */
    kind?: "info";
    /** A button that the toast offers, for an error that the operator can fix in the UI. */
    action?: { label: string; run: () => void };
    /** The task that this toast is about (a blocked merge). It closes by itself while the detail of that task is open. */
    task?: { path: string; id: number };
}

/** A session that waits for an answer, with the words that the attention bar shows. */
export interface Question {
    sessionId: string;
    path: string;
    slug: string;
    /** "Eli (Engineer)", or "Setup agent". */
    label: string;
    floorName: string;
}

/** The title of the window. `(2) Agent Tower` while two sessions ask a question. */
export function windowTitleFor(questions: number): string {
    return questions > 0 ? `(${questions}) Agent Tower` : "Agent Tower";
}

/** The field that the operator typed an "open folder" path in. The error of a bad path shows under that field. */
export type OpenSource = "column" | "empty";

/** Why a control that writes in the open floor is disabled while the folder of the floor is gone. */
export const GONE_REASON = "The folder of this floor is gone. The app cannot change this floor.";

/** What a button was clicked to do. A start, a resume or the setup agent. */
export type LaunchKind = "start" | "resume" | "setup";

const LAST_FLOOR_KEY = "agent-tower:last-floor";
const DEBUG_KEY = "agent-tower:debug";
/** An information toast closes by itself after this time. An error toast stays until the operator closes it. */
export const TOAST_INFO_MS = 6000;
/** The toast of a blocked merge closes by itself after this time while the detail of its task is open (the detail shows the reason). */
export const TOAST_TASK_MS = 8000;
/** The "MSG" mark on the tab of the receiver of a message shows this long, when the tab is not in view. */
export const MSG_MARK_MS = 10_000;
/** The overlay shows this many toasts. The newest is the first (at the top). */
export const TOASTS_VISIBLE = 4;
/** The store keeps this many toasts. The oldest go first. */
const TOASTS_KEPT = 20;

function readLastFloor(): string | null {
    try {
        return localStorage.getItem(LAST_FLOOR_KEY);
    } catch {
        return null;
    }
}

function readDebug(): boolean {
    try {
        return localStorage.getItem(DEBUG_KEY) === "1";
    } catch {
        return false;
    }
}

function writeDebug(on: boolean): void {
    try {
        localStorage.setItem(DEBUG_KEY, on ? "1" : "0");
    } catch {
        // Storage can be blocked. The choice works for this run only.
    }
}

function writeLastFloor(path: string): void {
    try {
        localStorage.setItem(LAST_FLOOR_KEY, path);
    } catch {
        // Storage can be blocked. The UI works without it.
    }
}

class Store {
    floors = $state.raw<FloorEntry[]>([]);
    openPath = $state<string | null>(null);
    floorView = $state.raw<FloorView | null>(null);
    sessionState = $state.raw<SessionState>(emptySessionState);
    activeByPath = $state.raw<Record<string, string>>({});
    toasts = $state.raw<Toast[]>([]);
    programs = $state.raw<Programs | null>(null);
    /** The last `program_test` result of each program. A change of the path clears it. */
    programTests = $state.raw<Record<ProgramName, ProgramTestResult | null>>({ claude: null, codex: null, node: null });
    /** A `program_test` runs now for this program. */
    programTesting = $state.raw<Record<ProgramName, boolean>>({ claude: false, codex: false, node: false });
    settings = $state.raw<Settings | null>(null);
    /** The last `codex_sandbox_probe` result. Null until the first test ends. */
    sandboxProbe = $state.raw<SandboxProbe | null>(null);
    /** A `codex_sandbox_probe` runs now (it takes 5 s at most). */
    sandboxTesting = $state(false);
    /** The model lists (docs/ipc-settings.md). Null until the first `models_get` answers. */
    models = $state.raw<ModelCatalog | null>(null);
    /** The settings panel is open. */
    settingsOpen = $state(false);
    /** A section of the settings panel to scroll to when it opens. */
    settingsSection = $state<string | null>(null);
    /** The debug paste bar shows under the terminal. Off by default: the operator does not need it. */
    debugVisible = $state(readDebug());
    /** The reader overlay of the open task: the report text or the diff. */
    reader = $state<"report" | "diff" | null>(null);
    /** The employees of each floor that the UI opened. The attention bar names sessions of any floor with it. */
    employeesByPath = $state.raw<Record<string, Employee[]>>({});
    /** The lines of the "Team activity" list, for all floors, oldest first. The list shows the open floor only. */
    activity = $state.raw<ActivityEntry[]>([]);
    private activitySeq = 0;
    /** The order in which sessions started to ask a question, by session ID. Lower is older. */
    private questionSince = $state.raw<Record<string, number>>({});
    private questionSeq = 0;
    /** Sessions that finished a turn while their tab was not in view, by session ID. */
    private unread = $state.raw<Record<string, true>>({});
    /** The session that the operator typed in last. */
    private lastInputId: string | null = null;
    /** The question that "Open" opened last. The next click opens the next one. */
    private lastOpenedQuestion = $state<string | null>(null);
    /** Logo of each floor as a `data:` URL, by floor path. A floor without a logo has no entry. */
    logos = $state.raw<Record<string, string>>({});
    /** The employee that the operator selected, by floor path. */
    selectedByPath = $state.raw<Record<string, string>>({});
    /** Text of a rejected `logo_generate` call (`NO_KEY:` or `BUSY:`), by floor path. */
    logoCallError = $state.raw<Record<string, string>>({});
    /** Set when `floor_open` failed with `NO_FLOOR:`. The UI offers to create the floor. */
    pendingCreate = $state.raw<{ path: string; message: string } | null>(null);
    /**
     * Set when `floor_open` of a typed path failed with `NOT_ABSOLUTE:`, `NOT_FOUND:`, `NOT_A_FOLDER:`
     * or `NO_WRITE:`. `text` is for a person. The field `source` shows it under itself. No "create" is offered.
     */
    openError = $state.raw<{ source: OpenSource; path: string; text: string } | null>(null);
    /** The tasks of each floor that the UI has loaded, oldest first. */
    tasksByPath = $state.raw<Record<string, TaskView[]>>({});
    /** The task whose detail panel is open, by floor path. */
    openTaskByPath = $state.raw<Record<string, number>>({});
    /** The detail of the open task. `report` is `undefined` while it loads, `null` when the task has none. */
    taskDetail = $state.raw<{ path: string; detail: TaskDetail; report: string | null | undefined; reportError: string | null } | null>(
        null,
    );
    /** The diff box of the open task (docs/ipc-m3.md). `diff` is null while it loads. */
    taskDiff = $state.raw<{ path: string; id: number; diff: TaskDiff | null; error: string | null } | null>(null);
    /** The rules of each floor that the settings panel loaded. The app stores them in its own config, not in the project. */
    permissionsByPath = $state.raw<Record<string, string[]>>({});
    /** The warnings of the last save of the rules of each floor (docs/ipc-m3.md): `<rule>: <text>`. A save replaces them. */
    permissionWarningsByPath = $state.raw<Record<string, string[]>>({});
    /** The rules of an old `[permissions]` table in the project that the operator can choose to use, by floor path. */
    proposedByPath = $state.raw<Record<string, string[]>>({});
    backend = $state<"tauri" | "mock" | "">("");
    ready = $state(false);
    private busy = $state.raw<Record<string, LaunchKind>>({});
    /** The CLI and the name that a start was made with, by floor path and slug. For the loading text. */
    private launched = $state.raw<Record<string, { cli: CliKind; name: string }>>({});

    /** The size of the last terminal that fitted. New sessions start with it. */
    lastSize = { rows: 30, cols: 110 };

    private toastId = 0;
    private unlisten: ipc.Unlisten[] = [];
    /** Version of the logo that was loaded (or asked for) for each floor. */
    private logoLoaded = new Map<string, number | null>();

    /** The folder of the open floor is gone (`FloorView.gone`). The controls that write are disabled. */
    gone = $derived(this.floorView?.gone === true);

    /** The tasks of the open floor. */
    tasks = $derived(this.openPath ? (this.tasksByPath[this.openPath] ?? []) : []);

    /** The ID of the task that is open in the detail panel of the open floor. */
    openTaskId = $derived(this.openPath ? (this.openTaskByPath[this.openPath] ?? null) : null);

    /** Tabs of the open floor, in a stable order. */
    tabs = $derived.by(() => {
        const path = this.openPath;
        return tabSessions(this.sessionState)
            .filter((s) => s.path === path)
            .sort((a, b) => this.employeeIndex(a.slug) - this.employeeIndex(b.slug));
    });

    /** The employees of the open floor in the order of the hierarchy. Cards and tabs use it. */
    orderedEmployees = $derived.by(() => {
        const view = this.floorView;
        return view ? orderEmployees(view.employees) : [];
    });

    /** Sessions that need a terminal instance: every tab of every floor. */
    allTabSessions = $derived(tabSessions(this.sessionState));

    activeId = $derived.by(() => {
        const path = this.openPath;
        if (!path) return null;
        return pickActive(this.tabs, this.activeByPath[path] ?? null);
    });

    /** True when the app can resume this session (it had a turn). The stopped note of a pane uses it. */
    sessionCanResume(sessionId: string): boolean {
        return this.sessionState.byId[sessionId]?.canResume === true;
    }

    activeSession = $derived(this.activeId ? (this.sessionState.byId[this.activeId] ?? null) : null);

    /** The selected employee of the open floor, if it still exists. */
    selectedEmployee = $derived.by(() => {
        const path = this.openPath;
        const slug = path ? this.selectedByPath[path] : undefined;
        return slug ? (this.orderedEmployees.find((e) => e.slug === slug) ?? null) : null;
    });

    /** The live or last setup session of the open floor, when the operator selected it (its tab). */
    selectedSetup = $derived.by((): TrackedSession | null => {
        const path = this.openPath;
        if (!path || this.selectedByPath[path] !== SETUP_SLUG) return null;
        return sessionOf(this.sessionState, path, SETUP_SLUG) ?? null;
    });

    private employeeIndex(slug: string): number {
        const i = this.orderedEmployees.findIndex((e) => e.slug === slug);
        return i < 0 ? Number.MAX_SAFE_INTEGER : i;
    }

    // ---- start up ----

    async init(): Promise<void> {
        this.backend = await ipc.backendKind();
        try {
            this.unlisten.push(
                await ipc.onSessionChanged((payload) => {
                    this.commitSessions(applySessionChanged(this.sessionState, payload), true);
                }),
                // The tab and the status of the new session come from `session-changed`, as for a
                // session that the operator starts. This event adds only the notice. It does not
                // change the active tab: the operator keeps the tab that is open.
                await ipc.onEmployeeStarted((payload) => {
                    this.toast(employeeStartedText(payload), "info");
                    // "Ada started Lin" says more than the line "Lin started" of the same session. Keep only the first.
                    this.activity = this.activity.filter((e) => !(e.sessionId === payload.sessionId && e.kind === "session" && e.words === "started"));
                    this.addActivity([startedDraft(payload.path, payload.byName, payload.name, payload.sessionId, payload.slug)]);
                }),
                await ipc.onTasksChanged((payload) => {
                    // The tasks of another floor load when the operator opens that floor.
                    if (payload.path === this.openPath) void this.loadTasks(payload.path);
                }),
                // A message that the app delivered: a row in "Team activity", and a short mark on the tab of the receiver.
                await ipc.onMessageDelivered((payload) => this.onMessageDelivered(payload)),
                // A merge that is blocked, or a start that the team resume could not do.
                await ipc.onNotice((payload) => this.onNotice(payload)),
                await ipc.onFloorChanged((payload) => {
                    // The list carries the logo version of every floor.
                    void this.loadFloors();
                    if (payload.path === this.openPath) void this.reloadFloor();
                }),
            );
        } catch (e) {
            this.toast(`Cannot listen to events: ${ipc.errorText(e)}`);
        }
        await this.loadFloors();
        void this.loadPrograms();
        void this.loadSettings();
        void this.loadModels();
        const last = readLastFloor();
        const first = this.floors.find((f) => f.path === last && !f.missing) ?? this.floors.find((f) => !f.missing);
        if (first) await this.openFloor(first.path);
        this.ready = true;
    }

    dispose(): void {
        for (const u of this.unlisten) u();
        this.unlisten = [];
    }

    // ---- toasts ----

    /**
     * Show a toast. An information toast closes after `TOAST_INFO_MS`, unless `stay` is true. An
     * error toast stays until the operator closes it. An error is shown as text for a person (see
     * messages.ts), and a problem with a program path offers a button for Settings. The same text
     * twice shows once. `buttons` is a button that the toast offers (an information toast can have one).
     */
    toast(text: string, kind?: "info", stay = false, buttons?: Toast["action"], task?: Toast["task"]): number {
        const id = ++this.toastId;
        let shown = text;
        let action: Toast["action"] = buttons;
        if (kind !== "info") {
            const described = describeError(text);
            shown = described.text;
            action = this.actionFor(described.action);
        }
        const same = this.toasts.filter((t) => !(t.text === shown && t.kind === kind));
        for (const t of this.toasts) if (!same.includes(t)) this.stopInfoTimer(t.id);
        this.toasts = [...same, { id, text: shown, kind, action, task }].slice(-TOASTS_KEPT);
        if (kind === "info" && !stay) this.startInfoTimer(id, TOAST_INFO_MS);
        if (task) this.closeTaskToastLater(id);
        return id;
    }

    /**
     * The timers of the information toasts. A timer pauses while a modal is open: the toast waits under the
     * backdrop, so the operator could not read it. It runs again, with the time that is left, when the last
     * modal closes. `since` is the start of the run that is in progress, or null while the timer is paused.
     */
    private infoTimers = new Map<number, { left: number; since: number | null; handle: ReturnType<typeof setTimeout> | null }>();

    /** Pause the timers when a modal opens. Run them again when the last modal closes. Clear the errors of the modal actions then. */
    private stopModalWatch = onModalChange((open) => {
        if (open) {
            for (const timer of this.infoTimers.values()) {
                if (timer.handle === null || timer.since === null) continue;
                clearTimeout(timer.handle);
                timer.left = Math.max(0, timer.left - (Date.now() - timer.since));
                timer.handle = null;
                timer.since = null;
            }
        } else {
            for (const id of [...this.infoTimers.keys()]) this.runInfoTimer(id);
            this.actionErrors = {};
        }
    });

    private startInfoTimer(id: number, ms: number): void {
        this.infoTimers.set(id, { left: ms, since: null, handle: null });
        if (!modalOpen()) this.runInfoTimer(id);
    }

    private runInfoTimer(id: number): void {
        const timer = this.infoTimers.get(id);
        if (!timer || timer.handle !== null) return;
        timer.since = Date.now();
        timer.handle = setTimeout(() => this.dismissToast(id), timer.left);
    }

    private stopInfoTimer(id: number): void {
        const timer = this.infoTimers.get(id);
        if (timer?.handle != null) clearTimeout(timer.handle);
        this.infoTimers.delete(id);
    }

    /** Toasts that already have a timer for the detail of their task. */
    private taskTimers = new Set<number>();

    /**
     * A toast about a task closes after `TOAST_TASK_MS` when the detail of that task is open then. The
     * detail already shows the reason, so the toast would only stay in the way. The check runs when the
     * timer ends, so a detail that the operator closes in the meantime keeps the toast.
     */
    private closeTaskToastLater(id: number): void {
        const toast = this.toasts.find((t) => t.id === id);
        if (!toast?.task || this.taskTimers.has(id)) return;
        const { path, id: taskId } = toast.task;
        if (this.openTaskByPath[path] !== taskId) return;
        this.taskTimers.add(id);
        setTimeout(() => {
            this.taskTimers.delete(id);
            const now = this.toasts.find((t) => t.id === id);
            if (now?.task && this.openTaskByPath[path] === taskId) this.dismissToast(id);
        }, TOAST_TASK_MS);
    }

    /**
     * A `notice` event (docs/ipc-m3.md, docs/ipc-final.md). `noticePlan` says how the toast looks.
     * `floor_gone` and `floor_back` also read the list and the open floor again. The app sends
     * `floor-changed` first, which does the same: this keeps the UI right if that event is lost.
     */
    private onNotice(payload: NoticePayload): void {
        const plan = noticePlan(payload.kind);
        // The banner of the open floor already says that its folder is gone. A toast would only
        // cover the buttons of the banner. The toast stays for the other floors.
        const bannerSaysIt = payload.kind === "floor_gone" && payload.path === this.openPath;
        if (bannerSaysIt) {
            // No toast.
        } else if (plan.tone === "error") {
            const taskId = payload.kind === "task_blocked" ? blockedTaskId(payload.text) : null;
            this.toast(payload.text, undefined, false, undefined, taskId === null ? undefined : { path: payload.path, id: taskId });
        } else {
            // The notice of "Automatic" with a sandbox that does not work has a button for the section in Settings.
            const button = payload.kind === "sandbox_auto_off" ? this.actionFor("settings-sandbox") : undefined;
            this.toast(payload.text, "info", plan.stay, button);
        }
        if (payload.kind === "floor_gone" || payload.kind === "floor_back") {
            void this.loadFloors();
            if (payload.path === this.openPath) void this.reloadFloor();
        }
    }

    /** The reason for a control that writes, while the open floor is gone. Else the normal title. */
    goneTitle(normal?: string): string | undefined {
        return this.gone ? GONE_REASON : normal;
    }

    private actionFor(kind: ErrorAction): Toast["action"] {
        if (kind === "settings-programs") return { label: "Open Settings", run: () => this.openSettings("programs") };
        if (kind === "settings-sandbox") return { label: "Open Settings", run: () => this.openSettings("codex-sandbox") };
        return undefined;
    }

    openSettings(section: string | null = null): void {
        this.settingsSection = section;
        this.settingsOpen = true;
    }

    setDebugVisible(on: boolean): void {
        this.debugVisible = on;
        writeDebug(on);
    }

    dismissToast(id: number): void {
        this.stopInfoTimer(id);
        this.toasts = this.toasts.filter((t) => t.id !== id);
    }

    private fail(e: unknown): void {
        this.toast(ipc.errorText(e));
    }

    /**
     * The error of the last action of a modal (the employee form, the settings panel), by area. The modal shows it as a
     * line in the form, because the toast stays under the backdrop while the modal is open. The same text also goes to a
     * toast, as before. The modal clears it before the next try, and it clears at once when the last modal closes.
     */
    actionErrors = $state.raw<Record<string, string>>({});

    /** Clear (`null`) or set the inline error of one area. */
    setActionError(area: string, text: string | null): void {
        if (text === null) {
            if (!(area in this.actionErrors)) return;
            const { [area]: _gone, ...rest } = this.actionErrors;
            this.actionErrors = rest;
        } else this.actionErrors = { ...this.actionErrors, [area]: text };
    }

    /**
     * An action of a modal failed: the error shows in the inline line of `area`, inside the modal. No toast: the
     * operator already sees the text where the action is, and a toast would stay after the modal closes.
     */
    private failIn(area: string, e: unknown): void {
        this.setActionError(area, describeError(ipc.errorText(e)).text);
    }

    // ---- floors ----

    async loadFloors(): Promise<void> {
        try {
            this.floors = await ipc.floorList();
            this.syncLogos();
        } catch (e) {
            this.fail(e);
        }
    }

    private applyView(view: FloorView): void {
        this.floorView = view;
        this.employeesByPath = { ...this.employeesByPath, [view.path]: view.employees };
        this.commitSessions(applyFloorView(this.sessionState, view));
        this.syncLogos();
    }

    // ---- what the operator must see: questions, unread marks, finished turns ----

    /**
     * Put a new session state in place. It compares the old and the new state to keep the
     * question order and the unread marks, and to tell the operator about a turn that finished.
     */
    private commitSessions(next: SessionState, announceNew = false): void {
        const prev = this.sessionState;
        const tracked = trackQuestions(this.questionSince, next, this.questionSeq);
        this.questionSeq = tracked.seq;
        if (tracked.since !== this.questionSince) this.questionSince = tracked.since;
        const finished = finishedTurns(prev, next);
        this.sessionState = next;
        // A question on a floor that the UI never opened is named with the employees of that floor.
        for (const s of Object.values(next.byId)) if (s.status === "question") this.loadEmployeesOf(s.path);
        this.addActivity(sessionDrafts(prev, next, (s) => this.sessionLabel(s), announceNew));
        for (const s of finished) this.turnFinished(s);
        this.markRead();
    }

    /** Add lines to the "Team activity" list. Each line gets an ID and the time of now. */
    private addActivity(drafts: ActivityDraft[]): void {
        if (drafts.length === 0) return;
        // A status row replaces the older status row of the same employee (see `appendActivity`).
        this.activity = appendActivity(this.activity, drafts, Date.now(), () => ++this.activitySeq).slice(-ACTIVITY_KEPT);
    }

    /** The sessions whose tab shows the "MSG" mark now (a message was delivered while the tab was not in view). */
    private msgMarks = $state.raw<Record<string, true>>({});
    private msgTimers = new Map<string, ReturnType<typeof setTimeout>>();

    /** A `message-delivered` event: add the row, and mark the tab of the receiver for `MSG_MARK_MS` when it is not in view. */
    private onMessageDelivered(payload: MessageDeliveredPayload): void {
        this.addActivity([messageDraft(payload)]);
        const session = sessionOf(this.sessionState, payload.path, payload.toSlug);
        if (!session || this.inView(session)) return;
        const id = session.sessionId;
        this.msgMarks = { ...this.msgMarks, [id]: true };
        const old = this.msgTimers.get(id);
        if (old) clearTimeout(old);
        this.msgTimers.set(
            id,
            setTimeout(() => this.clearMsgMark(id), MSG_MARK_MS),
        );
    }

    private clearMsgMark(id: string): void {
        const timer = this.msgTimers.get(id);
        if (timer) clearTimeout(timer);
        this.msgTimers.delete(id);
        if (!this.msgMarks[id]) return;
        const next = { ...this.msgMarks };
        delete next[id];
        this.msgMarks = next;
    }

    /** True for `MSG_MARK_MS` after a message was delivered to this session, while its tab is not in view. */
    hasMsgMark(sessionId: string): boolean {
        const s = this.sessionState.byId[sessionId];
        return !!this.msgMarks[sessionId] && !!s && !this.inView(s);
    }

    private employeeLoads = new Set<string>();

    /**
     * Read the employees of a floor that the UI never opened, so that a question there names the
     * employee ("Bo (Engineer)") and not the slug. `floor_open` only reads the floor and returns its view.
     * The view is not used for anything else: the open floor does not change. An error leaves the
     * slug in the text. Each floor is asked once.
     */
    private loadEmployeesOf(path: string): void {
        if (path in this.employeesByPath || this.employeeLoads.has(path)) return;
        this.employeeLoads.add(path);
        void ipc
            .floorOpen(path)
            .then((view) => {
                if (!(path in this.employeesByPath)) this.employeesByPath = { ...this.employeesByPath, [path]: view.employees };
            })
            .catch(() => {
                // The folder is gone, or the call failed: the text keeps the slug.
            });
    }

    /** The tab of this session is in front of the operator now. */
    private inView(s: { path: string; sessionId: string }): boolean {
        return s.path === this.openPath && s.sessionId === this.activeId;
    }

    private turnFinished(s: TrackedSession): void {
        if (this.inView(s)) return;
        this.unread = { ...this.unread, [s.sessionId]: true };
        // One notice, for the session that the operator last typed in.
        if (s.sessionId === this.lastInputId) {
            this.toast(`${this.sessionLabel(s)} answered`, "info", false, {
                label: "Open",
                run: () => void this.openSession(s.sessionId),
            });
        }
    }

    /** Clear the unread mark of the tab that is in view. */
    private markRead(): void {
        const id = this.activeId;
        // The operator looks at the tab: the mark "a message arrived here" is read too.
        if (id && this.msgMarks[id] && this.openPath && this.sessionState.byId[id]?.path === this.openPath) this.clearMsgMark(id);
        if (id && this.unread[id]) {
            const next = { ...this.unread };
            delete next[id];
            this.unread = next;
        }
    }

    /** True when the session finished a turn and its tab is not in view. */
    isUnread(sessionId: string): boolean {
        const s = this.sessionState.byId[sessionId];
        return !!this.unread[sessionId] && !!s && !this.inView(s);
    }

    /** The operator typed or pasted in the terminal of this session. */
    noteInput(sessionId: string): void {
        this.lastInputId = sessionId;
        // The operator answers a session: it is read.
        if (this.unread[sessionId]) {
            const next = { ...this.unread };
            delete next[sessionId];
            this.unread = next;
        }
    }

    /** "Eli (Engineer)", or "Setup agent". Works for a session of any floor that the UI opened. */
    sessionLabel(s: { path: string; slug: string }): string {
        if (s.slug === SETUP_SLUG) return "Setup agent";
        return employeeLabel(this.employeesByPath[s.path], s.slug);
    }

    /** The sessions in `question` on all floors that the UI knows, the oldest first. */
    questions = $derived.by((): Question[] =>
        questionOrder(this.questionSince, this.sessionState).map((s) => {
            const floor = this.floors.find((f) => f.path === s.path);
            return {
                sessionId: s.sessionId,
                path: s.path,
                slug: s.slug,
                label: this.sessionLabel(s),
                floorName: floor?.name ?? s.path.split("/").filter((p) => p !== "").pop() ?? s.path,
            };
        }),
    );

    /** The question that "Open" opens now: the one after the last opened one, or the oldest. */
    nextQuestion = $derived(nextAfter(this.questions, this.lastOpenedQuestion));

    windowTitle = $derived(windowTitleFor(this.questions.length));

    /**
     * Open a session that asks a question: open its floor when it is not the open one, select the
     * employee (this makes its tab the active tab), and put the keyboard focus in its terminal.
     */
    async openQuestion(sessionId: string): Promise<void> {
        if (!this.sessionState.byId[sessionId]) return;
        this.lastOpenedQuestion = sessionId;
        await this.openSession(sessionId);
    }

    /** Open the tab of a session, as "Open" of a question does, without a change of the question order. */
    async openSession(sessionId: string): Promise<void> {
        const s = this.sessionState.byId[sessionId];
        if (!s) return;
        if (s.path !== this.openPath && !(await this.openFloor(s.path))) return;
        this.selectedByPath = { ...this.selectedByPath, [s.path]: s.slug };
        this.selectTab(sessionId);
        await tick();
        // The pane shows after the tab change. A modal that is open keeps the focus.
        requestAnimationFrame(() => {
            if (!modalOpen()) focusTerminal(sessionId);
        });
    }

    /** Forget the text of a rejected `logo_generate` call: of one floor, or of all floors. */
    private clearLogoCallError(path?: string): void {
        if (path === undefined) {
            if (Object.keys(this.logoCallError).length > 0) this.logoCallError = {};
        } else if (path in this.logoCallError) {
            const next = { ...this.logoCallError };
            delete next[path];
            this.logoCallError = next;
        }
    }

    /** Change the open floor. The call error of the floor that the operator leaves goes away. */
    private switchTo(view: FloorView): void {
        if (this.openPath !== null && this.openPath !== view.path) this.clearLogoCallError(this.openPath);
        this.openPath = view.path;
        // The detail panel of the floor that the operator comes back to shows its task again.
        const open = this.openTaskByPath[view.path];
        if (this.taskDetail?.path !== view.path || this.taskDetail.detail.task.id !== open) {
            this.taskDetail = null;
            if (open !== undefined) void this.loadTaskDetail(view.path, open);
        }
    }

    /** Load the logo of each floor again when its version changed. */
    private syncLogos(): void {
        for (const entry of this.floors) {
            if (entry.missing) continue;
            const view = this.floorView && this.floorView.path === entry.path ? this.floorView : null;
            const want = view ? view.logoVersion : entry.logoVersion;
            if (this.logoLoaded.has(entry.path) && this.logoLoaded.get(entry.path) === want) continue;
            this.logoLoaded.set(entry.path, want);
            // The logo changed, so an old call error (for example `NO_KEY:`) is out of date.
            this.clearLogoCallError(entry.path);
            if (want === null) {
                if (entry.path in this.logos) {
                    const next = { ...this.logos };
                    delete next[entry.path];
                    this.logos = next;
                }
                continue;
            }
            void ipc
                .floorLogo(entry.path)
                .then((b64) => {
                    // A newer version can have been asked for while this call ran.
                    if (this.logoLoaded.get(entry.path) !== want) return;
                    const next = { ...this.logos };
                    if (b64) next[entry.path] = `data:image/png;base64,${b64}`;
                    else delete next[entry.path];
                    this.logos = next;
                })
                .catch(() => {
                    // Forget the version, so that the next change tries again.
                    if (this.logoLoaded.get(entry.path) === want) this.logoLoaded.delete(entry.path);
                });
        }
    }

    /** The operator changed the typed path: the old error under the field is out of date. */
    clearOpenError(): void {
        if (this.openError !== null) this.openError = null;
    }

    /**
     * Open a floor by path. Returns true on success. `source` is the field that the path was typed
     * in. A path that cannot be a floor (`NOT_ABSOLUTE:`, `NOT_FOUND:`, `NOT_A_FOLDER:`, `NO_WRITE:`)
     * shows its text under that field. With no `source` (a click on a floor) it shows as a toast.
     */
    async openFloor(path: string, source?: OpenSource): Promise<boolean> {
        const clean = path.trim();
        this.openError = null;
        if (clean === "") {
            this.toast("Enter a folder path first.");
            return false;
        }
        try {
            const view = await ipc.floorOpen(clean);
            this.pendingCreate = null;
            this.switchTo(view);
            this.applyView(view);
            this.syncSelection();
            writeLastFloor(view.path);
            void this.loadTasks(view.path);
            await this.loadFloors();
            return true;
        } catch (e) {
            const text = ipc.errorText(e);
            if (text.startsWith("NO_FLOOR:")) {
                this.pendingCreate = { path: clean, message: text };
            } else if (source && isPathError(text)) {
                // No "create" for these: the path is not a folder that can be a floor.
                this.pendingCreate = null;
                this.openError = { source, path: clean, text: tidyError(text) };
            } else {
                this.toast(text);
            }
            return false;
        }
    }

    async createFloor(path: string, name: string): Promise<boolean> {
        try {
            const view = await ipc.floorCreate(path, name.trim());
            this.pendingCreate = null;
            this.switchTo(view);
            this.applyView(view);
            this.syncSelection();
            writeLastFloor(view.path);
            void this.loadTasks(view.path);
            await this.loadFloors();
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    cancelCreate(): void {
        this.pendingCreate = null;
    }

    async forgetFloor(path: string): Promise<void> {
        try {
            await ipc.floorForget(path);
            if (this.openPath === path) {
                this.openPath = null;
                this.floorView = null;
            }
            await this.loadFloors();
        } catch (e) {
            this.fail(e);
        }
    }

    /** Read the open floor again. Returns the new view, or null when there is none or the call failed. */
    async reloadFloor(): Promise<FloorView | null> {
        const path = this.openPath;
        if (!path) return null;
        try {
            const view = await ipc.floorOpen(path);
            if (this.openPath !== path) return null;
            const wasGone = this.floorView?.path === path && this.floorView.gone;
            this.applyView(view);
            // The folder is back: the tasks that the UI holds can be old.
            if (wasGone && !view.gone) void this.loadTasks(path);
            return view;
        } catch (e) {
            this.fail(e);
            return null;
        }
    }

    /**
     * The button "Check again" of the banner of a gone floor. It reads the floor and the list again.
     * A floor that is still gone says so in a toast, so that the click has a visible answer.
     */
    async checkFloorAgain(): Promise<void> {
        const view = await this.reloadFloor();
        await this.loadFloors();
        if (view?.gone) this.toast(`The folder is still gone: ${view.path}.`, "info");
    }

    // ---- tasks (docs/ipc-m2.md) ----

    private taskLoads = new Map<string, number>();

    /** Load the tasks of a floor. An answer that is older than a newer call is dropped. */
    async loadTasks(path: string): Promise<void> {
        const call = (this.taskLoads.get(path) ?? 0) + 1;
        this.taskLoads.set(path, call);
        try {
            const tasks = await ipc.taskList(path);
            if (this.taskLoads.get(path) !== call) return;
            const names = (slug: string): string =>
                slug === "operator" ? "You" : ((this.employeesByPath[path] ?? []).find((e) => e.slug === slug)?.name ?? slug);
            this.addActivity(taskDrafts(path, this.tasksByPath[path], tasks, names));
            this.tasksByPath = { ...this.tasksByPath, [path]: tasks };
        } catch (e) {
            if (this.taskLoads.get(path) === call) this.fail(e);
            return;
        }
        // The panel that is open shows the newest events and report.
        const open = this.openTaskByPath[path];
        if (open !== undefined) await this.loadTaskDetail(path, open);
    }

    /** Load the detail of a task, and its report text when it has a report path. */
    async loadTaskDetail(path: string, id: number): Promise<void> {
        let detail: TaskDetail;
        try {
            detail = await ipc.taskGet(path, id);
        } catch (e) {
            // The task is gone, or the floor is: close the panel.
            this.closeTask();
            this.fail(e);
            return;
        }
        if (this.openTaskByPath[path] !== id) return;
        const keep = this.taskDetail && this.taskDetail.path === path && this.taskDetail.detail.task.id === id ? this.taskDetail : null;
        this.taskDetail = {
            path,
            detail,
            report: detail.task.report === null ? null : (keep?.report ?? undefined),
            reportError: keep?.reportError ?? null,
        };
        if (detail.task.report === null) return;
        try {
            const text = await ipc.taskReport(path, id);
            if (this.openTaskByPath[path] !== id) return;
            this.taskDetail = { path, detail, report: text, reportError: null };
        } catch (e) {
            if (this.openTaskByPath[path] !== id) return;
            this.taskDetail = { path, detail, report: null, reportError: ipc.errorText(e) };
        }
    }

    openTask(id: number): void {
        const path = this.openPath;
        if (!path) return;
        this.openTaskByPath = { ...this.openTaskByPath, [path]: id };
        this.taskDetail = null;
        this.taskDiff = null;
        this.reader = null;
        void this.loadTaskDetail(path, id);
        // A toast of a blocked merge of this task is old news now: the detail shows the reason.
        for (const t of this.toasts) if (t.task?.path === path && t.task.id === id) this.closeTaskToastLater(t.id);
    }

    closeTask(): void {
        const path = this.openPath;
        if (!path) return;
        const next = { ...this.openTaskByPath };
        delete next[path];
        this.openTaskByPath = next;
        this.taskDetail = null;
        this.taskDiff = null;
        this.reader = null;
    }

    /** Show the diff box of a task. The box is empty ("loading") until the command answers. */
    async showDiff(id: number): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        this.taskDiff = { path, id, diff: null, error: null };
        try {
            const diff = await ipc.taskDiff(path, id);
            if (this.taskDiff?.path === path && this.taskDiff.id === id) this.taskDiff = { path, id, diff, error: null };
        } catch (e) {
            if (this.taskDiff?.path === path && this.taskDiff.id === id) {
                this.taskDiff = { path, id, diff: null, error: ipc.errorText(e) };
            }
        }
    }

    hideDiff(): void {
        this.taskDiff = null;
        if (this.reader === "diff") this.reader = null;
    }

    /** Open the report or the diff of the open task in the large reader overlay. */
    async openReader(kind: "report" | "diff", id: number): Promise<void> {
        this.reader = kind;
        if (kind === "diff") await this.showDiff(id);
    }

    closeReader(): void {
        if (this.reader === "diff") this.taskDiff = null;
        this.reader = null;
    }

    /**
     * The operator merges the branch of a task. A merge that is blocked comes as the `notice`
     * event (a toast), so this adds a toast only for a merge that worked. Returns the result,
     * or null when the command was refused (the error shows in a toast).
     */
    async mergeTask(id: number): Promise<TaskMerge | null> {
        const path = this.openPath;
        if (!path) return null;
        try {
            const result = await ipc.taskMerge(path, id);
            this.taskDiff = null;
            this.reader = null;
            if (result.merged) this.toast(result.message, "info");
            await this.loadTasks(path);
            return result;
        } catch (e) {
            this.fail(e);
            return null;
        }
    }

    /** The operator discards a task: its worktree and branch go, and its status is `blocked`. */
    async discardTask(id: number): Promise<boolean> {
        const path = this.openPath;
        if (!path) return false;
        try {
            await ipc.taskDiscard(path, id);
            this.taskDiff = null;
            this.reader = null;
            this.toast(`${taskLabel(id)} is discarded.`, "info");
            await this.loadTasks(path);
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    /** The operator makes a task. Returns the new task, or `null` when the command failed. */
    async createTask(title: string, description: string): Promise<TaskView | null> {
        const path = this.openPath;
        if (!path) return null;
        try {
            const task = await ipc.taskCreate(path, title, description);
            await this.loadTasks(path);
            return task;
        } catch (e) {
            this.fail(e);
            return null;
        }
    }

    async assignTask(id: number, slug: string): Promise<boolean> {
        const path = this.openPath;
        if (!path) return false;
        try {
            await ipc.taskAssign(path, id, slug);
            await this.loadTasks(path);
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    async updateTask(id: number, status: TaskStatus, note: string): Promise<boolean> {
        const path = this.openPath;
        if (!path) return false;
        try {
            await ipc.taskUpdate(path, id, status, note.trim() === "" ? null : note);
            await this.loadTasks(path);
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    // ---- the restore of the last team (docs/ipc-m3.md) ----

    /** Resume the employees of the last team, one by one, in the background. */
    async resumeTeam(): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            const progress = await ipc.teamResume(path);
            // The button shows the progress at once. `floor-changed` follows after each start.
            if (this.floorView && this.floorView.path === path) {
                this.floorView = { ...this.floorView, resumable: [], resumeProgress: progress };
            }
        } catch (e) {
            this.fail(e);
        }
    }

    // ---- commands with no question (docs/ipc-m3.md) ----

    /** The rules of `[permissions] allow` of the open floor. */
    permissions = $derived(this.openPath ? (this.permissionsByPath[this.openPath] ?? null) : null);

    /** The warnings of the last save for the open floor. */
    permissionWarnings = $derived(this.openPath ? (this.permissionWarningsByPath[this.openPath] ?? []) : []);

    /** The rules that an old `[permissions]` table of the project offers for the open floor. Empty in normal use. */
    proposedRules = $derived(this.openPath ? (this.proposedByPath[this.openPath] ?? []) : []);

    /** Read the rules of the open floor, and the rules that the project proposes. */
    async loadPermissions(): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            const allow = await ipc.floorPermissionsGet(path);
            this.permissionsByPath = { ...this.permissionsByPath, [path]: allow };
        } catch (e) {
            this.fail(e);
            return;
        }
        try {
            const proposed = await ipc.floorPermissionsProposed(path);
            this.proposedByPath = { ...this.proposedByPath, [path]: proposed };
        } catch (e) {
            this.fail(e);
        }
    }

    /**
     * Write the rules of the open floor. Returns null on success, or the refusal text (it is
     * for the form, not a toast). The warnings of the answer show next to the rules. The rules are
     * saved even when there is a warning. After a save the app config has an entry for the floor, so
     * the proposal of the project is gone.
     */
    async savePermissions(allow: string[]): Promise<string | null> {
        const path = this.openPath;
        if (!path) return "No floor is open.";
        try {
            const saved = await ipc.floorPermissionsSet(path, allow);
            this.permissionsByPath = { ...this.permissionsByPath, [path]: saved.allow };
            this.permissionWarningsByPath = { ...this.permissionWarningsByPath, [path]: saved.warnings };
            if ((this.proposedByPath[path] ?? []).length > 0) this.proposedByPath = { ...this.proposedByPath, [path]: [] };
            return null;
        } catch (e) {
            return ipc.errorText(e);
        }
    }

    // ---- logo and setup ----

    /** Start a logo generation. `prompt` null keeps the stored prompt. */
    async generateLogo(prompt: string | null): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        this.clearLogoCallError(path);
        try {
            await ipc.logoGenerate(path, prompt);
        } catch (e) {
            // `NO_KEY:` and `BUSY:` can come as the error of the call. They show like a `logoError`.
            this.logoCallError = { ...this.logoCallError, [path]: ipc.errorText(e) };
        }
        void this.reloadFloor();
    }

    /** Save the OpenAI key, then try the logo again. Returns true when the key was saved. */
    async saveKeyAndRetry(key: string): Promise<boolean> {
        try {
            this.settings = await ipc.settingsSet(key.trim(), null);
        } catch (e) {
            this.fail(e);
            return false;
        }
        // The key is saved: a `NO_KEY:` error of any floor is out of date.
        this.clearLogoCallError();
        await this.generateLogo(null);
        return true;
    }

    /** Start the setup agent of the open floor. */
    startSetup(cli: CliKind): Promise<void> {
        return this.launch(SETUP_SLUG, "setup", { cli, name: "Setup" }, (path, rows, cols) =>
            ipc.setupStart(path, cli, rows, cols),
        );
    }

    // ---- employees ----

    async saveEmployee(employee: Employee): Promise<boolean> {
        this.setActionError("employee", null);
        const path = this.openPath;
        if (!path) return false;
        try {
            this.applyView(await ipc.employeeSave(path, employee));
            return true;
        } catch (e) {
            this.failIn("employee", e);
            return false;
        }
    }

    async removeEmployee(slug: string): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            this.applyView(await ipc.employeeRemove(path, slug));
        } catch (e) {
            this.fail(e);
        }
    }

    // ---- sessions and tabs ----

    /** Select an employee of the open floor. A live session of the employee gets its tab. */
    selectEmployee(slug: string): void {
        const path = this.openPath;
        if (!path) return;
        this.selectedByPath = { ...this.selectedByPath, [path]: slug };
        const session = this.sessionOfEmployee(slug);
        if (session && isLive(session)) this.selectTab(session.sessionId);
    }

    sessionOfEmployee(slug: string): TrackedSession | undefined {
        return this.openPath ? sessionOf(this.sessionState, this.openPath, slug) : undefined;
    }

    questionsIn(path: string): number {
        return questionCount(this.sessionState, path);
    }

    /** The number of live sessions of a floor (every status but `stopped`). */
    liveIn(path: string): number {
        return Object.values(this.sessionState.byId).filter((s) => s.path === path && isLive(s)).length;
    }

    /** Make a tab the active tab. The employee of the tab is the selected employee: the bar and the tab agree. */
    selectTab(sessionId: string): void {
        const path = this.openPath;
        if (!path) return;
        this.activeByPath = { ...this.activeByPath, [path]: sessionId };
        const session = this.sessionState.byId[sessionId];
        if (session && session.path === path) this.selectedByPath = { ...this.selectedByPath, [path]: session.slug };
        this.markRead();
    }

    /** The selected employee follows the active tab of the open floor. */
    private syncSelection(): void {
        const path = this.openPath;
        const id = this.activeId;
        const session = id ? this.sessionState.byId[id] : undefined;
        if (path && session && session.path === path) this.selectedByPath = { ...this.selectedByPath, [path]: session.slug };
        this.markRead();
    }

    closeTab(sessionId: string): void {
        this.commitSessions(dismissTab(this.sessionState, sessionId));
        // The tab that is active now (a closed tab never was) is the employee in the bar.
        this.syncSelection();
    }

    private adopt(path: string, session: SessionView): void {
        this.commitSessions(upsertSession(this.sessionState, path, session), true);
        // Show the new tab only when the operator still looks at the same floor.
        if (this.openPath === path) this.selectTab(session.sessionId);
    }

    /** True while a start or a resume of this employee is running. The card disables its buttons. */
    isBusy(slug: string): boolean {
        return this.busyKind(slug) !== null;
    }

    /** Which button was clicked for this employee, while its command runs. `null` when none runs. */
    busyKind(slug: string): LaunchKind | null {
        return this.busy[`${this.openPath}\u0000${slug}`] ?? null;
    }

    /**
     * Run a start or a resume. On failure the error shows in a toast, and nothing else
     * changes: no tab is made, and the card goes back to its normal state.
     * `info` is the CLI and the name for the loading text of the new session.
     */
    private async launch(
        slug: string,
        kind: LaunchKind,
        info: { cli: CliKind; name: string },
        run: (path: string, rows: number, cols: number) => Promise<SessionView>,
    ): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        const key = `${path}\u0000${slug}`;
        if (this.busy[key]) return;
        this.busy = { ...this.busy, [key]: kind };
        try {
            const { rows, cols } = this.lastSize;
            const session = await run(path, rows, cols);
            this.launched = { ...this.launched, [key]: info };
            this.adopt(path, session);
        } catch (e) {
            this.fail(e);
        } finally {
            const next = { ...this.busy };
            delete next[key];
            this.busy = next;
        }
    }

    /** The CLI and the name that the loading text of a session uses. A null CLI means "not known". */
    describeSession(session: { path: string; slug: string }): { cli: CliKind | null; name: string } {
        const known = this.launched[`${session.path}\u0000${session.slug}`];
        if (session.slug === SETUP_SLUG) return { cli: known?.cli ?? null, name: "Setup" };
        const emp = session.path === this.openPath ? this.orderedEmployees.find((e) => e.slug === session.slug) : undefined;
        return { cli: emp?.cli ?? known?.cli ?? null, name: emp?.name ?? known?.name ?? session.slug };
    }

    private employeeInfo(slug: string): { cli: CliKind; name: string } {
        const emp = this.orderedEmployees.find((e) => e.slug === slug);
        return { cli: emp?.cli ?? "claude", name: emp?.name ?? slug };
    }

    startSession(slug: string): Promise<void> {
        return this.launch(slug, "start", this.employeeInfo(slug), (path, rows, cols) =>
            ipc.sessionStart(path, slug, null, rows, cols),
        );
    }

    resumeSession(slug: string): Promise<void> {
        return this.launch(slug, "resume", this.employeeInfo(slug), (path, rows, cols) =>
            ipc.sessionResume(path, slug, null, rows, cols),
        );
    }

    async stopSession(sessionId: string): Promise<void> {
        try {
            await ipc.sessionStop(sessionId);
        } catch (e) {
            this.fail(e);
        }
    }

    async clearDraft(sessionId: string): Promise<void> {
        try {
            await ipc.clearDraft(sessionId);
        } catch (e) {
            this.fail(e);
        }
    }

    /** Returns the refusal text, or null when the paste went through. */
    async debugPaste(sessionId: string, text: string): Promise<string | null> {
        try {
            await ipc.debugPaste(sessionId, text);
            return null;
        } catch (e) {
            return ipc.errorText(e);
        }
    }

    isLive(session: SessionView | undefined): boolean {
        return !!session && isLive(session);
    }

    reportSize(rows: number, cols: number): void {
        if (rows > 0 && cols > 0) this.lastSize = { rows, cols };
    }

    // ---- programs ----

    async loadPrograms(): Promise<void> {
        try {
            this.programs = await ipc.programsGet();
        } catch (e) {
            this.fail(e);
        }
    }

    async loadSettings(): Promise<void> {
        try {
            this.settings = await ipc.settingsGet();
        } catch (e) {
            this.fail(e);
        }
    }

    // ---- settings panel, model lists, floor defaults (docs/ipc-settings.md) ----

    /** Read the lists again. The Rust side reads the Codex file on each call. */
    async loadModels(): Promise<void> {
        try {
            this.models = await ipc.modelsGet();
        } catch (e) {
            this.fail(e);
        }
    }

    /** Replace the custom models of one kind. Returns true on success. */
    async setCustomModels(kind: ModelKind, names: string[]): Promise<boolean> {
        this.setActionError(`models:${kind}`, null);
        try {
            this.models = await ipc.modelsSetCustom(kind, names);
            return true;
        } catch (e) {
            this.failIn(`models:${kind}`, e);
            return false;
        }
    }

    /** Store the image model of the logo. Returns true on success. */
    async setImageModel(model: string): Promise<boolean> {
        this.setActionError("image", null);
        try {
            this.settings = await ipc.settingsSet(null, model);
            return true;
        } catch (e) {
            this.failIn("image", e);
            return false;
        }
    }

    /** Store the OpenAI key in the app config. An empty text removes it. Returns true on success. */
    async setOpenaiKey(key: string): Promise<boolean> {
        this.setActionError("keys", null);
        try {
            this.settings = await ipc.settingsSet(key.trim(), null);
            // A stored key makes an old `NO_KEY:` error out of date.
            if (key.trim() !== "") this.clearLogoCallError();
            return true;
        } catch (e) {
            this.failIn("keys", e);
            return false;
        }
    }

    /** Write the floor defaults of the open floor. Returns true on success. */
    async setFloorDefaults(defaults: FloorDefaults): Promise<boolean> {
        this.setActionError("defaults", null);
        const path = this.openPath;
        if (!path) return false;
        try {
            this.applyView(await ipc.floorDefaultsSet(path, defaults));
            return true;
        } catch (e) {
            this.failIn("defaults", e);
            return false;
        }
    }

    async setProgram(name: ProgramName, path: string): Promise<boolean> {
        this.setActionError(`program:${name}`, null);
        try {
            this.programs = await ipc.programsSet(name, path.trim());
            this.programTests = { ...this.programTests, [name]: null };
            return true;
        } catch (e) {
            this.failIn(`program:${name}`, e);
            return false;
        }
    }

    /** Remove the override of a program. The app then searches again, and `programs` shows what it finds. */
    async resetProgram(name: ProgramName): Promise<boolean> {
        return this.setProgram(name, "");
    }

    /** Run `<path> --version` for a program. The result shows next to the program. */
    async testProgram(name: ProgramName): Promise<void> {
        if (this.programTesting[name]) return;
        this.programTesting = { ...this.programTesting, [name]: true };
        try {
            const result = await ipc.programTest(name);
            this.programTests = { ...this.programTests, [name]: result };
        } catch (e) {
            // A refusal of the command itself (not a failed program). Show it in the same place.
            this.programTests = { ...this.programTests, [name]: { ok: false, path: null, output: ipc.errorText(e) } };
        } finally {
            this.programTesting = { ...this.programTesting, [name]: false };
        }
    }

    /** Test the Codex sandbox (`bwrap`). The result shows in the section "Codex sandbox" of the settings. */
    async testSandbox(clearError = true): Promise<void> {
        if (this.sandboxTesting) return;
        this.sandboxTesting = true;
        if (clearError) this.setActionError("codex-sandbox", null);
        try {
            try {
                this.sandboxProbe = await ipc.codexSandboxProbe();
            } catch (e) {
                // A refusal of the command itself (not a failed bwrap). Show it as a failed test.
                this.sandboxProbe = { state: "fails", detail: ipc.errorText(e) };
            }
            // The effective state (`settings.codexSandboxEffective`) comes from the last probe: read it again, before the
            // test counts as ended, so the line "In effect now" and the result of the test change together.
            await this.loadSettings();
        } finally {
            this.sandboxTesting = false;
        }
    }

    /**
     * The setting "Codex sandbox" with its three states (Automatic, Always, Never). The app runs the probe again for
     * "Always", and it refuses "Always" unless the probe says `works`. Returns true on success.
     */
    async setCodexSandboxMode(mode: CodexSandboxMode): Promise<boolean> {
        this.setActionError("codex-sandbox", null);
        try {
            this.settings = await ipc.settingsSet(null, null, null, null, mode);
            return true;
        } catch (e) {
            this.failIn("codex-sandbox", e);
            // The refusal tells that the probe did not pass: read the state again, so the section shows it.
            void this.testSandbox(false);
            return false;
        }
    }

    /** The switch "Desktop notification when an agent asks a question". Returns true on success. */
    async setNotifyOnQuestion(on: boolean): Promise<boolean> {
        this.setActionError("interface", null);
        try {
            this.settings = await ipc.settingsSet(null, null, on);
            return true;
        } catch (e) {
            this.failIn("interface", e);
            return false;
        }
    }
}

export const store = new Store();
