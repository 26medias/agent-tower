import { describe, expect, it } from "vitest";
import {
    ACTIVITY_SHOWN,
    FOLD_MS,
    activityTime,
    appendActivity,
    exchangeDraft,
    lineText,
    messageDraft,
    sessionDrafts,
    startedDraft,
    statusPhrase,
    taskDrafts,
    visibleActivity,
    type ActivityDraft,
    type ActivityEntry,
} from "./activity";
import { employeeLabel, emptySessionState, type SessionState } from "./reducers";
import type { Employee, MessageDeliveredPayload, Status, TaskView } from "./types";

function state(...rows: [string, string, Status][]): SessionState {
    const byId: SessionState["byId"] = {};
    for (const [id, slug, status] of rows) {
        byId[id] = { sessionId: id, slug, status, draft: false, messageWaiting: false, canResume: false, path: "/f" };
    }
    return { byId, seenLive: {} };
}

const label = (s: { slug: string }) => s.slug.toUpperCase();

function task(id: number, over: Partial<TaskView> = {}): TaskView {
    return {
        id,
        title: `Task ${id}`,
        description: "",
        parent: null,
        creator: "pm",
        assignee: null,
        assigner: null,
        status: "todo",
        report: null,
        createdAt: 0,
        updatedAt: 0,
        branch: null,
        baseBranch: null,
        baseCommit: null,
        worktree: null,
        ...over,
    };
}

describe("session lines", () => {
    it("says a status change, and says nothing when the status did not change", () => {
        const a = state(["s1", "ceo", "idle"]);
        expect(sessionDrafts(a, a, label, true)).toEqual([]);
        const b = state(["s1", "ceo", "working"]);
        expect(sessionDrafts(a, b, label, true).map((d) => d.text)).toEqual(["CEO is working"]);
    });

    it("says answered at the end of a turn, and marks a question", () => {
        const working = state(["s1", "ceo", "working"]);
        const idle = state(["s1", "ceo", "idle"]);
        expect(sessionDrafts(working, idle, label, false)[0].text).toBe("CEO answered");
        const question = state(["s1", "ceo", "question"]);
        const [q] = sessionDrafts(working, question, label, false);
        expect(q.kind).toBe("question");
        expect(q.text).toBe("CEO asks a question");
        expect(q.sessionId).toBe("s1");
        expect(q.slug).toBe("ceo");
        // Idle after a question is not an answer of a turn.
        expect(sessionDrafts(question, idle, label, false)[0].text).toBe("CEO is idle");
    });

    it("a new session is news only when the caller says so", () => {
        const next = state(["s1", "ceo", "starting"]);
        expect(sessionDrafts(emptySessionState, next, label, false)).toEqual([]);
        expect(sessionDrafts(emptySessionState, next, label, true).map((d) => d.text)).toEqual(["CEO started"]);
    });
});

describe("the text of a session line", () => {
    it("names the employee when the line is drawn, so a late roster gives the name", () => {
        const [q] = sessionDrafts(state(["s1", "pm", "working"]), state(["s1", "pm", "question"]), label, false);
        const line: ActivityEntry = { ...q, id: 1, at: 0 };
        expect(lineText(line, (_path, slug) => slug)).toBe("pm asks a question");
        expect(lineText(line, () => "Pia (Project Manager)")).toBe("Pia (Project Manager) asks a question");
        // A line that is not about a session keeps its own text.
        const task: ActivityEntry = { id: 2, at: 0, path: "/f", kind: "task", text: "T-1 is now done", taskId: 1 };
        expect(lineText(task, () => "x")).toBe("T-1 is now done");
    });
});

