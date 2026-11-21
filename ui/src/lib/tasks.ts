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
