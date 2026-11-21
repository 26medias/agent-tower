// The "Team activity" list: the newest changes of a floor, built only from data that the UI already
// has. Pure logic: no Svelte, no Tauri.
//   - a session changed its status (`session-changed`, `floor_open`),
//   - an employee started another employee (`employee-started`),
//   - a task changed (a `tasks-changed` reload compared with the tasks before it).
import type { SessionState } from "./reducers";
import { statusLabel, taskLabel } from "./tasks";
import type { Status, TaskView, TrackedSession } from "./types";

export type ActivityKind = "session" | "question" | "started" | "task";

export interface ActivityEntry {
    id: number;
    /** Unix ms. */
    at: number;
    /** The floor that the line belongs to. */
    path: string;
    kind: ActivityKind;
    /** The line as text. For a session line the text is only a fallback: the page builds it again from `slug` and `words`. */
    text: string;
    /** A click goes to the tab of this session (or the employee). */
    sessionId?: string;
    slug?: string;
    /** A session line: what happened ("asks a question"). The page puts the name of the employee before it. */
    words?: string;
    /** A click goes to this task. */
    taskId?: number;
}

/** A line before the store gives it an ID and a time. */
export type ActivityDraft = Omit<ActivityEntry, "id" | "at">;

/** The list shows this many lines at most, the newest first. */
export const ACTIVITY_SHOWN = 30;

/** The store keeps this many lines for all floors. The oldest go first. */
export const ACTIVITY_KEPT = 200;

/** The words for a status that a session reached. `null`: no line (the status is not news). */
function sessionWords(from: Status | null, to: Status): string | null {
    switch (to) {
        case "starting":
            return "started";
        case "working":
            return "is working";
        case "question":
            return "asks a question";
        case "idle":
            return from === "working" ? "answered" : "is idle";
        case "done":
            return "is done";
        case "stopped":
            return "stopped";
    }
}

/**
 * The lines for the sessions whose status changed between two states. A session that is new is
 * news only when `announceNew` is true (a session that a start or an event made): the sessions that
 * a floor brings when it opens are not news.
 */
export function sessionDrafts(
    prev: SessionState,
    next: SessionState,
    label: (s: TrackedSession) => string,
    announceNew: boolean,
): ActivityDraft[] {
    const out: ActivityDraft[] = [];
    for (const s of Object.values(next.byId)) {
        const before = prev.byId[s.sessionId];
        if (!before && !announceNew) continue;
        if (before && before.status === s.status) continue;
        const words = sessionWords(before?.status ?? null, s.status);
        if (words === null) continue;
        out.push({
            path: s.path,
            kind: s.status === "question" ? "question" : "session",
            text: `${label(s)} ${words}`,
            sessionId: s.sessionId,
            slug: s.slug,
            words,
        });
    }
    return out;
}

/** "Tomas started Kenji", for an `employee-started` event. */
export function startedDraft(path: string, byName: string, name: string, sessionId: string, slug: string): ActivityDraft {
    return { path, kind: "started", text: `${byName} started ${name}`, sessionId, slug };
}

/**
 * The lines for the tasks that changed between two loads of a floor: a new task, a new assignee and
 * a new status. `before` is `undefined` for the first load of a floor: that load is not news.
 * `who` turns an employee slug (or "operator") into a name.
 */
export function taskDrafts(
    path: string,
    before: TaskView[] | undefined,
    after: TaskView[],
    who: (slug: string) => string,
): ActivityDraft[] {
    if (before === undefined) return [];
    const old = new Map(before.map((t) => [t.id, t]));
    const out: ActivityDraft[] = [];
    for (const t of after) {
        const was = old.get(t.id);
        const line = (text: string): ActivityDraft => ({ path, kind: "task", text, taskId: t.id });
        if (!was) {
            out.push(line(`${who(t.creator)} made ${taskLabel(t.id)}: ${t.title}`));
            continue;
        }
        if (was.assignee !== t.assignee) {
            out.push(line(t.assignee ? `${taskLabel(t.id)} goes to ${who(t.assignee)}` : `${taskLabel(t.id)} has no owner now`));
        }
        if (was.status !== t.status) out.push(line(`${taskLabel(t.id)} is now ${statusLabel(t.status).toLowerCase()}`));
    }
    return out;
}

/**
 * The text of a line. A session line names the employee when the line is drawn, not when it was made:
 * the employees of a floor can come later than its first session event.
 */
export function lineText(line: ActivityEntry, label: (path: string, slug: string) => string): string {
    return line.words !== undefined && line.slug !== undefined ? `${label(line.path, line.slug)} ${line.words}` : line.text;
}

/** The lines of one floor, newest first, at most `ACTIVITY_SHOWN`. */
export function visibleActivity(all: readonly ActivityEntry[], path: string | null): ActivityEntry[] {
    if (path === null) return [];
    const mine = all.filter((e) => e.path === path);
    return mine.slice(-ACTIVITY_SHOWN).reverse();
}

/** The time of a line: "14:05". */
export function activityTime(at: number): string {
    const d = new Date(at);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