describe("started and task lines", () => {
    it("names who started whom", () => {
        expect(startedDraft("/f", "Ada", "Lin", "s2", "eng-lin")).toMatchObject({ text: "Ada started Lin", kind: "started", sessionId: "s2" });
    });

    it("the first load of a floor is not news", () => {
        expect(taskDrafts("/f", undefined, [task(1)], (s) => s)).toEqual([]);
    });

    it("says a new task, a new owner and a new status", () => {
        const who = (s: string) => (s === "operator" ? "You" : s === "pm" ? "Maya" : s);
        const before = [task(1), task(2, { assignee: "a" })];
        const after = [
            task(1, { status: "in_progress" }),
            task(2, { assignee: null }),
            task(3, { title: "Plan", creator: "operator" }),
        ];
        const texts = taskDrafts("/f", before, after, who).map((d) => d.text);
        expect(texts).toEqual(["T-1 is now in progress", "T-2 has no owner now", "You made T-3: Plan"]);
        // The board calls the status "Review". After "is now" it reads "in review".
        const review = taskDrafts("/f", [task(2)], [task(2, { status: "review" })], who).map((d) => d.text);
        expect(review).toEqual(["T-2 is now in review"]);
        const [first] = taskDrafts("/f", before, after, who);
        expect(first.taskId).toBe(1);
        expect(taskDrafts("/f", after, after, who)).toEqual([]);
    });
});

describe("the list", () => {
    const entry = (id: number, path = "/f"): ActivityEntry => ({ id, at: 0, path, kind: "session", text: `t${id}` });

    it("shows the open floor only, newest first, at most 30 lines", () => {
        const all = [...Array.from({ length: 40 }, (_, i) => entry(i + 1)), entry(99, "/other")];
        const lines = visibleActivity(all, "/f");
        expect(lines).toHaveLength(ACTIVITY_SHOWN);
        expect(lines[0].id).toBe(40);
        expect(lines.every((l) => l.path === "/f")).toBe(true);
        expect(visibleActivity(all, null)).toEqual([]);
    });

    it("writes the time as HH:MM", () => {
        expect(activityTime(new Date(2026, 0, 2, 3, 4).getTime())).toBe("03:04");
    });
});

describe("the label of an employee", () => {
    const emps = [{ slug: "eng", name: "Bo", role: "Engineer" }] as Employee[];

    it("is the name and the role when the floor is known", () => {
        expect(employeeLabel(emps, "eng")).toBe("Bo (Engineer)");
    });

    it("falls back to the slug when the floor or the employee is not known", () => {
        expect(employeeLabel(undefined, "eng")).toBe("eng");
        expect(employeeLabel(emps, "ghost")).toBe("ghost");
    });
});

describe("the words after \"is now\"", () => {
    it("reads in review, in progress, done, blocked and to do", () => {
        expect(statusPhrase("review")).toBe("in review");
        expect(statusPhrase("in_progress")).toBe("in progress");
        expect(statusPhrase("done")).toBe("done");
        expect(statusPhrase("blocked")).toBe("blocked");
        expect(statusPhrase("todo")).toBe("to do");
    });
});

function delivered(over: Partial<MessageDeliveredPayload> = {}): MessageDeliveredPayload {
    return {
        path: "/f",
        fromSlug: "pm",
        fromName: "Maya",
        toSlug: "eng",
        toName: "Eli",
        kind: "message",
        taskId: null,
        taskStatus: null,
        preview: "Also, at the end of your work, say the word BANANA.",
        ...over,
    };
}

