import { describe, expect, it } from "vitest";
import {
    STATUS_CHOICES,
    STATUS_ORDER,
    TASK_COLOR,
    defaultOpenGroups,
    eventText,
    formatTime,
    groupTasks,
    openCount,
    personName,
    statusLabel,
    taskLabel,
    upsertTask,
} from "./tasks";
import type { TaskEvent, TaskStatus, TaskView } from "./types";

function task(id: number, status: TaskStatus, extra: Partial<TaskView> = {}): TaskView {
    return {
        id,
        title: `Task ${id}`,
        description: "",
        parent: null,
        creator: "operator",
        assignee: null,
        assigner: null,
        status,
        report: null,
        createdAt: 1000 * id,
        updatedAt: 1000 * id,
        ...extra,
    };
}

function event(kind: TaskEvent["kind"], extra: Partial<TaskEvent> = {}): TaskEvent {
    return { id: 1, at: 0, author: "eng", kind, value: null, text: null, ...extra };
}

describe("taskLabel and statusLabel", () => {
    it("shows the number as T-<n>", () => {
        expect(taskLabel(12)).toBe("T-12");
    });

    it("has a readable label for each status and keeps an unknown word", () => {
        expect(statusLabel("in_progress")).toBe("In progress");
        expect(statusLabel("todo")).toBe("To do");
        expect(statusLabel("mystery")).toBe("mystery");
    });

    it("has one colour and one place in the select for each status", () => {
        expect(Object.keys(TASK_COLOR).sort()).toEqual([...STATUS_CHOICES].sort());
        expect([...STATUS_ORDER].sort()).toEqual([...STATUS_CHOICES].sort());
    });
});

describe("groupTasks", () => {
    const tasks = [
        task(1, "done"),
        task(2, "todo"),
        task(3, "in_progress"),
        task(4, "review"),
        task(5, "in_progress"),
        task(6, "blocked"),
    ];

    it("orders the groups: in_progress, review, blocked, todo, done", () => {
        expect(groupTasks(tasks).map((g) => g.status)).toEqual(["in_progress", "review", "blocked", "todo", "done"]);
    });

    it("keeps the oldest task first inside a group, whatever the input order", () => {
        const groups = groupTasks([...tasks].reverse());
        expect(groups[0].tasks.map((t) => t.id)).toEqual([3, 5]);
    });

    it("leaves out a group with no task", () => {
        expect(groupTasks([task(1, "todo")]).map((g) => g.status)).toEqual(["todo"]);
        expect(groupTasks([])).toEqual([]);
    });
});

describe("openCount and the groups that start open", () => {
    it("counts every task that is not done", () => {
        expect(openCount([task(1, "done"), task(2, "todo"), task(3, "blocked"), task(4, "done")])).toBe(2);
        expect(openCount([])).toBe(0);
    });

    it("collapses done only", () => {
        expect(defaultOpenGroups()).toEqual({ in_progress: true, review: true, blocked: true, todo: true, done: false });
    });
});

describe("personName", () => {
    const names = { eng: "Kenji" };
    it("names an employee, the operator, a removed employee and nobody", () => {
        expect(personName("eng", names)).toBe("Kenji");
        expect(personName("operator", names)).toBe("Operator");
        expect(personName("gone", names)).toBe("gone");
        expect(personName(null, names)).toBe("Unassigned");
    });
});

describe("formatTime", () => {
    it("writes the local date and time with two digits", () => {
        expect(formatTime(new Date(2026, 10, 21, 4, 5, 9).getTime())).toBe("2026-11-21 04:05:09");
    });
});

describe("eventText", () => {
    const names = { eng: "Kenji", pm: "Maya" };
    it("says what each kind of event did", () => {
        expect(eventText(event("created", { author: "operator" }), names)).toEqual({
            line: "Operator made the task",
            note: null,
        });
        expect(eventText(event("assigned", { author: "pm", value: "eng" }), names).line).toBe(
            "Maya gave the task to Kenji",
        );
        expect(eventText(event("status", { value: "review", text: "ready" }), names)).toEqual({
            line: "Kenji set the status to Review",
            note: "ready",
        });
        expect(eventText(event("comment", { text: "hello" }), names)).toEqual({
            line: "Kenji wrote a note",
            note: "hello",
        });
        expect(eventText(event("report", { value: ".swarm/reports/T-1.md" }), names).line).toBe(
            "Kenji set the report .swarm/reports/T-1.md",
        );
    });

    it("does not fail on a kind that it does not know", () => {
        const odd = { ...event("comment"), kind: "future" } as unknown as TaskEvent;
        expect(eventText(odd, names).line).toBe("Kenji: future");
    });
});

describe("upsertTask", () => {
    it("replaces a task with the same ID and keeps the order", () => {
        const list = [task(1, "todo"), task(2, "todo")];
        const next = upsertTask(list, task(1, "done"));
        expect(next.map((t) => [t.id, t.status])).toEqual([
            [1, "done"],
            [2, "todo"],
        ]);
        expect(list[0].status).toBe("todo");
    });

    it("adds a new task in the place of its ID", () => {
        expect(upsertTask([task(3, "todo"), task(1, "todo")], task(2, "todo")).map((t) => t.id)).toEqual([1, 2, 3]);
    });
});
