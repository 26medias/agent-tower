// Pure logic for the task board (docs/ipc-m2.md). No Svelte, no Tauri.
import type { TaskEvent, TaskStatus, TaskView } from "./types";

/** The groups of the board, from the work that needs attention first to the work that is finished. */
export const STATUS_ORDER: TaskStatus[] = ["in_progress", "review", "blocked", "todo", "done"];

/** The status words for a select, in the order of the work flow. */
export const STATUS_CHOICES: TaskStatus[] = ["todo", "in_progress", "blocked", "review", "done"];

const LABELS: Record<TaskStatus, string> = {
    todo: "To do",
    in_progress: "In progress",
    blocked: "Blocked",
    review: "Review",
    done: "Done",
};

/** The same values are in src/app.css (--tk-*). */
export const TASK_COLOR: Record<TaskStatus, string> = {
    todo: "#93a2c2",
    in_progress: "#5aa9f2",
    blocked: "#ff7a90",
    review: "#ffc56b",
    done: "#6fd08c",
};

/** The task has a git branch (the app made a worktree for it). Docs/ipc-m3.md. */
export function hasBranch(task: TaskView): boolean {
    return task.branch !== null;
}

/** `a1b2c3d4e5f6` for a commit ID: the first 12 characters. */
export function shortCommit(commit: string | null): string {
    return commit === null ? "" : commit.slice(0, 12);
}

/** The merge button: a task with a branch, in review or blocked. The app refuses any other status. */
export function canMerge(task: TaskView): boolean {
    return hasBranch(task) && (task.status === "review" || task.status === "blocked");
}

/** The discard button: a task that is not done and has a branch or a worktree to remove. */
export function canDiscard(task: TaskView): boolean {
    return task.status !== "done" && (task.branch !== null || task.worktree !== null);
}

/** The absolute worktree folder of a task, or null when there is none now. */
export function worktreePath(floorPath: string, task: TaskView): string | null {
    if (task.worktree === null) return null;
    return `${floorPath.replace(/\/+$/, "")}/${task.worktree}`;
}

/** What the diff box shows when the task has no branch, or the branch is gone, or there is no change. */
export function diffPlaceholder(diff: { branch: string | null; stat: string; patch: string; note: string | null }): string | null {
    if (diff.note !== null && diff.patch === "" && diff.stat === "") return diff.note;
    if (diff.stat.trim() === "" && diff.patch.trim() === "") return "There is no change on the branch yet.";
    return null;
}

/** The text of the diff box: the diffstat, a blank line, the patch, and the note. */
export function diffText(diff: { stat: string; patch: string; note: string | null }): string {
    const parts = [diff.stat.trimEnd(), diff.patch.trimEnd()].filter((p) => p !== "");
    if (diff.note !== null) parts.push(`[${diff.note}]`);
    return parts.join("\n\n");
}

export function statusLabel(status: string): string {
    return (LABELS as Record<string, string>)[status] ?? status;
}

/** `12` is shown as `T-12`. */
export function taskLabel(id: number): string {
    return `T-${id}`;
}

export interface TaskGroup {
    status: TaskStatus;
    tasks: TaskView[];
}

/** The groups in `STATUS_ORDER`. A group with no task is left out. Inside a group the oldest task is first. */
export function groupTasks(tasks: TaskView[]): TaskGroup[] {
    return STATUS_ORDER.map((status) => ({
        status,
        tasks: tasks.filter((t) => t.status === status).sort((a, b) => a.id - b.id),
    })).filter((g) => g.tasks.length > 0);
}

/** The tasks that are not `done`: the number on the collapsed strip. */
export function openCount(tasks: TaskView[]): number {
    return tasks.filter((t) => t.status !== "done").length;
}

/** The groups that show their cards at the start. `done` is collapsed. */
export function defaultOpenGroups(): Record<TaskStatus, boolean> {
    return { in_progress: true, review: true, blocked: true, todo: true, done: false };
}

/** The name of a slug for the board: the employee name, `Operator`, or the slug of a removed employee. */
export function personName(slug: string | null, names: Record<string, string>): string {
    if (slug === null) return "Unassigned";
    if (slug === "operator") return "Operator";
    return names[slug] ?? slug;
}

/** `2026-11-21 14:05:09` in local time. */
export function formatTime(ms: number): string {
    const d = new Date(ms);
    const two = (n: number) => String(n).padStart(2, "0");
    return (
        `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ` +
        `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`
    );
}

/** One line for an event of the detail panel, and its note (the comment or the note of a status change). */
export function eventText(event: TaskEvent, names: Record<string, string>): { line: string; note: string | null } {
    const who = personName(event.author, names);
    switch (event.kind) {
        case "created":
            return { line: `${who} made the task`, note: null };
        case "assigned":
            return { line: `${who} gave the task to ${personName(event.value, names)}`, note: null };
        case "status":
            return { line: `${who} set the status to ${statusLabel(event.value ?? "")}`, note: event.text };
        case "report":
            return { line: `${who} set the report ${event.value ?? ""}`, note: null };
        case "worktree":
            return { line: `The app made the branch ${event.value ?? ""}`, note: event.text };
        case "comment":
            return { line: `${who} wrote a note`, note: event.text };
        default:
            return { line: `${who}: ${event.kind}`, note: event.text };
    }
}

/** Put a task in a list (a new one at the end, an old one in its place). The list stays in order of ID. */
export function upsertTask(list: TaskView[], task: TaskView): TaskView[] {
    const next = list.some((t) => t.id === task.id) ? list.map((t) => (t.id === task.id ? task : t)) : [...list, task];
    return next.sort((a, b) => a.id - b.id);
}

// ---- the filter and the done group of the board ----

/** What the board shows: all tasks, or the tasks of one assignee, or the tasks that match a text. */
export interface TaskFilter {
    /** "" = everyone. `UNASSIGNED` = tasks with no assignee. Else an employee slug. */
    assignee: string;
    /** Matches the title, the description, the label (`T-12`), the assignee name and the status words. Ignores case. */
    query: string;
}

export const UNASSIGNED = "(unassigned)";

export const NO_FILTER: TaskFilter = { assignee: "", query: "" };

export function isFiltering(filter: TaskFilter): boolean {
    return filter.assignee !== "" || filter.query.trim() !== "";
}

/** The tasks that pass the filter. A blank query matches all. Every word of the query must match. */
export function filterTasks(tasks: TaskView[], filter: TaskFilter, names: Record<string, string> = {}): TaskView[] {
    const words = filter.query.toLowerCase().split(/\s+/).filter((w) => w !== "");
    return tasks.filter((t) => {
        if (filter.assignee === UNASSIGNED) {
            if (t.assignee !== null) return false;
        } else if (filter.assignee !== "" && t.assignee !== filter.assignee) {
            return false;
        }
        if (words.length === 0) return true;
        const hay = [taskLabel(t.id), t.title, t.description, personName(t.assignee, names), statusLabel(t.status)]
            .join("\n")
            .toLowerCase();
        return words.every((w) => hay.includes(w));
    });
}

/** The board keeps this many of the newest done tasks in view. The rest of the group folds. */
export const DONE_KEEP = 3;

/**
 * Split the done tasks: the newest `keep` (by the time of the last change, then by ID), newest
 * first, and the older rest (newest first too).
 */
export function splitDone(tasks: TaskView[], keep = DONE_KEEP): { recent: TaskView[]; older: TaskView[] } {
    const sorted = [...tasks].sort((a, b) => b.updatedAt - a.updatedAt || b.id - a.id);
    return { recent: sorted.slice(0, keep), older: sorted.slice(keep) };
}