describe("a delivered message", () => {
    it("ends with an ellipsis when the preview has the full 80 characters (it is probably cut)", () => {
        const full = "x".repeat(80);
        expect(messageDraft(delivered({ preview: full })).text).toBe(`Maya \u2192 Eli: ${full}\u2026`);
        expect(messageDraft(delivered({ preview: "x".repeat(79) })).text).toBe(`Maya \u2192 Eli: ${"x".repeat(79)}`);
    });

    it("is a row \"Maya \u2192 Eli: <preview>\" that points at the receiver", () => {
        expect(messageDraft(delivered())).toEqual({
            path: "/f",
            kind: "message",
            text: "Maya \u2192 Eli: Also, at the end of your work, say the word BANANA.",
            slug: "eng",
            fromSlug: "pm",
        });
    });

    it("says that a task was assigned, or that its status changed", () => {
        expect(messageDraft(delivered({ kind: "task", taskId: 3 })).text).toBe("Maya \u2192 Eli: task T-3 assigned");
        expect(messageDraft(delivered({ kind: "task", taskId: 3, taskStatus: "review" })).text).toBe("Maya \u2192 Eli: T-3 is now in review");
        expect(messageDraft(delivered({ kind: "task", taskId: 3, taskStatus: "in_progress" })).text).toBe("Maya \u2192 Eli: T-3 is now in progress");
        expect(messageDraft(delivered({ kind: "task", taskId: 3, taskStatus: "" })).text).toBe("Maya \u2192 Eli: T-3 changed");
    });
});

describe("a stopped exchange", () => {
    it("is one row with the text of the notice that points at the refused employee", () => {
        const text = "The app stopped an exchange between Maya and Eli: 8 messages in 5 minutes.";
        expect(exchangeDraft("/f", text, "pm")).toEqual({ path: "/f", kind: "exchange", text, slug: "pm" });
    });

    it("is never folded away, and it keeps its text", () => {
        const text = "The app stopped an exchange between Maya and Eli: 8 messages in 5 minutes.";
        let id = 0;
        let list = appendActivity([], [{ path: "/f", kind: "session", text: "Maya is working", slug: "pm", words: "is working" }], 1000, () => ++id);
        list = appendActivity(list, [exchangeDraft("/f", text, "pm")], 2000, () => ++id);
        list = appendActivity(list, [{ path: "/f", kind: "session", text: "Maya answered", slug: "pm", words: "answered" }], 3000, () => ++id);
        expect(list.map((l) => l.kind)).toEqual(["exchange", "session"]);
        expect(lineText(list[0], () => "X")).toBe(text);
    });
});

