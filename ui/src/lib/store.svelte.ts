// The one store of the UI: floors, the open floor, sessions by ID, the active tab, toasts.
// Svelte 5 runes. The pure state changes live in ./reducers.ts.
import * as ipc from "./ipc";
import {
    applyFloorView,
    applySessionChanged,
    dismissTab,
    emptySessionState,
    isLive,
    pickActive,
    questionCount,
    sessionOf,
    tabSessions,
    upsertSession,
    type SessionState,
} from "./reducers";
import type {
    Employee,
    FloorEntry,
    FloorView,
    ProgramName,
    Programs,
    SessionView,
    TrackedSession,
} from "./types";

export interface Toast {
    id: number;
    text: string;
}

const LAST_FLOOR_KEY = "agent-tower:last-floor";
const TOAST_MS = 9000;

function readLastFloor(): string | null {
    try {
        return localStorage.getItem(LAST_FLOOR_KEY);
    } catch {
        return null;
    }
}

function writeLastFloor(path: string): void {
    try {
        localStorage.setItem(LAST_FLOOR_KEY, path);
    } catch {
        // Storage can be blocked. The UI works without it.
    }
}

class Store {
    floors = $state.raw<FloorEntry[]>([]);
    openPath = $state<string | null>(null);
    floorView = $state.raw<FloorView | null>(null);
    sessionState = $state.raw<SessionState>(emptySessionState);
    activeByPath = $state.raw<Record<string, string>>({});
    toasts = $state.raw<Toast[]>([]);
    programs = $state.raw<Programs | null>(null);
    /** Set when `floor_open` failed with `NO_FLOOR:`. The UI offers to create the floor. */
    pendingCreate = $state.raw<{ path: string; message: string } | null>(null);
    backend = $state<"tauri" | "mock" | "">("");
    ready = $state(false);

    /** The size of the last terminal that fitted. New sessions start with it. */
    lastSize = { rows: 30, cols: 110 };

    private toastId = 0;
    private unlisten: ipc.Unlisten[] = [];

    /** Tabs of the open floor, in a stable order. */
    tabs = $derived.by(() => {
        const path = this.openPath;
        return tabSessions(this.sessionState)
            .filter((s) => s.path === path)
            .sort((a, b) => this.employeeIndex(a.slug) - this.employeeIndex(b.slug));
    });

    /** Sessions that need a terminal instance: every tab of every floor. */
    allTabSessions = $derived(tabSessions(this.sessionState));

    activeId = $derived.by(() => {
        const path = this.openPath;
        if (!path) return null;
        return pickActive(this.tabs, this.activeByPath[path] ?? null);
    });

    activeSession = $derived(this.activeId ? (this.sessionState.byId[this.activeId] ?? null) : null);

    private employeeIndex(slug: string): number {
        const i = this.floorView?.employees.findIndex((e) => e.slug === slug) ?? -1;
        return i < 0 ? Number.MAX_SAFE_INTEGER : i;
    }

    // ---- start up ----

    async init(): Promise<void> {
        this.backend = await ipc.backendKind();
        try {
            this.unlisten.push(
                await ipc.onSessionChanged((payload) => {
                    this.sessionState = applySessionChanged(this.sessionState, payload);
                }),
                await ipc.onFloorChanged((payload) => {
                    if (payload.path === this.openPath) void this.reloadFloor();
                }),
            );
        } catch (e) {
            this.toast(`Cannot listen to events: ${ipc.errorText(e)}`);
        }
        await this.loadFloors();
        void this.loadPrograms();
        const last = readLastFloor();
        const first = this.floors.find((f) => f.path === last && !f.missing) ?? this.floors.find((f) => !f.missing);
        if (first) await this.openFloor(first.path);
        this.ready = true;
    }

    dispose(): void {
        for (const u of this.unlisten) u();
        this.unlisten = [];
    }

    // ---- toasts ----

    toast(text: string): void {
        const id = ++this.toastId;
        this.toasts = [...this.toasts, { id, text }];
        setTimeout(() => this.dismissToast(id), TOAST_MS);
    }

    dismissToast(id: number): void {
        this.toasts = this.toasts.filter((t) => t.id !== id);
    }

    private fail(e: unknown): void {
        this.toast(ipc.errorText(e));
    }

    // ---- floors ----

    async loadFloors(): Promise<void> {
        try {
            this.floors = await ipc.floorList();
        } catch (e) {
            this.fail(e);
        }
    }

