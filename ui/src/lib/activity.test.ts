import { describe, expect, it } from "vitest";
import { ACTIVITY_SHOWN, activityTime, lineText, sessionDrafts, startedDraft, taskDrafts, visibleActivity, type ActivityEntry } from "./activity";
import { employeeLabel, emptySessionState, type SessionState } from "./reducers";
import type { Employee, Status, TaskView } from "./types";

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
