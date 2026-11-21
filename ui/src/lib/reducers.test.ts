import { describe, expect, it } from "vitest";
import {
    applyFloorView,
    applySessionChanged,
    dismissTab,
    emptySessionState,
    employeeStartedText,
    finishedTurns,
    nextAfter,
    pickActive,
    questionCount,
    questionOrder,
    sessionOf,
    tabSessions,
    trackQuestions,
} from "./reducers";
import type { FloorView, SessionView } from "./types";

const P = "/floor/a";

function session(over: Partial<SessionView> = {}): SessionView {
    return {
        sessionId: "s1",
        slug: "ceo",
        status: "starting",
        draft: false,
        messageWaiting: false,
        canResume: false,
        ...over,
    };
}

describe("session-changed reducer", () => {
    it("adds a new session and gives it a tab", () => {
        const s = applySessionChanged(emptySessionState, { path: P, session: session() });
        expect(s.byId["s1"]).toMatchObject({ path: P, slug: "ceo", status: "starting" });
        expect(tabSessions(s).map((t) => t.sessionId)).toEqual(["s1"]);
    });

    it("does not change the old state object", () => {
        const before = applySessionChanged(emptySessionState, { path: P, session: session() });
        const after = applySessionChanged(before, { path: P, session: session({ status: "working" }) });
        expect(before.byId["s1"].status).toBe("starting");
        expect(after.byId["s1"].status).toBe("working");
        expect(emptySessionState.byId).toEqual({});
    });

    it("updates the status", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session() });
        for (const status of ["idle", "working", "question", "done"] as const) {
            s = applySessionChanged(s, { path: P, session: session({ status }) });
            expect(s.byId["s1"].status).toBe(status);
        }
    });

    it("updates the draft and the message waiting flags", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session({ status: "idle" }) });
        s = applySessionChanged(s, { path: P, session: session({ status: "idle", draft: true }) });
        expect(s.byId["s1"]).toMatchObject({ draft: true, messageWaiting: false });
        s = applySessionChanged(s, { path: P, session: session({ status: "idle", draft: true, messageWaiting: true }) });
        expect(s.byId["s1"]).toMatchObject({ draft: true, messageWaiting: true });
        s = applySessionChanged(s, { path: P, session: session({ status: "idle" }) });
        expect(s.byId["s1"]).toMatchObject({ draft: false, messageWaiting: false });
    });

    it("keeps the tab of a session that stopped, and stores canResume", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session({ status: "idle" }) });
        s = applySessionChanged(s, { path: P, session: session({ status: "stopped", canResume: true }) });
        expect(s.byId["s1"]).toMatchObject({ status: "stopped", canResume: true });
        expect(tabSessions(s).map((t) => t.sessionId)).toEqual(["s1"]);
    });

    it("gives no tab to a session that is stopped when first seen", () => {
        const s = applySessionChanged(emptySessionState, {
            path: P,
            session: session({ status: "stopped", canResume: true }),
        });
        expect(s.byId["s1"].canResume).toBe(true);
        expect(tabSessions(s)).toEqual([]);
    });

    it("replaces the old session of the same employee when the ID is new", () => {
        let s = applySessionChanged(emptySessionState, {
            path: P,
            session: session({ sessionId: "old", status: "stopped", canResume: true }),
        });
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "new", status: "starting" }) });
        expect(Object.keys(s.byId)).toEqual(["new"]);
        expect(sessionOf(s, P, "ceo")?.sessionId).toBe("new");
    });

    it("keeps sessions of other employees and other floors", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session() });
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "s2", slug: "pm" }) });
        s = applySessionChanged(s, { path: "/floor/b", session: session({ sessionId: "s3" }) });
        expect(Object.keys(s.byId).sort()).toEqual(["s1", "s2", "s3"]);
        expect(sessionOf(s, "/floor/b", "ceo")?.sessionId).toBe("s3");
    });

    it("counts sessions in question for one floor", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session({ status: "question" }) });
        s = applySessionChanged(s, { path: "/floor/b", session: session({ sessionId: "s3", status: "question" }) });
        expect(questionCount(s, P)).toBe(1);
    });
});

describe("floor view and tabs", () => {
    const view = (sessions: SessionView[]): FloorView => ({
        path: P,
        name: "A",
        employees: [],
        employeeErrors: [],
        sessions,
        description: null,
        logoPrompt: null,
        logoVersion: null,
        logoBusy: false,
        logoError: null,
        setupSessionId: null,
        defaults: { cli: null, model: null, effort: null, policy: null },
        resumable: [],
        resumeProgress: null,
    });

    it("replaces the sessions of the floor and keeps the other floors", () => {
        let s = applySessionChanged(emptySessionState, { path: "/floor/b", session: session({ sessionId: "b1" }) });
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "gone", slug: "pm" }) });
        s = applyFloorView(s, view([session({ sessionId: "s1", status: "idle" })]));
        expect(Object.keys(s.byId).sort()).toEqual(["b1", "s1"]);
    });

    it("a stopped session from a view has no tab, a live one has", () => {
        const s = applyFloorView(
            emptySessionState,
            view([
                session({ sessionId: "a", slug: "ceo", status: "stopped", canResume: true }),
                session({ sessionId: "b", slug: "pm", status: "working" }),
            ]),
        );
        expect(tabSessions(s).map((t) => t.sessionId)).toEqual(["b"]);
    });

    it("keeps the tab of a session that was live, after a floor reload shows it stopped", () => {
        let s = applyFloorView(emptySessionState, view([session({ status: "idle" })]));
        s = applyFloorView(s, view([session({ status: "stopped", canResume: true })]));
        expect(tabSessions(s).map((t) => t.sessionId)).toEqual(["s1"]);
    });

    it("dismissTab closes a stopped tab but not a live one", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session({ status: "idle" }) });
        expect(tabSessions(dismissTab(s, "s1"))).toHaveLength(1);
        s = applySessionChanged(s, { path: P, session: session({ status: "stopped" }) });
        expect(tabSessions(dismissTab(s, "s1"))).toHaveLength(0);
    });

    it("pickActive keeps the current tab, prefers a given one, else the first", () => {
        const s = applyFloorView(
            emptySessionState,
            view([
                session({ sessionId: "a", slug: "ceo", status: "idle" }),
                session({ sessionId: "b", slug: "pm", status: "idle" }),
            ]),
        );
        const tabs = tabSessions(s);
        expect(pickActive(tabs, "b")).toBe("b");
        expect(pickActive(tabs, "zzz")).toBe("a");
        expect(pickActive(tabs, "a", "b")).toBe("b");
        expect(pickActive([], "a")).toBeNull();
    });
});

