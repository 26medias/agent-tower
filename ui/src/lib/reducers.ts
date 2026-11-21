// Pure state reducers for the session store. No Svelte, no Tauri.
import type { Employee, EmployeeStartedPayload, FloorView, SessionView, TrackedSession } from "./types";

export interface SessionState {
    /** Sessions by session ID, for all floors that the UI knows. */
    byId: Record<string, TrackedSession>;
    /** IDs of sessions that were live in this run of the UI. They keep a tab after they stop. */
    seenLive: Record<string, true>;
}

export const emptySessionState: SessionState = { byId: {}, seenLive: {} };

export function isLive(session: SessionView): boolean {
    return session.status !== "stopped";
}

/**
 * Put one session in the state. A floor holds one session for each employee, so a session
 * with a new ID for the same floor and employee replaces the old one.
 */
export function upsertSession(state: SessionState, path: string, session: SessionView): SessionState {
    const byId: Record<string, TrackedSession> = {};
    const seenLive: Record<string, true> = {};
    for (const [id, old] of Object.entries(state.byId)) {
        if (old.path === path && old.slug === session.slug && id !== session.sessionId) continue;
        byId[id] = old;
        if (state.seenLive[id]) seenLive[id] = true;
    }
    byId[session.sessionId] = { ...session, path };
    if (state.seenLive[session.sessionId] || isLive(session)) seenLive[session.sessionId] = true;
    return { byId, seenLive };
}

/** Reducer for the `session-changed` event. */
export function applySessionChanged(
    state: SessionState,
    payload: { path: string; session: SessionView },
): SessionState {
    return upsertSession(state, payload.path, payload.session);
}

/** The sessions of a loaded `FloorView` replace all sessions of that floor. */
export function applyFloorView(state: SessionState, view: FloorView): SessionState {
    const byId: Record<string, TrackedSession> = {};
    const seenLive: Record<string, true> = {};
    for (const [id, old] of Object.entries(state.byId)) {
        if (old.path === view.path) continue;
        byId[id] = old;
        if (state.seenLive[id]) seenLive[id] = true;
    }
    for (const session of view.sessions) {
        byId[session.sessionId] = { ...session, path: view.path };
        if (state.seenLive[session.sessionId] || isLive(session)) seenLive[session.sessionId] = true;
    }
    return { byId, seenLive };
}

/** Drop a tab of a stopped session. A live session cannot be dismissed. */
export function dismissTab(state: SessionState, sessionId: string): SessionState {
    const session = state.byId[sessionId];
    if (!session || isLive(session)) return state;
    const seenLive = { ...state.seenLive };
    delete seenLive[sessionId];
    return { ...state, seenLive };
}

/** Sessions that have a tab: live, or live earlier in this run. */
export function tabSessions(state: SessionState): TrackedSession[] {
    return Object.values(state.byId).filter((s) => state.seenLive[s.sessionId]);
}

/** The session of one employee in one floor, if any. */
export function sessionOf(state: SessionState, path: string, slug: string): TrackedSession | undefined {
    return Object.values(state.byId).find((s) => s.path === path && s.slug === slug);
}

/** Number of live sessions in `question` for one floor. */
export function questionCount(state: SessionState, path: string): number {
    return Object.values(state.byId).filter((s) => s.path === path && s.status === "question").length;
}

/** Pick the active tab after a change. Keep `current` when it still has a tab. */
export function pickActive(
    tabs: TrackedSession[],
    current: string | null,
    preferred: string | null = null,
): string | null {
    if (preferred && tabs.some((t) => t.sessionId === preferred)) return preferred;
    if (current && tabs.some((t) => t.sessionId === current)) return current;
    return tabs.length > 0 ? tabs[0].sessionId : null;
}

/** The text of the toast for an employee that started another employee: "Tomas started Kenji". */
export function employeeStartedText(payload: Pick<EmployeeStartedPayload, "byName" | "name">): string {
    return `${payload.byName} started ${payload.name}`;
}

/** Sessions that went from `working` to `idle` or `done` between two states: a turn that finished. */
export function finishedTurns(prev: SessionState, next: SessionState): TrackedSession[] {
    return Object.values(next.byId).filter((s) => {
        const before = prev.byId[s.sessionId];
        return !!before && before.status === "working" && (s.status === "idle" || s.status === "done");
    });
}

/**
 * Keep the order in which sessions started to ask a question. A session that asks and has no number
 * gets the next one. A session that does not ask any more, or is gone, loses its number. The result
 * is the same object when nothing changed. Lower is older.
 */
export function trackQuestions(
    since: Record<string, number>,
    state: SessionState,
    seq: number,
): { since: Record<string, number>; seq: number } {
    const next = { ...since };
    let n = seq;
    let changed = false;
    for (const s of Object.values(state.byId)) {
        if (s.status === "question") {
            if (!(s.sessionId in next)) {
                next[s.sessionId] = ++n;
                changed = true;
            }
        } else if (s.sessionId in next) {
            delete next[s.sessionId];
            changed = true;
        }
    }
    for (const id of Object.keys(next)) {
        if (!state.byId[id]) {
            delete next[id];
            changed = true;
        }
    }
    return changed ? { since: next, seq: n } : { since, seq };
}

/** The sessions in `question` on all floors, the oldest first. */
export function questionOrder(since: Record<string, number>, state: SessionState): TrackedSession[] {
    return Object.values(state.byId)
        .filter((s) => s.status === "question")
        .sort((a, b) => (since[a.sessionId] ?? 0) - (since[b.sessionId] ?? 0));
}

/** The question that "Open" opens now: the one after `last` in the list, wrapping. With no `last`, the oldest. */
export function nextAfter<T extends { sessionId: string }>(list: T[], last: string | null): T | null {
    if (list.length === 0) return null;
    const at = list.findIndex((q) => q.sessionId === last);
    return list[(at + 1) % list.length];
}

/**
 * "Eli (Engineer)": the name and the role of an employee. `employees` is the list of the floor, or
 * `undefined` when the UI does not know the floor yet. Then the text is the slug: nothing else is known.
 */
export function employeeLabel(employees: readonly Employee[] | undefined, slug: string): string {
    const emp = employees?.find((e) => e.slug === slug);
    return emp ? `${emp.name} (${emp.role})` : slug;
}