    private applyView(view: FloorView): void {
        this.floorView = view;
        this.sessionState = applyFloorView(this.sessionState, view);
    }

    /** Open a floor by path. Returns true on success. */
    async openFloor(path: string): Promise<boolean> {
        const clean = path.trim();
        if (clean === "") {
            this.toast("Enter a folder path first.");
            return false;
        }
        try {
            const view = await ipc.floorOpen(clean);
            this.pendingCreate = null;
            this.openPath = view.path;
            this.applyView(view);
            writeLastFloor(view.path);
            await this.loadFloors();
            return true;
        } catch (e) {
            const text = ipc.errorText(e);
            if (text.startsWith("NO_FLOOR:")) {
                this.pendingCreate = { path: clean, message: text };
            } else {
                this.toast(text);
            }
            return false;
        }
    }

    async createFloor(path: string, name: string): Promise<boolean> {
        try {
            const view = await ipc.floorCreate(path, name.trim());
            this.pendingCreate = null;
            this.openPath = view.path;
            this.applyView(view);
            writeLastFloor(view.path);
            await this.loadFloors();
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    cancelCreate(): void {
        this.pendingCreate = null;
    }

    async forgetFloor(path: string): Promise<void> {
        try {
            await ipc.floorForget(path);
            if (this.openPath === path) {
                this.openPath = null;
                this.floorView = null;
            }
            await this.loadFloors();
        } catch (e) {
            this.fail(e);
        }
    }

    async reloadFloor(): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            const view = await ipc.floorOpen(path);
            if (this.openPath === path) this.applyView(view);
        } catch (e) {
            this.fail(e);
        }
    }

    // ---- employees ----

    async saveEmployee(employee: Employee): Promise<boolean> {
        const path = this.openPath;
        if (!path) return false;
        try {
            this.applyView(await ipc.employeeSave(path, employee));
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    async removeEmployee(slug: string): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            this.applyView(await ipc.employeeRemove(path, slug));
        } catch (e) {
            this.fail(e);
        }
    }

    // ---- sessions and tabs ----

    sessionOfEmployee(slug: string): TrackedSession | undefined {
        return this.openPath ? sessionOf(this.sessionState, this.openPath, slug) : undefined;
    }

    questionsIn(path: string): number {
        return questionCount(this.sessionState, path);
    }

    selectTab(sessionId: string): void {
        const path = this.openPath;
        if (!path) return;
        this.activeByPath = { ...this.activeByPath, [path]: sessionId };
    }

    closeTab(sessionId: string): void {
        this.sessionState = dismissTab(this.sessionState, sessionId);
    }

    private adopt(session: SessionView): void {
        const path = this.openPath;
        if (!path) return;
        this.sessionState = upsertSession(this.sessionState, path, session);
        this.selectTab(session.sessionId);
    }

    async startSession(slug: string): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            const { rows, cols } = this.lastSize;
            this.adopt(await ipc.sessionStart(path, slug, null, rows, cols));
        } catch (e) {
            this.fail(e);
        }
    }

    async resumeSession(slug: string): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        try {
            const { rows, cols } = this.lastSize;
            this.adopt(await ipc.sessionResume(path, slug, null, rows, cols));
        } catch (e) {
            this.fail(e);
        }
    }

    async stopSession(sessionId: string): Promise<void> {
        try {
            await ipc.sessionStop(sessionId);
        } catch (e) {
            this.fail(e);
        }
    }

    async clearDraft(sessionId: string): Promise<void> {
        try {
            await ipc.clearDraft(sessionId);
        } catch (e) {
            this.fail(e);
        }
    }

    /** Returns the refusal text, or null when the paste went through. */
    async debugPaste(sessionId: string, text: string): Promise<string | null> {
        try {
            await ipc.debugPaste(sessionId, text);
            return null;
        } catch (e) {
            return ipc.errorText(e);
        }
    }

    isLive(session: SessionView | undefined): boolean {
        return !!session && isLive(session);
    }

    reportSize(rows: number, cols: number): void {
        if (rows > 0 && cols > 0) this.lastSize = { rows, cols };
    }

    // ---- programs ----

    async loadPrograms(): Promise<void> {
        try {
            this.programs = await ipc.programsGet();
        } catch (e) {
            this.fail(e);
        }
    }

    async setProgram(name: ProgramName, path: string): Promise<boolean> {
        try {
            this.programs = await ipc.programsSet(name, path.trim());
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }
}

export const store = new Store();
