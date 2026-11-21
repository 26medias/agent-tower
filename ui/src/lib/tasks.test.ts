import { describe, expect, it } from "vitest";
import {
    STATUS_CHOICES,
    STATUS_ORDER,
    TASK_COLOR,
    canDiscard,
    canMerge,
    defaultOpenGroups,
    diffPlaceholder,
    diffText,
    eventText,
    hasBranch,
    shortCommit,
    worktreePath,
    formatTime,
    groupTasks,
    openCount,
    personName,
    statusLabel,
    taskLabel,
    upsertTask,
    DONE_KEEP,
    NO_FILTER,
    UNASSIGNED,
    filterTasks,
    isFiltering,
    splitDone,
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
        branch: null,
        baseBranch: null,
        baseCommit: null,
        worktree: null,
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

const withBranch = (status: TaskStatus, extra: Partial<TaskView> = {}) =>
    task(7, status, {
        branch: "task/T-7-add-login",
        baseBranch: "main",
        baseCommit: "0123456789abcdef0123456789abcdef01234567",
        worktree: ".swarm/worktrees/T-7",
        ...extra,
    });

describe("the git branch of a task (docs/ipc-m3.md)", () => {
    it("knows a task with a branch", () => {
        expect(hasBranch(withBranch("todo"))).toBe(true);
        expect(hasBranch(task(1, "todo"))).toBe(false);
    });

    it("lets the operator merge a task with a branch in review or blocked, and no other", () => {
        for (const status of STATUS_CHOICES) {
            const expected = status === "review" || status === "blocked";
            expect(canMerge(withBranch(status)), status).toBe(expected);
            // A task with no branch has nothing to merge from the board.
            expect(canMerge(task(1, status)), `no branch, ${status}`).toBe(false);
        }
    });

    it("lets the operator discard a task that is not done and has a branch or a worktree", () => {
        expect(canDiscard(withBranch("in_progress"))).toBe(true);
        expect(canDiscard(withBranch("blocked"))).toBe(true);
        expect(canDiscard(withBranch("done"))).toBe(false);
        expect(canDiscard(task(1, "in_progress"))).toBe(false);
        // A worktree that is left after a merge that could not remove it.
        expect(canDiscard(task(1, "review", { worktree: ".swarm/worktrees/T-1" }))).toBe(true);
    });

    it("shortens a commit and builds the absolute worktree path", () => {
        expect(shortCommit("0123456789abcdef0123456789abcdef01234567")).toBe("0123456789ab");
        expect(shortCommit(null)).toBe("");
        expect(worktreePath("/home/me/floor", withBranch("review"))).toBe("/home/me/floor/.swarm/worktrees/T-7");
        expect(worktreePath("/home/me/floor/", withBranch("review"))).toBe("/home/me/floor/.swarm/worktrees/T-7");
        expect(worktreePath("/f", withBranch("done", { worktree: null }))).toBeNull();
    });

    it("shows the diff text, the placeholder and the note", () => {
        const diff = { stat: " a.txt | 1 +\n", patch: "diff --git a/a.txt b/a.txt\n+A\n", note: null };
        expect(diffText(diff)).toBe(" a.txt | 1 +\n\ndiff --git a/a.txt b/a.txt\n+A");
        expect(diffText({ ...diff, note: "The patch is cut." })).toContain("\n\n[The patch is cut.]");
        expect(diffPlaceholder({ branch: "b", ...diff })).toBeNull();
        expect(diffPlaceholder({ branch: "b", stat: "", patch: "", note: null })).toBe("There is no change on the branch yet.");
        expect(diffPlaceholder({ branch: null, stat: "", patch: "", note: "No git branch." })).toBe("No git branch.");
    });

    it("writes the worktree event", () => {
        const e = event("worktree", { author: "pm", value: "task/T-7-add-login", text: "Branch task/T-7-add-login from main at 0123456789ab." });
        expect(eventText(e, {})).toEqual({
            line: "The app made the branch task/T-7-add-login",
            note: "Branch task/T-7-add-login from main at 0123456789ab.",
        });
    });
});

describe("filterTasks", () => {
    const list = [
        task(1, "todo", { title: "Write the login form", assignee: "eng-lin" }),
        task(2, "done", { title: "Fix the build", description: "Linux only", assignee: "eng-sam" }),
        task(3, "review", { title: "Plan the release", assignee: null }),
    ];
    const names = { "eng-lin": "Lin", "eng-sam": "Sam" };
    const ids = (l: TaskView[]) => l.map((t) => t.id);

    it("keeps all tasks with no filter", () => {
        expect(filterTasks(list, NO_FILTER, names)).toEqual(list);
        expect(isFiltering(NO_FILTER)).toBe(false);
        expect(isFiltering({ assignee: "", query: "  " })).toBe(false);
        expect(isFiltering({ assignee: "x", query: "" })).toBe(true);
    });

    it("filters by assignee, and by no assignee", () => {
        expect(ids(filterTasks(list, { assignee: "eng-sam", query: "" }, names))).toEqual([2]);
        expect(ids(filterTasks(list, { assignee: UNASSIGNED, query: "" }, names))).toEqual([3]);
    });

    it("searches title, description, label, assignee name and status, ignoring case", () => {
        expect(ids(filterTasks(list, { assignee: "", query: "LOGIN" }, names))).toEqual([1]);
        expect(ids(filterTasks(list, { assignee: "", query: "linux" }, names))).toEqual([2]);
        expect(ids(filterTasks(list, { assignee: "", query: "t-3" }, names))).toEqual([3]);
        expect(ids(filterTasks(list, { assignee: "", query: "sam" }, names))).toEqual([2]);
        expect(ids(filterTasks(list, { assignee: "", query: "review" }, names))).toEqual([3]);
    });

    it("needs every word of the query, and combines with the assignee", () => {
        expect(ids(filterTasks(list, { assignee: "", query: "the build" }, names))).toEqual([2]);
        expect(ids(filterTasks(list, { assignee: "", query: "the zzz" }, names))).toEqual([]);
        expect(ids(filterTasks(list, { assignee: "eng-lin", query: "build" }, names))).toEqual([]);
    });
});

describe("splitDone", () => {
    it("keeps the 3 newest tasks in view, newest first, and folds the rest", () => {
        const done = [1, 2, 3, 4, 5].map((id) => task(id, "done", { updatedAt: id === 2 ? 99_000 : id * 1000 }));
        const { recent, older } = splitDone(done);
        expect(DONE_KEEP).toBe(3);
        expect(recent.map((t) => t.id)).toEqual([2, 5, 4]);
        expect(older.map((t) => t.id)).toEqual([3, 1]);
    });

    it("has no older tasks when 3 or fewer are done", () => {
        const { recent, older } = splitDone([task(1, "done"), task(2, "done")]);
        expect(recent.map((t) => t.id)).toEqual([2, 1]);
        expect(older).toEqual([]);
    });

    it("breaks a tie of the time by the ID", () => {
        const { recent } = splitDone([task(1, "done", { updatedAt: 5 }), task(2, "done", { updatedAt: 5 })]);
        expect(recent.map((t) => t.id)).toEqual([2, 1]);
    });
});