describe("a session that an employee started (the UI did not start it)", () => {
    it("gets a tab from the first session-changed, with its status", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session({ sessionId: "ops", status: "idle" }) });
        // The event of a session that the UI never saw before. It is `starting`.
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "emp", slug: "eng", status: "starting" }) });
        expect(tabSessions(s).map((t) => t.sessionId).sort()).toEqual(["emp", "ops"]);
        expect(sessionOf(s, P, "eng")).toMatchObject({ sessionId: "emp", status: "starting" });
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "emp", slug: "eng", status: "working" }) });
        expect(sessionOf(s, P, "eng")?.status).toBe("working");
    });

    it("does not move the active tab: the tab that the operator has open stays", () => {
        let s = applySessionChanged(emptySessionState, { path: P, session: session({ sessionId: "ops", status: "idle" }) });
        const before = pickActive(tabSessions(s), "ops");
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "emp", slug: "eng", status: "starting" }) });
        // The store keeps the ID of the active tab. A new tab changes only the list.
        expect(pickActive(tabSessions(s), before)).toBe("ops");
        expect(pickActive(tabSessions(s), "ops", null)).toBe("ops");
    });

    it("a new session of an employee replaces its old stopped one, and the tab list follows", () => {
        let s = applySessionChanged(emptySessionState, {
            path: P,
            session: session({ sessionId: "old", slug: "eng", status: "stopped", canResume: true }),
        });
        s = applySessionChanged(s, { path: P, session: session({ sessionId: "new", slug: "eng", status: "starting" }) });
        expect(tabSessions(s).map((t) => t.sessionId)).toEqual(["new"]);
    });

    it("the toast text names who started whom", () => {
        expect(employeeStartedText({ byName: "Tomas", name: "Kenji" })).toBe("Tomas started Kenji");
    });
});

describe("finishedTurns", () => {
    const state = (...list: [string, SessionView["status"]][]) =>
        list.reduce((st, [id, status]) => applySessionChanged(st, { path: P, session: session({ sessionId: id, slug: id, status }) }), emptySessionState);

    it("finds a session that went from working to idle or done", () => {
        const prev = state(["a", "working"], ["b", "working"], ["c", "working"], ["d", "idle"]);
        const next = state(["a", "idle"], ["b", "done"], ["c", "question"], ["d", "idle"]);
        expect(finishedTurns(prev, next).map((s) => s.sessionId)).toEqual(["a", "b"]);
    });

    it("ignores a new session, an unchanged session and a start", () => {
        const prev = state(["a", "idle"], ["s", "starting"]);
        const next = state(["a", "idle"], ["s", "idle"], ["new", "idle"]);
        expect(finishedTurns(prev, next)).toEqual([]);
    });
});

describe("question order", () => {
    const state = (...list: [string, SessionView["status"]][]) =>
        list.reduce((st, [id, status]) => applySessionChanged(st, { path: P, session: session({ sessionId: id, slug: id, status }) }), emptySessionState);

    it("numbers sessions in the order in which they started to ask, and keeps the number", () => {
        let since: Record<string, number> = {};
        let seq = 0;
        const step = (st: ReturnType<typeof state>) => {
            const r = trackQuestions(since, st, seq);
            since = r.since;
            seq = r.seq;
            return questionOrder(since, st).map((s) => s.sessionId);
        };
        expect(step(state(["b", "question"], ["a", "idle"]))).toEqual(["b"]);
        expect(step(state(["b", "question"], ["a", "question"]))).toEqual(["b", "a"]);
        // b answers and asks again: it is the newest now.
        expect(step(state(["b", "idle"], ["a", "question"]))).toEqual(["a"]);
        expect(step(state(["b", "question"], ["a", "question"]))).toEqual(["a", "b"]);
        // A session that is gone loses its number.
        expect(step(state(["a", "question"]))).toEqual(["a"]);
        expect(Object.keys(since)).toEqual(["a"]);
    });

    it("returns the same object when nothing changed", () => {
        const st = state(["a", "question"]);
        const first = trackQuestions({}, st, 0);
        expect(trackQuestions(first.since, st, first.seq).since).toBe(first.since);
    });

    it("steps through the questions and wraps", () => {
        const list = [{ sessionId: "a" }, { sessionId: "b" }, { sessionId: "c" }];
        expect(nextAfter(list, null)?.sessionId).toBe("a");
        expect(nextAfter(list, "a")?.sessionId).toBe("b");
        expect(nextAfter(list, "c")?.sessionId).toBe("a");
        expect(nextAfter(list, "gone")?.sessionId).toBe("a");
        expect(nextAfter([{ sessionId: "x" }], "x")?.sessionId).toBe("x");
        expect(nextAfter([], null)).toBeNull();
    });
});
