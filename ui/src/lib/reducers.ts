// Pure state reducers for the session store. No Svelte, no Tauri.
import type { FloorView, SessionView, TrackedSession } from "./types";

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
