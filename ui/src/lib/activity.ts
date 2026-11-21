// The "Team activity" list: the newest changes of a floor, built only from data that the UI already
// has. Pure logic: no Svelte, no Tauri.
//   - a session changed its status (`session-changed`, `floor_open`),
//   - an employee started another employee (`employee-started`),
//   - a task changed (a `tasks-changed` reload compared with the tasks before it),
//   - a message between employees was delivered (`message-delivered`),
//   - the app stopped an exchange between two employees (the `notice` of the kind `exchange_stopped`).
// An employee has one current status row: see the rule at `appendActivity`.
import type { SessionState } from "./reducers";
import { statusLabel, taskLabel } from "./tasks";
import type { MessageDeliveredPayload, Status, TaskView, TrackedSession } from "./types";

export type ActivityKind = "session" | "question" | "started" | "task" | "message" | "exchange";

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
    /** A message line: the sender. `slug` is the receiver. */
    fromSlug?: string;
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
 * The status as it reads after "is now": "in review", "in progress", "done", "blocked", "to do". The label of the
 * board is "Review", and "T-2 is now review" reads wrong.
 */
export function statusPhrase(status: string): string {
    return status === "review" ? "in review" : statusLabel(status).toLowerCase();
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
        if (was.status !== t.status) out.push(line(`${taskLabel(t.id)} is now ${statusPhrase(t.status)}`));
    }
    return out;
}

/**
 * The line for a message that the app delivered. The text is "Maya → Eli: <preview>". For a message about a
 * task it is "Maya → Eli: task T-3 assigned" (an assignment) or "Maya → Eli: T-3 is now in review" (a status
 * message). A click goes to the tab of the receiver.
 */
export function messageDraft(p: MessageDeliveredPayload): ActivityDraft {
    // The preview has 80 characters at most. A text of that length is probably cut: the row ends with "…".
    let what = p.preview.length >= PREVIEW_CHARS ? `${p.preview}\u2026` : p.preview;
    if (p.kind === "task" && p.taskId !== null) {
        if (p.taskStatus === null || p.taskStatus === undefined) what = `task ${taskLabel(p.taskId)} assigned`;
        else if (p.taskStatus === "") what = `${taskLabel(p.taskId)} changed`;
        else what = `${taskLabel(p.taskId)} is now ${statusPhrase(p.taskStatus)}`;
    }
    return {
        path: p.path,
        kind: "message",
        text: `${p.fromName} → ${p.toName}: ${what}`,
        slug: p.toSlug,
        fromSlug: p.fromSlug,
    };
}

/**
 * The line for the notice `exchange_stopped`: the text of the notice. A click goes to the tab of the employee that
 * the app refused (`slug`).
 */
export function exchangeDraft(path: string, text: string, slug: string | undefined): ActivityDraft {
    return { path, kind: "exchange", text, slug };
}

/** The most characters of the preview of a delivered message (docs/ipc-final.md). */
export const PREVIEW_CHARS = 80;

/** A status row of an employee replaces its own older status row only when that row is younger than this. */
export const FOLD_MS = 5 * 60 * 1000;

/**
 * Add lines at the end of the list (the newest). The rule for the status rows (kind `session`: "is working",
 * "answered", "is idle", "is done", "stopped"). The aim is ONE current status row for each employee.
 *
 *  1. A new status row of an employee replaces that employee's previous status row, when that row is younger than
 *     `FOLD_MS`. Rows of other employees do not matter.
 *  2. A question row or a start row of the same employee ("Maya asks a question", "Ada started Maya") lies between
 *     two status rows of that employee. They tell a story ("asks, then works, then answers"), so the old status row
 *     stays and no fold happens.
 *  3. A message row never stops the fold, not a message to the employee and not a message from it. A message is an
 *     event of its own. The real app delivers it after the status row of the sender ("Maya answered", then "Maya →
 *     Eli: ..."). If a message stopped the fold, the list would show "Maya answered" twice, with the messages
 *     between them. (The first version of this rule did that. The live run "dc-activity" showed it.)
 *  4. Each employee that took part keeps its newest status row. A status row is never dropped because a message
 *     row is near it. (An earlier version dropped the "answered" row after a message of the same employee. Then an
 *     employee had no status row at all: the live run "final-b6" showed it.)
 *
 * Rows for questions, starts, tasks and messages are never removed by this function. `nextId` gives the ID of a
 * new line.
 */
export function appendActivity(
    list: readonly ActivityEntry[],
    drafts: readonly ActivityDraft[],
    at: number,
    nextId: () => number,
): ActivityEntry[] {
    const out = [...list];
    for (const draft of drafts) {
        let add = true;
        if (draft.kind === "session" && draft.slug !== undefined) {
            const slug = draft.slug;
            // The previous status row of the employee. Rules 1 to 3.
            let previous = -1;
            for (let i = out.length - 1; i >= 0; i--) {
                const row = out[i];
                if (row.path !== draft.path) continue;
                if (row.kind === "session" && row.slug === slug) {
                    previous = i;
                    break;
                }
                if ((row.kind === "question" || row.kind === "started") && row.slug === slug) break;
            }
            if (previous >= 0 && at - out[previous].at < FOLD_MS) out.splice(previous, 1);
        }
        if (add) out.push({ ...draft, id: nextId(), at });
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