describe("the fold of the status rows", () => {
    const status = (slug: string, words: string, path = "/f"): ActivityDraft => ({ path, kind: "session", text: `${slug} ${words}`, slug, words });
    const other = (kind: ActivityDraft["kind"], slug: string, extra: Partial<ActivityDraft> = {}): ActivityDraft => ({ path: "/f", kind, text: `${kind} ${slug}`, slug, ...extra });
    const T0 = 1_000_000;
    let id = 0;
    const next = () => ++id;
    const texts = (list: ActivityEntry[]) => list.map((l) => l.text);
    const run = (steps: [number, ActivityDraft][]): ActivityEntry[] => {
        let list: ActivityEntry[] = [];
        for (const [at, draft] of steps) list = appendActivity(list, [draft], T0 + at, next);
        return list;
    };

    it("keeps one status row for an employee, not a history of each turn", () => {
        const list = run([
            [0, status("eng", "is working")],
            [1000, status("eng", "answered")],
            [2000, status("eng", "is working")],
            [3000, status("eng", "answered")],
        ]);
        expect(texts(list)).toEqual(["eng answered"]);
        expect(list[0].at).toBe(T0 + 3000);
    });

    it("folds each employee on its own", () => {
        const list = run([
            [0, status("eng", "is working")],
            [10, status("pm", "is working")],
            [20, status("eng", "answered")],
        ]);
        expect(texts(list)).toEqual(["pm is working", "eng answered"]);
    });

    it("never removes a question, a start, a task row or a message row", () => {
        const list = run([
            [0, other("question", "eng", { text: "eng asks a question" })],
            [1, other("started", "eng", { text: "Ada started eng" })],
            [2, other("task", "eng", { taskId: 1, text: "T-1 goes to eng" })],
            [3, other("message", "eng", { fromSlug: "pm", text: "Maya \u2192 Eli: hi" })],
            [4, status("eng", "is working")],
            [5, status("eng", "answered")],
        ]);
        expect(texts(list)).toEqual(["eng asks a question", "Ada started eng", "T-1 goes to eng", "Maya \u2192 Eli: hi", "eng answered"]);
    });

    it("does not fold across a row of another kind for the same employee", () => {
        const list = run([
            [0, status("eng", "is working")],
            [1, other("question", "eng", { text: "eng asks a question" })],
            [2, status("eng", "is working")],
        ]);
        expect(texts(list)).toEqual(["eng is working", "eng asks a question", "eng is working"]);
        // A start row of the same employee also lies between.
        const started = run([
            [0, status("eng", "is working")],
            [1, other("started", "eng", { text: "Ada started eng" })],
            [2, status("eng", "is working")],
        ]);
        expect(texts(started)).toEqual(["eng is working", "Ada started eng", "eng is working"]);
    });

    it("folds across a message, to the employee or from it (the live run showed \"Maya answered\" twice)", () => {
        // The order of the live run "dc-activity": Maya answers, then her message to Eli is delivered, then Eli's message
        // to Maya, then Eli answers, then Maya works and answers again. One "Maya answered" row is left, the newest, and
        // Eli keeps its own status row (each employee that took part has one).
        const list = run([
            [0, status("pm", "is working")],
            [1, status("pm", "answered")],
            [2, other("message", "eng", { fromSlug: "pm", text: "Maya \u2192 Eli: say MANGO" })],
            [3, other("message", "pm", { fromSlug: "eng", text: "Eli \u2192 Maya: MANGO" })],
            [4, status("eng", "answered")],
            [5, status("pm", "is working")],
            [6, status("pm", "answered")],
        ]);
        expect(texts(list)).toEqual(["Maya \u2192 Eli: say MANGO", "Eli \u2192 Maya: MANGO", "eng answered", "pm answered"]);
        expect(list.filter((l) => l.kind === "session" && l.slug === "pm")).toHaveLength(1);
        expect(list.filter((l) => l.kind === "session" && l.slug === "eng")).toHaveLength(1);
    });

    it("keeps the newest status row of an employee after a message that the same employee sent", () => {
        // The "answered" row replaces the old "is working" row. It is not dropped: an employee that took part always
        // has one status row (the live run "final-b6" showed an employee with none).
        const list = run([
            [0, status("pm", "is working")],
            [1, other("message", "eng", { fromSlug: "pm", text: "Maya \u2192 Eli: hi" })],
            [2, status("pm", "answered")],
        ]);
        expect(texts(list)).toEqual(["Maya \u2192 Eli: hi", "pm answered"]);
        const received = run([
            [0, status("eng", "is working")],
            [1, other("message", "eng", { fromSlug: "pm", text: "Maya \u2192 Eli: hi" })],
            [2, status("eng", "answered")],
        ]);
        expect(texts(received)).toEqual(["Maya \u2192 Eli: hi", "eng answered"]);
    });

    it("folds across a row that is about another employee", () => {
        const list = run([
            [0, status("eng", "is working")],
            [1, other("question", "pm", { text: "pm asks a question" })],
            [2, status("eng", "answered")],
        ]);
        expect(texts(list)).toEqual(["pm asks a question", "eng answered"]);
    });

    it("does not fold a status row that is 5 minutes old or older", () => {
        const young = run([
            [0, status("eng", "is working")],
            [FOLD_MS - 1, status("eng", "answered")],
        ]);
        expect(texts(young)).toEqual(["eng answered"]);
        const old = run([
            [0, status("eng", "is working")],
            [FOLD_MS, status("eng", "answered")],
        ]);
        expect(texts(old)).toEqual(["eng is working", "eng answered"]);
    });

    it("keeps the floors apart", () => {
        const list = run([
            [0, status("eng", "is working", "/a")],
            [1, status("eng", "answered", "/b")],
        ]);
        expect(texts(list)).toEqual(["eng is working", "eng answered"]);
    });
});
