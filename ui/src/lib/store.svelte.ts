// The one store of the UI: floors, the open floor, sessions by ID, the active tab, the selected
// employee, logos, settings, toasts.
// Svelte 5 runes. The pure state changes live in ./reducers.ts.
import * as ipc from "./ipc";
import { orderEmployees } from "./order";
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
    CliKind,
    Employee,
    FloorDefaults,
    FloorEntry,
    FloorView,
    ModelCatalog,
    ModelKind,
    ProgramName,
    Programs,
    SessionView,
    Settings,
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
    settings = $state.raw<Settings | null>(null);
    /** The model lists (docs/ipc-settings.md). Null until the first `models_get` answers. */
    models = $state.raw<ModelCatalog | null>(null);
    /** The settings panel is open. */
    settingsOpen = $state(false);
    /** Logo of each floor as a `data:` URL, by floor path. A floor without a logo has no entry. */
    logos = $state.raw<Record<string, string>>({});
    /** The employee that the operator selected, by floor path. */
    selectedByPath = $state.raw<Record<string, string>>({});
    /** Text of a rejected `logo_generate` call (`NO_KEY:` or `BUSY:`), by floor path. */
    logoCallError = $state.raw<Record<string, string>>({});
    /** Set when `floor_open` failed with `NO_FLOOR:`. The UI offers to create the floor. */
    pendingCreate = $state.raw<{ path: string; message: string } | null>(null);
    backend = $state<"tauri" | "mock" | "">("");
    ready = $state(false);
    private busy = $state.raw<Record<string, true>>({});

    /** The size of the last terminal that fitted. New sessions start with it. */
    lastSize = { rows: 30, cols: 110 };

    private toastId = 0;
    private unlisten: ipc.Unlisten[] = [];
    /** Version of the logo that was loaded (or asked for) for each floor. */
    private logoLoaded = new Map<string, number | null>();

    /** Tabs of the open floor, in a stable order. */
    tabs = $derived.by(() => {
        const path = this.openPath;
        return tabSessions(this.sessionState)
            .filter((s) => s.path === path)
            .sort((a, b) => this.employeeIndex(a.slug) - this.employeeIndex(b.slug));
    });

    /** The employees of the open floor in the order of the hierarchy. Cards and tabs use it. */
    orderedEmployees = $derived.by(() => {
        const view = this.floorView;
        return view ? orderEmployees(view.employees) : [];
    });

    /** Sessions that need a terminal instance: every tab of every floor. */
    allTabSessions = $derived(tabSessions(this.sessionState));

    activeId = $derived.by(() => {
        const path = this.openPath;
        if (!path) return null;
        return pickActive(this.tabs, this.activeByPath[path] ?? null);
    });

    activeSession = $derived(this.activeId ? (this.sessionState.byId[this.activeId] ?? null) : null);

    /** The selected employee of the open floor, if it still exists. */
    selectedEmployee = $derived.by(() => {
        const path = this.openPath;
        const slug = path ? this.selectedByPath[path] : undefined;
        return slug ? (this.orderedEmployees.find((e) => e.slug === slug) ?? null) : null;
    });

    private employeeIndex(slug: string): number {
        const i = this.orderedEmployees.findIndex((e) => e.slug === slug);
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
                    // The list carries the logo version of every floor.
                    void this.loadFloors();
                    if (payload.path === this.openPath) void this.reloadFloor();
                }),
            );
        } catch (e) {
            this.toast(`Cannot listen to events: ${ipc.errorText(e)}`);
        }
        await this.loadFloors();
        void this.loadPrograms();
        void this.loadSettings();
        void this.loadModels();
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
            this.syncLogos();
        } catch (e) {
            this.fail(e);
        }
    }

    private applyView(view: FloorView): void {
        this.floorView = view;
        this.sessionState = applyFloorView(this.sessionState, view);
        this.syncLogos();
    }

    /** Forget the text of a rejected `logo_generate` call: of one floor, or of all floors. */
    private clearLogoCallError(path?: string): void {
        if (path === undefined) {
            if (Object.keys(this.logoCallError).length > 0) this.logoCallError = {};
        } else if (path in this.logoCallError) {
            const next = { ...this.logoCallError };
            delete next[path];
            this.logoCallError = next;
        }
    }

    /** Change the open floor. The call error of the floor that the operator leaves goes away. */
    private switchTo(view: FloorView): void {
        if (this.openPath !== null && this.openPath !== view.path) this.clearLogoCallError(this.openPath);
        this.openPath = view.path;
    }

    /** Load the logo of each floor again when its version changed. */
    private syncLogos(): void {
        for (const entry of this.floors) {
            if (entry.missing) continue;
            const view = this.floorView && this.floorView.path === entry.path ? this.floorView : null;
            const want = view ? view.logoVersion : entry.logoVersion;
            if (this.logoLoaded.has(entry.path) && this.logoLoaded.get(entry.path) === want) continue;
            this.logoLoaded.set(entry.path, want);
            // The logo changed, so an old call error (for example `NO_KEY:`) is out of date.
            this.clearLogoCallError(entry.path);
            if (want === null) {
                if (entry.path in this.logos) {
                    const next = { ...this.logos };
                    delete next[entry.path];
                    this.logos = next;
                }
                continue;
            }
            void ipc
                .floorLogo(entry.path)
                .then((b64) => {
                    // A newer version can have been asked for while this call ran.
                    if (this.logoLoaded.get(entry.path) !== want) return;
                    const next = { ...this.logos };
                    if (b64) next[entry.path] = `data:image/png;base64,${b64}`;
                    else delete next[entry.path];
                    this.logos = next;
                })
                .catch(() => {
                    // Forget the version, so that the next change tries again.
                    if (this.logoLoaded.get(entry.path) === want) this.logoLoaded.delete(entry.path);
                });
        }
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
            this.switchTo(view);
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
            this.switchTo(view);
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

    // ---- logo and setup ----

    /** Start a logo generation. `prompt` null keeps the stored prompt. */
    async generateLogo(prompt: string | null): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        this.clearLogoCallError(path);
        try {
            await ipc.logoGenerate(path, prompt);
        } catch (e) {
            // `NO_KEY:` and `BUSY:` can come as the error of the call. They show like a `logoError`.
            this.logoCallError = { ...this.logoCallError, [path]: ipc.errorText(e) };
        }
        void this.reloadFloor();
    }

    /** Save the OpenAI key, then try the logo again. Returns true when the key was saved. */
    async saveKeyAndRetry(key: string): Promise<boolean> {
        try {
            this.settings = await ipc.settingsSet(key.trim(), null);
        } catch (e) {
            this.fail(e);
            return false;
        }
        // The key is saved: a `NO_KEY:` error of any floor is out of date.
        this.clearLogoCallError();
        await this.generateLogo(null);
        return true;
    }

    /** Start the setup agent of the open floor. */
    startSetup(cli: CliKind): Promise<void> {
        return this.launch("setup", (path, rows, cols) => ipc.setupStart(path, cli, rows, cols));
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

    /** Select an employee of the open floor. A live session of the employee gets its tab. */
    selectEmployee(slug: string): void {
        const path = this.openPath;
        if (!path) return;
        this.selectedByPath = { ...this.selectedByPath, [path]: slug };
        const session = this.sessionOfEmployee(slug);
        if (session && isLive(session)) this.selectTab(session.sessionId);
    }

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

    private adopt(path: string, session: SessionView): void {
        this.sessionState = upsertSession(this.sessionState, path, session);
        // Show the new tab only when the operator still looks at the same floor.
        if (this.openPath === path) this.selectTab(session.sessionId);
    }

    /** True while a start or a resume of this employee is running. The card disables its buttons. */
    isBusy(slug: string): boolean {
        return this.busy[`${this.openPath}\u0000${slug}`] === true;
    }

    /**
     * Run a start or a resume. On failure the error shows in a toast, and nothing else
     * changes: no tab is made, and the card goes back to its normal state.
     */
    private async launch(slug: string, run: (path: string, rows: number, cols: number) => Promise<SessionView>): Promise<void> {
        const path = this.openPath;
        if (!path) return;
        const key = `${path}\u0000${slug}`;
        if (this.busy[key]) return;
        this.busy = { ...this.busy, [key]: true };
        try {
            const { rows, cols } = this.lastSize;
            this.adopt(path, await run(path, rows, cols));
        } catch (e) {
            this.fail(e);
        } finally {
            const next = { ...this.busy };
            delete next[key];
            this.busy = next;
        }
    }

    startSession(slug: string): Promise<void> {
        return this.launch(slug, (path, rows, cols) => ipc.sessionStart(path, slug, null, rows, cols));
    }

    resumeSession(slug: string): Promise<void> {
        return this.launch(slug, (path, rows, cols) => ipc.sessionResume(path, slug, null, rows, cols));
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

    async loadSettings(): Promise<void> {
        try {
            this.settings = await ipc.settingsGet();
        } catch (e) {
            this.fail(e);
        }
    }

    // ---- settings panel, model lists, floor defaults (docs/ipc-settings.md) ----

    /** Read the lists again. The Rust side reads the Codex file on each call. */
    async loadModels(): Promise<void> {
        try {
            this.models = await ipc.modelsGet();
        } catch (e) {
            this.fail(e);
        }
    }

    /** Replace the custom models of one kind. Returns true on success. */
    async setCustomModels(kind: ModelKind, names: string[]): Promise<boolean> {
        try {
            this.models = await ipc.modelsSetCustom(kind, names);
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    /** Store the image model of the logo. Returns true on success. */
    async setImageModel(model: string): Promise<boolean> {
        try {
            this.settings = await ipc.settingsSet(null, model);
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    /** Store the OpenAI key in the app config. An empty text removes it. Returns true on success. */
    async setOpenaiKey(key: string): Promise<boolean> {
        try {
            this.settings = await ipc.settingsSet(key.trim(), null);
            // A stored key makes an old `NO_KEY:` error out of date.
            if (key.trim() !== "") this.clearLogoCallError();
            return true;
        } catch (e) {
            this.fail(e);
            return false;
        }
    }

    /** Write the floor defaults of the open floor. Returns true on success. */
    async setFloorDefaults(defaults: FloorDefaults): Promise<boolean> {
        const path = this.openPath;
        if (!path) return false;
        try {
            this.applyView(await ipc.floorDefaultsSet(path, defaults));
            return true;
        } catch (e) {
            this.fail(e);
            return false;
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
