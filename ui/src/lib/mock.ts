// In-memory mock of the Rust side, for `pnpm dev` in a normal browser and for the e2e check.
// It follows docs/ipc-m1.md, docs/ipc-visuals-init.md, docs/ipc-m2.md and docs/ipc-m3.md: same
// command names, same argument names, same error texts.
import { base64ToBytes, bytesToBase64, concatBytes } from "./codec";
import type { Backend, Unlisten } from "./ipc";
import { STANDARD_EFFORTS, isValidEffort, isValidModelName } from "./models";
import { ALLOW_RULES_MAX, checkAllowRule, ruleWarning } from "./permissions";
import type {
    AttachResult,
    Employee,
    FloorDefaults,
    FloorEntry,
    FloorView,
    ModelCatalog,
    ModelKind,
    ModelOption,
    OutputMessage,
    PermissionsSaved,
    ProgramName,
    Programs,
    ResumeProgress,
    SessionView,
    Settings,
    Status,
    TaskDetail,
    TaskDiff,
    TaskEvent,
    TaskMerge,
    TaskStatus,
    TaskView,
} from "./types";
import { SETUP_SLUG } from "./types";

interface MockFloor {
    path: string;
    name: string;
    employees: Employee[];
    sessions: SessionView[];
    description: string | null;
    logoPrompt: string | null;
    /** Base64 PNG of the logo, null when the floor has no logo. */
    logo: string | null;
    logoVersion: number | null;
    logoBusy: boolean;
    logoError: string | null;
    defaults: FloorDefaults;
    tasks: MockTask[];
    /** The rules that the app stores for the floor (its own config, not the project). */
    permissions: string[];
    /** The app config has an entry for the floor (also an empty list): the app offers no proposal then. */
    permissionsStored: boolean;
    /** The rules of an old `[permissions]` table in `floor.toml` that pass the check (`MockControl.setProposed`). */
    proposed: string[];
    /** `FloorView.resumable`: the employees of the last team that can resume. */
    resumable: string[];
    resumeProgress: ResumeProgress | null;
    /** When set, `task_merge` is blocked with this reason (a conflict, another branch). */
    mergeBlock: string | null;
    /** `FloorView.employeeErrors`: employee files that did not load. */
    employeeErrors: string[];
    /** `FloorView.gone`: the folder of the floor is gone (`MockControl.goneFloor`). */
    gone: boolean;
}

interface MockTask {
    view: TaskView;
    events: TaskEvent[];
    /** The text of the report file (`task_report`), when the task has one. */
    reportText: string | null;
}

interface MockSession {
    view: SessionView;
    path: string;
    log: Uint8Array;
    channel: { onmessage: (m: OutputMessage) => void } | null;
    line: number[];
    timer: ReturnType<typeof setTimeout> | null;
    /** Work that waits for the end of the status `starting`. Like the real CLI, a new session is silent until it is ready. */
    onReady: (() => void)[];
}

/**
 * Settings of the mock, read from `globalThis.__towerMockConfig` each time a session starts.
 * The e2e check sets them to see the loading states.
 */
export interface MockConfig {
    /** How long a new session stays in `starting`, in ms. Default 1500. A very large value holds it. */
    startingMs?: number;
    /** How long `session_start`, `session_resume` and `setup_start` take before they return, in ms. Default 0. */
    startCallMs?: number;
    /** The pause between two starts of `team_resume`, in ms. Default 400 (the real app waits 2 s). */
    resumePauseMs?: number;
    /** How long `program_test` takes, in ms. Default 0. */
    programTestMs?: number;
    /** Text that the CLI has already printed when a session is made (it shows in the scrollback of the attach). */
    startingText?: string;
    /** `programs_get` finds the optional program `rtk` (the examples of the rules in Settings then use it). Default false. */
    rtk?: boolean;
}

/** `globalThis.__towerMock`: a hand to the e2e check, to act on a session that is `starting`. */
export interface MockControl {
    /** Send text on the output channel of the session (also during `starting`). */
    output(sessionId: string, text: string): void;
    /** End the status `starting` at once. */
    leaveStarting(sessionId: string): void;
    /**
     * An employee starts another employee, as the tool `employee_start` does in the real app:
     * the session starts, `session-changed` follows, then `employee-started`. Returns the new
     * session ID.
     */
    employeeStart(path: string, bySlug: string, slug: string): string;
    /**
     * An employee changes a task, as the tools `task_create`, `task_assign` and `task_update` do in
     * the real app. `tasks-changed` follows. `create` makes a task (the creator is `bySlug`).
     */
    taskByEmployee(
        path: string,
        bySlug: string,
        change:
            | { create: string; description?: string }
            | { id: number; assign: string }
            | { id: number; status: TaskStatus; note?: string; report?: string },
    ): number;
    /** Give a task a git branch and a worktree, as `task_assign` does for an employee that edits files. */
    taskWorktree(path: string, id: number): void;
    /** The next `task_merge` calls of this floor are blocked with this reason. `null` removes the block. */
    blockMerge(path: string, reason: string | null): void;
    /** The employees of the last team (`FloorView.resumable`). A floor event follows. */
    setResumable(path: string, slugs: string[]): void;
    /** The next `team_resume` start of this employee fails (a notice follows). */
    failResume(slug: string): void;
    /** Stop the automatic status changes of all sessions (`true`), or let them run (`false`). A check that needs a fixed status uses it. */
    freeze(on: boolean): void;
    /** Set the status of a session now, like the real CLI does. `session-changed` follows. */
    setStatus(sessionId: string, status: Status): void;
    /** Start the session of an employee with no toast, as `session_start` does. Returns the session ID. */
    startSession(path: string, slug: string): string;
    /** Add `count` employees (engineers of the Project Manager) to a floor, with no model. A floor event follows. */
    addEmployees(path: string, count: number): void;
    /**
     * The app delivers a message from one employee to another, as the real app does by a paste, the Stop hook,
     * `inbox` or a first message: the event `message-delivered` follows. `text` is the body of the message (the
     * mock makes the preview: 80 characters, one line). For a message about a task give `task`: with `status`
     * it is a status message ("T-3 is now in review"), with no `status` it is an assignment. `fromSlug` can be
     * "operator".
     */
    deliverMessage(path: string, fromSlug: string, toSlug: string, what: string | { task: number; status?: string }): void;
    /** Send the `notice` event. `kind` is `task_blocked` (a blocked merge, an error toast) when it is left out. */
    notice(path: string, text: string, kind?: string): void;
    /**
     * The folder of the floor is gone, as the app finds it: `gone` is true, then `floor-changed`,
     * then the notice `floor_gone` (the text of docs/ipc-final.md). Commands on the floor give `FLOOR_GONE:`.
     */
    goneFloor(path: string): void;
    /** The folder is back: `gone` is false, then `floor-changed`, then the notice `floor_back`. */
    backFloor(path: string): void;
    /**
     * Put an old `[permissions]` table in the `floor.toml` of the floor, as an agent can write it. The rules show
     * in `floor_permissions_proposed` until a `floor_permissions_set` stores a list. Each rule must pass the check.
     */
    setProposed(path: string, rules: string[]): void;
    /** The employee files that did not load (`FloorView.employeeErrors`). A floor event follows. */
    setEmployeeErrors(path: string, errors: string[]): void;
    /** Set the `[brand]` description of a floor (`null` removes it). A floor event follows. */
    setDescription(path: string, text: string | null): void;
    /** Change the defaults or one employee of a floor. A floor event follows. */
    patchFloor(path: string, change: { defaults?: Partial<FloorDefaults>; employee?: { slug: string; patch: Partial<Employee> } }): void;
}

const config = (): MockConfig => ((globalThis as Record<string, unknown>).__towerMockConfig ?? {}) as MockConfig;
const startingMs = (): number => config().startingMs ?? 1500;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const enc = new TextEncoder();
const dec = new TextDecoder();
const LOG_MAX = 256 * 1024;
const NEXT: Record<string, [Status, number]> = {
    idle: ["working", 6000],
    working: ["question", 4000],
    question: ["idle", 8000],
    done: ["idle", 6000],
};

// A 1 x 1 transparent PNG, for a place without a canvas (a test in Node).
const TINY_PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/** A small sticker-like PNG (base64) with one letter. It stands for the image of the OpenAI API. */
function makeLogo(letter: string, hue: number): string {
    if (typeof document === "undefined") return TINY_PNG;
    const canvas = document.createElement("canvas");
    canvas.width = 256;
    canvas.height = 256;
    const ctx = canvas.getContext("2d");
    if (!ctx) return TINY_PNG;
    ctx.fillStyle = "#ffffff";
    ctx.beginPath();
    ctx.roundRect(8, 8, 240, 240, 56);
    ctx.fill();
    ctx.fillStyle = `hsl(${hue} 65% 45%)`;
    ctx.beginPath();
    ctx.roundRect(28, 28, 200, 200, 44);
    ctx.fill();
    ctx.fillStyle = "#ffffff";
    ctx.font = "bold 130px sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(letter, 128, 138);
    return canvas.toDataURL("image/png").replace(/^data:image\/png;base64,/, "");
}

function employee(slug: string, name: string, role: string, manager: string | null, cli: "claude" | "codex"): Employee {
    return {
        slug,
        name,
        role,
        manager,
        cli,
        model: cli === "claude" ? "haiku" : "gpt-6-luna",
        effort: null,
        policy: "read_only",
        roleText: `You are ${name}, the ${role}.`,
    };
}

const noDefaults = (): FloorDefaults => ({ cli: null, model: null, effort: null, policy: null });

function modelOption(id: string, label: string, efforts: readonly string[], defaultEffort: string | null): ModelOption {
    return { id, label, efforts: [...efforts], defaultEffort, custom: false };
}

/** The lists that the mock starts with. They look like the real ones (the Codex file has "ultra"). */
const CLAUDE_MODELS = [
    modelOption("fable", "Fable", STANDARD_EFFORTS, null),
    modelOption("opus", "Opus", STANDARD_EFFORTS, null),
    modelOption("sonnet", "Sonnet", STANDARD_EFFORTS, null),
    modelOption("haiku", "Haiku", STANDARD_EFFORTS, null),
];
const CODEX_MODELS = [
    modelOption("gpt-6.2-sol", "GPT-6.2-Sol", [...STANDARD_EFFORTS, "ultra"], "low"),
    modelOption("gpt-6-astra", "GPT-6-Astra", [...STANDARD_EFFORTS, "ultra"], "medium"),
    modelOption("gpt-6-luna", "GPT-6-Luna", STANDARD_EFFORTS, "medium"),
    modelOption("gpt-5.6-luna", "GPT-5.6-Luna", STANDARD_EFFORTS, "medium"),
];
const IMAGE_MODELS = [
    modelOption("gpt-image-2.6-sunburst", "gpt-image-2.6-sunburst", [], null),
    modelOption("gpt-image-2.6-flare", "gpt-image-2.6-flare", [], null),
];

/** Paths that the mock turns into other paths, like a symlink does. */
const ALIASES: Record<string, string> = { "/home/demo/link": "/home/demo/tower" };

/**
 * Like the Rust side: `FloorView.path` is a canonical path. A typed `~/x`, `a/../b`, a trailing
 * slash or a symlink gives another text than the one that was typed.
 */
export function canonicalPath(typed: string): string {
    let p = typed.trim();
    if (p === "~" || p.startsWith("~/")) p = "/home/demo" + p.slice(1);
    const parts: string[] = [];
    for (const segment of p.split("/")) {
        if (segment === "" || segment === ".") continue;
        if (segment === "..") parts.pop();
        else parts.push(segment);
    }
    const out = "/" + parts.join("/");
    return ALIASES[out] ?? out;
}

export function createMockBackend(): Backend {
    const floors = new Map<string, MockFloor>();
    const known: string[] = [];
    const sessions = new Map<string, MockSession>();
    const listeners = new Map<string, Set<(payload: unknown) => void>>();
    // What the app finds when there is no override (the "reset" button goes back to this).
    const FOUND: Record<ProgramName, string | null> = { claude: "/usr/local/bin/claude", codex: null, node: "/usr/bin/node" };
    const VERSIONS: Record<ProgramName, string> = { claude: "2.1.330 (Claude Code)", codex: "codex-cli 0.9.1", node: "v24.10.0" };
    const programs: Programs = {
        ...FOUND,
        hook: "/opt/agent-tower/bin/tower-hook",
        rtk: null,
        overrides: { claude: null, codex: null, node: null },
    };
    let counter = 0;
    // Errors that the mock gives one time, to show what the UI does when a start fails.
    const failOnce = new Set(["start:eng-sam", "resume:pm"]);
    // Employees whose start in `team_resume` fails one time (`MockControl.failResume`).
    const failResume = new Set<string>();
    // `MockControl.freeze`: no automatic status change while this is true.
    let frozen = false;

    const settings: Settings = {
        openaiKeySet: false,
        openaiKeySource: null,
        imageModel: "gpt-image-2.6-sunburst",
        notifyOnQuestion: true,
        homeDir: "/home/demo",
    };
    // The custom models of the app config (`[models]`).
    const custom: Record<ModelKind, string[]> = { claude: [], codex: [], image: [] };

    function catalog(): ModelCatalog {
        const build = (base: ModelOption[], kind: ModelKind): ModelOption[] => {
            const list = base.map((m) => ({ ...m, efforts: [...m.efforts] }));
            for (const id of custom[kind]) {
                if (list.some((m) => m.id === id)) continue;
                list.push({
                    id,
                    label: id,
                    efforts: kind === "image" ? [] : [...STANDARD_EFFORTS],
                    defaultEffort: null,
                    custom: true,
                });
            }
            return list;
        };
        return {
            claude: build(CLAUDE_MODELS, "claude"),
            codex: build(CODEX_MODELS, "codex"),
            image: build(IMAGE_MODELS, "image"),
            codexSource: "cache",
        };
    }

    /** Like the Rust side: a model or an effort that is not valid is refused. */
    function checkModelAndEffort(what: string, model: string | null, effort: string | null): void {
        if (model && !isValidModelName(model)) throw `the ${what} model ${JSON.stringify(model)} is not a valid model name.`;
        if (effort && !isValidEffort(effort)) throw `the ${what} effort ${JSON.stringify(effort)} is not a valid effort.`;
    }
    const LOGO_MS = 1500;

    function addFloor(path: string, name: string, employees: Employee[]): MockFloor {
        const floor: MockFloor = {
            path,
            name,
            employees,
            sessions: [],
            description: null,
            logoPrompt: null,
            logo: null,
            logoVersion: null,
            logoBusy: false,
            logoError: null,
            defaults: noDefaults(),
            tasks: [],
            permissions: [],
            permissionsStored: false,
            proposed: [],
            resumable: [],
            resumeProgress: null,
            mergeBlock: null,
            employeeErrors: [],
            gone: false,
        };
        floors.set(path, floor);
        if (!known.includes(path)) known.push(path);
        return floor;
    }

    function seedTask(
        floor: MockFloor,
        id: number,
        title: string,
        description: string,
        status: TaskStatus,
        creator: string,
        assignee: string | null,
        assigner: string | null,
        extra: { report?: string; reportText?: string } = {},
    ): void {
        const at = 1_700_000_000_000 + id * 60_000;
        const events: TaskEvent[] = [{ id: 1, at, author: creator, kind: "created", value: null, text: null }];
        if (assignee) events.push({ id: 2, at: at + 1000, author: assigner ?? creator, kind: "assigned", value: assignee, text: null });
        if (status !== "todo") {
            events.push({ id: events.length + 1, at: at + 2000, author: assignee ?? creator, kind: "status", value: status, text: null });
        }
        floor.tasks.push({
            view: {
                id,
                title,
                description,
                parent: null,
                creator,
                assignee,
                assigner,
                status,
                report: extra.report ?? null,
                createdAt: at,
                updatedAt: at + 2000,
                branch: null,
                baseBranch: null,
                baseCommit: null,
                worktree: null,
            },
            events,
            reportText: extra.reportText ?? null,
        });
    }

    const tower = addFloor("/home/demo/tower", "Demo Tower", [
        employee("ceo", "Ada", "CEO", null, "claude"),
        employee("pm", "Maya", "Project Manager", "ceo", "claude"),
        employee("eng-sam", "Sam", "Engineer", "pm", "codex"),
        employee("eng-lin", "Lin", "Engineer", "pm", "claude"),
    ]);
    // The first floor has a logo, a description and a logo prompt.
    tower.description = "A demo project with a CEO, a project manager and two engineers.";
    tower.logoPrompt = "A tall tower with a golden crown.";
    tower.logo = makeLogo("D", 165);
    tower.logoVersion = 1_700_000_000_000;
    // The task board has one task in each group.
    seedTask(tower, 1, "Add a hello page", "Make a page that says hello.", "done", "operator", "eng-lin", "pm", {
        report: ".swarm/reports/T-1.md",
        reportText: "# T-1 report\n\nWhat I did: added hello.html.\nWhat I verified: opened it.\nWhat I did not do: styling.\nOpen questions: none.\n",
    });
    seedTask(tower, 2, "Write the login form", "A form with a name and a password.", "in_progress", "pm", "eng-lin", "pm");
    seedTask(tower, 3, "Check the build on Linux", "Run the build and report.", "review", "pm", "eng-sam", "pm", {
        report: ".swarm/reports/T-3.md",
        reportText: "# T-3 report\n\nThe build passes.\n",
    });
    // T-2 and T-3 have a branch and a worktree, like a task of an employee that edits files.
    giveWorktree(tower.tasks[1], tower.tasks[1].view.updatedAt);
    giveWorktree(tower.tasks[2], tower.tasks[2].view.updatedAt);
    seedTask(tower, 4, "Pick the colours", "Wait for the logo.", "blocked", "ceo", "pm", "ceo");
    seedTask(tower, 5, "Plan the release", "List the steps.", "todo", "operator", null, null);
    // Lin has a stopped session from "an earlier run" that can resume.
    tower.sessions.push({
        sessionId: "s-old-lin",
        slug: "eng-lin",
        status: "stopped",
        draft: false,
        messageWaiting: false,
        canResume: true,
    });
    // The second floor has no logo: the building shows the placeholder sticker.
    addFloor("/home/demo/second", "Second Floor", [employee("ceo", "Bo", "CEO", null, "claude")]);
    // A big office, to check the layout with 13 desks.
    const big = addFloor("/home/demo/big", "Big Office", [
        employee("ceo", "Eve", "CEO", null, "claude"),
        employee("pm", "Pia", "Project Manager", "ceo", "claude"),
        ...[
            ["eng-1", "Kai", "Engineer"],
            ["eng-2", "Leo", "Engineer"],
            ["eng-3", "Mo", "Engineer"],
            ["eng-4", "Nia", "Engineer"],
            ["des-1", "Noor", "Designer"],
            ["des-2", "Wolfeschlegel", "Designer"],
            ["qa-1", "Sam", "Tester"],
            ["qa-2", "Rhea", "Tester"],
            ["doc-1", "Bartholomew", "Writer"],
            ["ops-1", "Ola", "Support"],
            ["ops-2", "Jun", "Support"],
        ].map(([slug, name, role]) => employee(slug, name, role, "pm", slug.length % 2 === 0 ? "claude" : "codex")),
    ]);
    big.description = "An office with thirteen desks.";
    big.logoPrompt = "A busy beehive.";
    big.logo = makeLogo("B", 35);
    big.logoVersion = 1_700_000_100_000;
    // A path in the list that does not exist any more.
    known.push("/home/demo/gone");

    // ---- tasks (docs/ipc-m2.md) ----

    const TASK_STATUSES: TaskStatus[] = ["todo", "in_progress", "blocked", "review", "done"];

    function taskOf(floor: MockFloor, id: number): MockTask {
        const t = floor.tasks.find((x) => x.view.id === id);
        if (!t) throw `there is no task T-${id}`;
        return t;
    }

    function addEvent(t: MockTask, author: string, kind: TaskEvent["kind"], value: string | null, text: string | null): void {
        const at = Date.now();
        t.events.push({ id: t.events.length + 1, at, author, kind, value, text });
        t.view.updatedAt = at;
    }

    /** The rules of `task_create`: a one-line title of 1 to 200 characters, a description of 20000 at most. */
    function makeTask(floor: MockFloor, creator: string, title: string, description: string): MockTask {
        const line = title.replace(/[\n\t\r]/g, " ").trim();
        if (line === "") throw "The title is empty.";
        if ([...title].length > 200) throw `The title is too long: ${[...title].length} characters. The limit is 200.`;
        if ([...description].length > 20_000) {
            throw `The description is too long: ${[...description].length} characters. The limit is 20000. Write a shorter text, or put the details in a file.`;
        }
        const id = floor.tasks.reduce((m, t) => Math.max(m, t.view.id), 0) + 1;
        const now = Date.now();
        const t: MockTask = {
            view: {
                id,
                title: line,
                description,
                parent: null,
                creator,
                assignee: null,
                assigner: null,
                status: "todo",
                report: null,
                createdAt: now,
                updatedAt: now,
                branch: null,
                baseBranch: null,
                baseCommit: null,
                worktree: null,
            },
            events: [],
            reportText: null,
        };
        floor.tasks.push(t);
        addEvent(t, creator, "created", null, null);
        return t;
    }

    /** The branch name of the real app: `task/T-12-add-login` (see `branch_slug` in gitwork.rs). */
    function branchName(t: MockTask): string {
        const slug = t.view.title
            .toLowerCase()
            .replace(/[^a-z0-9]+/g, "-")
            .replace(/^-+|-+$/g, "")
            .slice(0, 40)
            .replace(/-+$/, "");
        return `task/T-${t.view.id}-${slug === "" ? "task" : slug}`;
    }

    /** A task gets its branch and worktree when it is given to an employee that edits files. */
    function giveWorktree(t: MockTask, at?: number): void {
        t.view.branch = branchName(t);
        t.view.baseBranch = "main";
        t.view.baseCommit = "0123456789abcdef0123456789abcdef01234567";
        t.view.worktree = `.swarm/worktrees/T-${t.view.id}`;
        const note = `Branch ${t.view.branch} from main at 0123456789ab.`;
        if (at === undefined) {
            addEvent(t, t.view.assigner ?? "operator", "worktree", t.view.branch, note);
        } else {
            t.events.push({ id: t.events.length + 1, at, author: t.view.assigner ?? "operator", kind: "worktree", value: t.view.branch, text: note });
        }
    }

    function assignTask(t: MockTask, by: string, slug: string): void {
        t.view.assignee = slug;
        t.view.assigner = by;
        addEvent(t, by, "assigned", slug, null);
    }

    /** A status change is an event of the kind `status`. The same status with a note is a `comment`. */
    function updateTask(t: MockTask, by: string, status: TaskStatus, note: string | null, report: string | null): void {
        const text = note !== null && note.trim() !== "" ? note : null;
        if (status !== t.view.status) {
            t.view.status = status;
            addEvent(t, by, "status", status, text);
        } else if (text !== null) {
            addEvent(t, by, "comment", null, text);
        }
        if (report !== null && report !== t.view.report) {
            t.view.report = report;
            t.reportText = `# ${report}\n\nReport text of the mock.\n`;
            addEvent(t, by, "report", report, null);
        }
        t.view.updatedAt = Date.now();
    }

    // ---- helpers ----

    // Events wait in one queue and go out in the order of `emit`, each with a copy of its payload
    // from the moment of the call. So a later change of a session does not change an event that
    // waits, and two events of the same moment keep their order (as the events of the real app do).
    const outbox: Array<() => void> = [];
    let draining = false;

    function emit(event: string, payload: unknown): void {
        const copy = payload === undefined ? payload : structuredClone(payload);
        for (const handler of listeners.get(event) ?? []) outbox.push(() => handler(copy));
        if (draining) return;
        draining = true;
        setTimeout(() => {
            draining = false;
            const batch = outbox.splice(0);
            for (const send of batch) {
                try {
                    send();
                } catch (error) {
                    console.error(error);
                }
            }
        }, 0);
    }

    function viewOf(floor: MockFloor): FloorView {
        const setup = floor.sessions.find((x) => x.slug === SETUP_SLUG && x.status !== "stopped");
        return {
            path: floor.path,
            name: floor.name,
            employees: structuredClone(floor.employees),
            employeeErrors: [...floor.employeeErrors],
            sessions: structuredClone(floor.sessions),
            description: floor.description,
            logoPrompt: floor.logoPrompt,
            logoVersion: floor.logoVersion,
            logoBusy: floor.logoBusy,
            logoError: floor.logoError,
            setupSessionId: setup ? setup.sessionId : null,
            defaults: { ...floor.defaults },
            resumable: floor.resumeProgress ? [] : [...floor.resumable],
            resumeProgress: floor.resumeProgress ? { ...floor.resumeProgress } : null,
            gone: floor.gone,
        };
    }

    /** Paths of floors that were gone when the operator forgot them. The app does not hold them: `NOT_FOUND:`. */
    const forgottenGone = new Set<string>();

    /**
     * The path check of `floor_open` and `floor_create` (docs/ipc-final.md). `~` is expanded first.
     * The mock knows four simple cases: a relative path, a path under `/missing/`, a path that ends in
     * `.txt` (a file) and a path under `/readonly/`. Returns the path after the expansion of `~`.
     */
    function checkPath(typed: string): string {
        let p = typed.trim();
        if (p === "~" || p.startsWith("~/")) p = "/home/demo" + p.slice(1);
        if (!p.startsWith("/")) throw "NOT_ABSOLUTE: Type the full path of the folder, for example /home/you/project.";
        if (p === "/missing" || p.startsWith("/missing/") || forgottenGone.has(canonicalPath(p))) {
            throw `NOT_FOUND: The folder ${p} does not exist.`;
        }
        if (p.endsWith(".txt")) throw `NOT_A_FOLDER: ${p} is a file, not a folder.`;
        if (p === "/readonly" || p.startsWith("/readonly/")) throw `NO_WRITE: You cannot write in the folder ${p}.`;
        return p;
    }

    /** Commands that still work on a gone floor (docs/ipc-final.md). `session_stop` has no path. */
    const WORKS_ON_GONE = new Set(["floor_list", "floor_open", "floor_create", "floor_forget", "session_stop"]);

    function floorOf(path: string): MockFloor {
        const floor = floors.get(path);
        if (!floor) throw `no floor at ${path}`;
        return floor;
    }

    function sessionById(sessionId: string): MockSession {
        const s = sessions.get(sessionId);
        if (!s) throw `unknown session ${sessionId}`;
        return s;
    }

    function syncFloorSession(s: MockSession): void {
        const floor = floors.get(s.path);
        if (!floor) return;
        const i = floor.sessions.findIndex((x) => x.slug === s.view.slug);
        const copy = { ...s.view };
        if (i >= 0) floor.sessions[i] = copy;
        else floor.sessions.push(copy);
    }

    function changed(s: MockSession): void {
        syncFloorSession(s);
        emit("session-changed", { path: s.path, session: { ...s.view } });
    }

    function setStatus(s: MockSession, status: Status): void {
        if (s.view.status === status) return;
        const wasStarting = s.view.status === "starting";
        s.view.status = status;
        if (wasStarting && status !== "stopped") {
            // The CLI is ready: its first output shows now.
            const work = s.onReady;
            s.onReady = [];
            for (const fn of work) fn();
        }
        // The setup session cannot resume.
        if (status === "stopped" && s.view.slug !== SETUP_SLUG) s.view.canResume = true;
        changed(s);
        if (status === "stopped" && s.view.slug === SETUP_SLUG) emit("floor-changed", { path: s.path });
        schedule(s);
    }

    function schedule(s: MockSession): void {
        if (s.timer) clearTimeout(s.timer);
        s.timer = null;
        if (frozen) return;
        // The setup session goes to `working` and then waits for its own timer (see `startSetup`).
        const next: [Status, number] | undefined =
            s.view.status === "starting"
                ? [s.view.slug === SETUP_SLUG ? "working" : "idle", startingMs()]
                : s.view.slug === SETUP_SLUG
                  ? undefined
                  : NEXT[s.view.status];
        if (!next) return;
        s.timer = setTimeout(() => setStatus(s, next[0]), next[1]);
    }

    function output(s: MockSession, bytes: Uint8Array): void {
        s.log = concatBytes([s.log, bytes]);
        if (s.log.length > LOG_MAX) s.log = s.log.slice(s.log.length - LOG_MAX);
        s.channel?.onmessage({ data: bytesToBase64(bytes) });
    }

    const text = (s: MockSession, value: string) => output(s, enc.encode(value));
    const showPrompt = (s: MockSession) => text(s, "\x1b[32m$\x1b[0m ");

    function banner(s: MockSession, emp: Employee): void {
        text(
            s,
            `\x1b[1;36mAgent Tower mock terminal\x1b[0m for \x1b[1m${emp.name}\x1b[0m (${emp.role}, ${emp.cli})\r\n` +
                `\x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m \x1b[33myellow\x1b[0m \x1b[34mblue\x1b[0m ` +
                `\x1b[35mmagenta\x1b[0m \x1b[36mcyan\x1b[0m \x1b[7m reverse \x1b[0m \x1b[4munderline\x1b[0m\r\n` +
                `Type something. It is echoed back when you press Enter.\r\n`,
        );
        showPrompt(s);
    }

    function startSession(path: string, slug: string): SessionView {
        const floor = floorOf(path);
        const emp = floor.employees.find((e) => e.slug === slug);
        if (!emp) throw `unknown employee ${slug}`;
        const old = floor.sessions.find((x) => x.slug === slug);
        if (old && old.status !== "stopped") throw `employee ${slug} already has a live session`;
        counter += 1;
        const s: MockSession = {
            view: {
                sessionId: `s-${counter}`,
                slug,
                status: "starting",
                draft: false,
                messageWaiting: false,
                canResume: false,
            },
            path,
            log: new Uint8Array(0),
            channel: null,
            line: [],
            timer: null,
            onReady: [],
        };
        sessions.set(s.view.sessionId, s);
        if (config().startingText) text(s, config().startingText as string);
        s.onReady.push(() => banner(s, emp));
        changed(s);
        schedule(s);
        return { ...s.view };
    }

    /** The setup agent: prints a few lines, then adds one employee to the floor and ends its turn. */
    function startSetup(path: string, cli: string): SessionView {
        const floor = floorOf(path);
        if (cli !== "claude" && cli !== "codex") throw `unknown cli ${cli}`;
        const old = floor.sessions.find((x) => x.slug === SETUP_SLUG);
        if (old && old.status !== "stopped") throw "a setup session is already live for this floor";
        counter += 1;
        const s: MockSession = {
            view: {
                sessionId: `s-${counter}`,
                slug: SETUP_SLUG,
                status: "starting",
                draft: false,
                messageWaiting: false,
                canResume: false,
            },
            path,
            log: new Uint8Array(0),
            channel: null,
            line: [],
            timer: null,
            onReady: [],
        };
        sessions.set(s.view.sessionId, s);
        if (config().startingText) text(s, config().startingText as string);
        // Silent while `starting`. When the CLI is ready it prints, works for 2.3 s, and ends its turn.
        s.onReady.push(() => {
            text(s, `\x1b[1;36mAgent Tower mock setup agent\x1b[0m (${cli}) in \x1b[1m${floor.name}\x1b[0m\r\n`);
            text(s, "Reading CLAUDE.md, README.md and the top-level layout...\r\n");
            setTimeout(() => {
                if (s.view.status === "stopped") return;
                text(s, "I propose one ceo, one pm and one tester.\r\n");
                text(s, "Writing .swarm/employees/qa.md and the [brand] table...\r\n");
                if (!floor.employees.some((e) => e.slug === "qa")) {
                    floor.employees.push(employee("qa", "Quinn", "Tester", "pm", cli));
                }
                floor.description ??= `The project in ${floor.name}.`;
                floor.logoPrompt ??= `A friendly emblem for ${floor.name}.`;
                text(s, "\x1b[32mThe setup is complete. You can stop this session.\x1b[0m\r\n");
                showPrompt(s);
                emit("floor-changed", { path });
                setStatus(s, "idle");
            }, 2300);
        });
        changed(s);
        schedule(s);
        return { ...s.view };
    }

    // ---- operator input: an echo terminal ----

    function echoInput(s: MockSession, bytes: Uint8Array): void {
        const out: number[] = [];
        let typedPrintable = false;
        for (const b of bytes) {
            if (b === 0x0d) {
                const line = dec.decode(new Uint8Array(s.line));
                s.line = [];
                out.push(...enc.encode(`\r\n\x1b[33myou typed:\x1b[0m ${JSON.stringify(line)}\r\n\x1b[32m$\x1b[0m `));
                if (s.view.draft) {
                    s.view.draft = false;
                    changed(s);
                }
                if (s.view.status === "question") setStatus(s, "working");
            } else if (b === 0x7f) {
                if (s.line.length > 0) {
                    s.line.pop();
                    out.push(0x08, 0x20, 0x08);
                }
            } else if (b < 0x20) {
                // Control byte, for example ESC of an arrow key or a bracketed paste: show as ^X.
                s.line.push(b);
                out.push(...enc.encode(`\x1b[2m^${String.fromCharCode(b + 64)}\x1b[0m`));
            } else {
                s.line.push(b);
                out.push(b);
                typedPrintable = true;
            }
        }
        if (out.length > 0) output(s, new Uint8Array(out));
        const status = s.view.status;
        if (typedPrintable && !s.view.draft && (status === "idle" || status === "working" || status === "done")) {
            s.view.draft = true;
            changed(s);
        }
    }

    // ---- command dispatch ----

    /** `optional` names may be missing, as an `Option` argument of a Tauri command may be. */
    function args<T>(raw: Record<string, unknown> | undefined, names: string[], optional: string[] = []): T {
        const a = raw ?? {};
        for (const name of names) {
            if (!(name in a)) throw `missing argument ${name}`;
        }
        for (const name of Object.keys(a)) {
            if (!names.includes(name) && !optional.includes(name)) throw `unexpected argument ${name}`;
        }
        return a as T;
    }

    async function handle(command: string, raw?: Record<string, unknown>): Promise<unknown> {
        // Each command that reads or writes a gone floor gives the same error and writes nothing.
        const target = typeof raw?.path === "string" ? floors.get(raw.path) : undefined;
        if (target?.gone && !WORKS_ON_GONE.has(command)) {
            throw `FLOOR_GONE: The folder of this floor is gone: ${target.path}.`;
        }
        switch (command) {
            case "floor_list": {
                args(raw, []);
                return known.map<FloorEntry>((path) => {
                    const floor = floors.get(path);
                    return {
                        path,
                        name: floor ? floor.name : null,
                        missing: !floor || floor.gone,
                        logoVersion: floor ? floor.logoVersion : null,
                    };
                });
            }
            case "floor_open": {
                const { path } = args<{ path: string }>(raw, ["path"]);
                const typed = path.trim();
                if (typed === "") throw "the path is empty";
                const expanded = checkPath(typed);
                const clean = canonicalPath(expanded);
                const floor = floors.get(clean);
                if (!floor) throw `NO_FLOOR: ${typed} has no .swarm/ folder`;
                if (!known.includes(clean)) known.push(clean);
                return viewOf(floor);
            }
            case "floor_create": {
                const { path, name } = args<{ path: string; name: string }>(raw, ["path", "name"]);
                const clean = canonicalPath(checkPath(path));
                if (floors.has(clean)) throw `${clean} already has a .swarm/ folder`;
                if (name.trim() === "") throw "the floor name is empty";
                const floor = addFloor(clean, name.trim(), [
                    employee("ceo", "CEO", "CEO", null, "claude"),
                    employee("pm", "PM", "Project Manager", "ceo", "claude"),
                ]);
                return viewOf(floor);
            }
            case "floor_forget": {
                const { path } = args<{ path: string }>(raw, ["path"]);
                // Like the real app: a floor with a live session is not forgotten.
                if ([...sessions.values()].some((s) => s.path === path && s.view.status !== "stopped")) {
                    throw "the floor has live sessions. Stop them before you forget the floor.";
                }
                const i = known.indexOf(path);
                if (i >= 0) known.splice(i, 1);
                // The app does not hold a gone floor that was forgotten: a later open gives `NOT_FOUND:`.
                if (floors.get(path)?.gone) {
                    floors.delete(path);
                    forgottenGone.add(path);
                }
                return null;
            }
            case "employee_save": {
                const { path, employee: emp } = args<{ path: string; employee: Employee }>(raw, ["path", "employee"]);
                const floor = floorOf(path);
                if (emp.slug === SETUP_SLUG) throw `the slug "${SETUP_SLUG}" is reserved`;
                if (!/^[a-z0-9][a-z0-9-]*$/.test(emp.slug)) throw `bad slug "${emp.slug}"`;
                checkModelAndEffort(`employee ${emp.slug}`, emp.model, emp.effort);
                const i = floor.employees.findIndex((e) => e.slug === emp.slug);
                if (i >= 0) floor.employees[i] = structuredClone(emp);
                else floor.employees.push(structuredClone(emp));
                emit("floor-changed", { path });
                return viewOf(floor);
            }
            case "employee_remove": {
                const { path, slug } = args<{ path: string; slug: string }>(raw, ["path", "slug"]);
                const floor = floorOf(path);
                const live = floor.sessions.find((x) => x.slug === slug && x.status !== "stopped");
                if (live) throw `employee ${slug} has a live session`;
                floor.employees = floor.employees.filter((e) => e.slug !== slug);
                floor.sessions = floor.sessions.filter((x) => x.slug !== slug);
                emit("floor-changed", { path });
                return viewOf(floor);
            }
            case "session_start": {
                const a = args<{ path: string; slug: string; firstMessage: string | null; rows: number; cols: number }>(
                    raw,
                    ["path", "slug", "firstMessage", "rows", "cols"],
                );
                await sleep(config().startCallMs ?? 0);
                if (failOnce.delete(`start:${a.slug}`)) {
                    throw `cannot start ${a.slug}: the program "codex" was not found. Set its path in Programs.`;
                }
                return startSession(a.path, a.slug);
            }
            case "session_resume": {
                const a = args<{ path: string; slug: string; message: string | null; rows: number; cols: number }>(
                    raw,
                    ["path", "slug", "message", "rows", "cols"],
                );
                await sleep(config().startCallMs ?? 0);
                const old = floorOf(a.path).sessions.find((x) => x.slug === a.slug);
                if (failOnce.delete(`resume:${a.slug}`)) {
                    throw `cannot resume ${a.slug}: the session never had a turn, so it has nothing to resume. Start it again.`;
                }
                if (!old?.canResume) throw `employee ${a.slug} has no session to resume`;
                return startSession(a.path, a.slug);
            }
            case "session_stop": {
                const { sessionId } = args<{ sessionId: string }>(raw, ["sessionId"]);
                const s = sessionById(sessionId);
                text(s, "\r\n\x1b[2m[process exited]\x1b[0m\r\n");
                setStatus(s, "stopped");
                return null;
            }
            case "session_attach": {
                const { sessionId, channel } = args<{
                    sessionId: string;
                    channel: { onmessage: (m: OutputMessage) => void };
                }>(raw, ["sessionId", "channel"]);
                const s = sessionById(sessionId);
                s.channel = channel; // a new attach replaces the old one
                const result: AttachResult = { scrollback: bytesToBase64(s.log) };
                if (s.view.status !== "stopped") {
                    // Live output after attach, with a multi-byte character split over two chunks.
                    // A session that is still `starting` is silent: this waits for the end of the start.
                    const live = enc.encode("\r\n\x1b[2mchannel online ✓ café\x1b[0m\r\n");
                    const cut = live.indexOf(0xe2) + 1; // inside the 3 bytes of U+2713
                    const send = () =>
                        setTimeout(() => {
                            output(s, live.slice(0, cut));
                            setTimeout(() => {
                                output(s, live.slice(cut));
                                showPrompt(s);
                            }, 20);
                        }, 80);
                    if (s.view.status === "starting") s.onReady.push(send);
                    else send();
                }
                return result;
            }
            case "pty_write": {
                const { sessionId, data } = args<{ sessionId: string; data: string }>(raw, ["sessionId", "data"]);
                const s = sessionById(sessionId);
                if (s.view.status === "stopped") throw "the session has no live process";
                echoInput(s, base64ToBytes(data));
                return null;
            }
            case "pty_resize": {
                const { sessionId } = args<{ sessionId: string; rows: number; cols: number }>(raw, [
                    "sessionId",
                    "rows",
                    "cols",
                ]);
                sessionById(sessionId);
                return null;
            }
            case "clear_draft": {
                const { sessionId } = args<{ sessionId: string }>(raw, ["sessionId"]);
                const s = sessionById(sessionId);
                // Like the Rust side: clears the draft flag and the "message waiting" flag, then
                // sends session-changed. The mock also delivers a held message when it can.
                s.view.draft = false;
                if (s.view.messageWaiting && (s.view.status === "idle" || s.view.status === "done")) {
                    text(s, "\r\n\x1b[35m[held message delivered]\x1b[0m\r\n\x1b[32m$\x1b[0m ");
                }
                s.view.messageWaiting = false;
                changed(s);
                return null;
            }
            case "debug_paste": {
                const { sessionId, text: pasted } = args<{ sessionId: string; text: string }>(raw, [
                    "sessionId",
                    "text",
                ]);
                const s = sessionById(sessionId);
                const status = s.view.status;
                if (status === "question" || status === "starting" || status === "stopped") {
                    throw `REFUSED: status ${status}`;
                }
                if (s.view.draft) {
                    s.view.messageWaiting = true;
                    changed(s);
                    throw "REFUSED: draft";
                }
                s.view.messageWaiting = false;
                text(s, `\r\n\x1b[35m[paste]\x1b[0m ${pasted.replace(/\r?\n/g, "\r\n")}\r\n\x1b[32m$\x1b[0m `);
                changed(s);
                return null;
            }
            case "floor_logo": {
                const { path } = args<{ path: string }>(raw, ["path"]);
                return floorOf(path).logo;
            }
            case "logo_generate": {
                const { path, prompt } = args<{ path: string; prompt: string | null }>(raw, ["path", "prompt"]);
                const floor = floorOf(path);
                if (floor.logoBusy) throw "BUSY: a logo generation already runs for this floor";
                // An empty text removes the stored prompt (like the real app). A null keeps it.
                if (prompt !== null) floor.logoPrompt = prompt.trim() === "" ? null : prompt.trim();
                if (!settings.openaiKeySet) {
                    // No key: the call fails at once with `NO_KEY:`, like the real app. Nothing runs.
                    emit("floor-changed", { path });
                    throw "NO_KEY: no OpenAI key is known. Enter a key, or set OPENAI_API_KEY.";
                }
                floor.logoBusy = true;
                floor.logoError = null;
                emit("floor-changed", { path });
                setTimeout(() => {
                    floor.logoBusy = false;
                    floor.logoError = null;
                    floor.logo = makeLogo((floor.name[0] ?? "?").toUpperCase(), (Date.now() / 7) % 360);
                    floor.logoVersion = Date.now();
                    emit("floor-changed", { path });
                }, LOGO_MS);
                return null;
            }
            case "setup_start": {
                const a = args<{ path: string; cli: string; rows: number; cols: number }>(raw, [
                    "path",
                    "cli",
                    "rows",
                    "cols",
                ]);
                await sleep(config().startCallMs ?? 0);
                return startSetup(a.path, a.cli);
            }
            case "settings_get": {
                args(raw, []);
                return { ...settings };
            }
            case "settings_set": {
                const a = args<{ openaiKey: string | null; imageModel: string | null; notifyOnQuestion?: boolean | null }>(
                    raw,
                    ["openaiKey", "imageModel"],
                    ["notifyOnQuestion"],
                );
                if (a.notifyOnQuestion !== undefined && a.notifyOnQuestion !== null) {
                    settings.notifyOnQuestion = a.notifyOnQuestion;
                }
                if (a.openaiKey !== null) {
                    const key = a.openaiKey.trim();
                    settings.openaiKeySet = key !== "";
                    settings.openaiKeySource = key !== "" ? "config" : null;
                    if (key !== "") {
                        for (const floor of floors.values()) {
                            if (floor.logoError?.startsWith("NO_KEY:")) {
                                floor.logoError = null;
                                emit("floor-changed", { path: floor.path });
                            }
                        }
                    }
                }
                if (a.imageModel !== null) {
                    const model = a.imageModel.trim();
                    if (model !== "" && !isValidModelName(model)) {
                        throw `the image model ${JSON.stringify(model)} is not a valid model name.`;
                    }
                    // An empty text resets the model, like the real app.
                    settings.imageModel = model !== "" ? model : "gpt-image-2.6-sunburst";
                }
                return { ...settings };
            }
            case "models_get": {
                args(raw, []);
                return catalog();
            }
            case "models_set_custom": {
                const a = args<{ kind: string; models: string[] }>(raw, ["kind", "models"]);
                if (a.kind !== "claude" && a.kind !== "codex" && a.kind !== "image") {
                    throw `unknown model list ${JSON.stringify(a.kind)} (use claude, codex or image)`;
                }
                const names = a.models.map((m) => m.trim());
                for (const name of names) {
                    if (!isValidModelName(name)) throw `the model name ${JSON.stringify(name)} is not a valid model name.`;
                }
                const base = catalog()[a.kind].filter((m) => !m.custom).map((m) => m.id);
                custom[a.kind] = names.filter((n, i) => !base.includes(n) && names.indexOf(n) === i);
                return catalog();
            }
            case "floor_defaults_set": {
                const a = args<{ path: string; defaults: FloorDefaults }>(raw, ["path", "defaults"]);
                const floor = floorOf(a.path);
                checkModelAndEffort("default", a.defaults.model, a.defaults.effort);
                floor.defaults = {
                    cli: a.defaults.cli ?? null,
                    model: a.defaults.model ?? null,
                    effort: a.defaults.effort ?? null,
                    policy: a.defaults.policy ?? null,
                };
                emit("floor-changed", { path: a.path });
                return viewOf(floor);
            }
            case "programs_get": {
                args(raw, []);
                // `rtk` is optional. The e2e check sets `__towerMockConfig.rtk` to find it.
                programs.rtk = config().rtk ? "/usr/local/bin/rtk" : null;
                return { ...programs, overrides: { ...programs.overrides } };
            }
            case "programs_set": {
                const { name, path } = args<{ name: ProgramName; path: string }>(raw, ["name", "path"]);
                if (name !== "claude" && name !== "codex" && name !== "node") throw `unknown program ${name}`;
                if (path.trim() === "") {
                    // An empty path removes the override. The app searches again, and shows what it finds.
                    programs.overrides[name] = null;
                    programs[name] = FOUND[name];
                } else {
                    programs.overrides[name] = path.trim();
                    programs[name] = path.trim();
                }
                return { ...programs, overrides: { ...programs.overrides } };
            }
            case "program_test": {
                const { name } = args<{ name: ProgramName }>(raw, ["name"]);
                if (name !== "claude" && name !== "codex" && name !== "node") throw `unknown program ${name}`;
                await sleep(config().programTestMs ?? 0);
                const path = programs[name];
                if (path === null) {
                    return { ok: false, path: null, output: `The program "${name}" was not found. Set its path in Programs.` };
                }
                // A path whose file name starts with "broken" is a program that fails (for the e2e check).
                if (/(^|\/)broken[^/]*$/.test(path)) {
                    return { ok: false, path, output: "exit code 2: unknown option --version" };
                }
                return { ok: true, path, output: VERSIONS[name] };
            }
            case "task_list": {
                const a = args<{ path: string }>(raw, ["path"]);
                return floorOf(a.path).tasks.map((t) => ({ ...t.view }));
            }
            case "task_get": {
                const a = args<{ path: string; id: number }>(raw, ["path", "id"]);
                const t = taskOf(floorOf(a.path), a.id);
                return { task: { ...t.view }, events: t.events.map((e) => ({ ...e })) } satisfies TaskDetail;
            }
            case "task_create": {
                const a = args<{ path: string; title: string; description: string }>(raw, ["path", "title", "description"]);
                const floor = floorOf(a.path);
                const t = makeTask(floor, "operator", a.title, a.description);
                emit("tasks-changed", { path: a.path });
                return { ...t.view };
            }
            case "task_assign": {
                const a = args<{ path: string; id: number; slug: string }>(raw, ["path", "id", "slug"]);
                const floor = floorOf(a.path);
                const t = taskOf(floor, a.id);
                if (a.slug === SETUP_SLUG || !floor.employees.some((e) => e.slug === a.slug)) {
                    throw `There is no employee with the slug "${a.slug}" on this floor. Use the tool org_chart to see the team.`;
                }
                assignTask(t, "operator", a.slug);
                emit("tasks-changed", { path: a.path });
                return { ...t.view };
            }
            case "task_update": {
                const a = args<{ path: string; id: number; status: TaskStatus; note: string | null }>(raw, [
                    "path",
                    "id",
                    "status",
                    "note",
                ]);
                const t = taskOf(floorOf(a.path), a.id);
                if (!TASK_STATUSES.includes(a.status)) throw `unknown variant \`${a.status}\``;
                const note = a.note ?? null;
                if (note !== null && [...note].length > 20_000) {
                    throw `The note is too long: ${[...note].length} characters. The limit is 20000. Write a shorter text, or put the details in a file.`;
                }
                updateTask(t, "operator", a.status, note, null);
                emit("tasks-changed", { path: a.path });
                return { ...t.view };
            }
            case "task_merge": {
                const a = args<{ path: string; id: number }>(raw, ["path", "id"]);
                const floor = floorOf(a.path);
                const t = taskOf(floor, a.id);
                if (t.view.status !== "review" && t.view.status !== "blocked") {
                    throw `Task T-${a.id} has the status ${t.view.status}. Only a task in review or blocked can be merged.`;
                }
                if (t.view.branch === null) {
                    const note = "There is nothing to merge: the task has no worktree and no branch. The work is in the project folder.";
                    updateTask(t, "operator", "done", note, null);
                    emit("tasks-changed", { path: a.path });
                    return { merged: true, message: `Task T-${a.id}: ${note} The status is done.`, task: { ...t.view } } satisfies TaskMerge;
                }
                if (floor.mergeBlock !== null) {
                    updateTask(t, "operator", "blocked", floor.mergeBlock, null);
                    emit("tasks-changed", { path: a.path });
                    emit("notice", { path: a.path, kind: "task_blocked", text: `Task T-${a.id} is blocked: ${floor.mergeBlock}` });
                    return {
                        merged: false,
                        message: `Task T-${a.id} is blocked and not merged. ${floor.mergeBlock} The status is blocked.`,
                        task: { ...t.view },
                    } satisfies TaskMerge;
                }
                const branch = t.view.branch;
                const note = `Merged ${branch} into main (commit 89abcdef0123). The worktree is removed. The branch is removed.`;
                updateTask(t, "operator", "done", note, null);
                t.view.worktree = null;
                emit("tasks-changed", { path: a.path });
                return { merged: true, message: `Task T-${a.id}: ${note} The status is done.`, task: { ...t.view } } satisfies TaskMerge;
            }
            case "task_diff": {
                const a = args<{ path: string; id: number }>(raw, ["path", "id"]);
                const t = taskOf(floorOf(a.path), a.id);
                if (t.view.branch === null) {
                    return {
                        branch: null,
                        baseBranch: null,
                        stat: "",
                        patch: "",
                        truncated: false,
                        note: "This task has no git branch: no worktree was made for it. The employee edits the project folder directly, so there is no diff to show.",
                    } satisfies TaskDiff;
                }
                return {
                    branch: t.view.branch,
                    baseBranch: t.view.baseBranch,
                    stat: " a.txt | 1 +\n 1 file changed, 1 insertion(+)\n",
                    patch:
                        "diff --git a/a.txt b/a.txt\nnew file mode 100644\nindex 0000000..8c7e5a6\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+A\n",
                    truncated: false,
                    note: null,
                } satisfies TaskDiff;
            }
            case "task_discard": {
                const a = args<{ path: string; id: number }>(raw, ["path", "id"]);
                const t = taskOf(floorOf(a.path), a.id);
                if (t.view.status === "done") throw `Task T-${a.id} is done. The app does not discard a done task.`;
                updateTask(t, "operator", "blocked", "discarded by the operator", null);
                t.view.branch = null;
                t.view.baseBranch = null;
                t.view.baseCommit = null;
                t.view.worktree = null;
                emit("tasks-changed", { path: a.path });
                return { ...t.view };
            }
            case "floor_permissions_get": {
                const a = args<{ path: string }>(raw, ["path"]);
                return [...floorOf(a.path).permissions];
            }
            case "floor_permissions_set": {
                const a = args<{ path: string; allow: string[] }>(raw, ["path", "allow"]);
                const floor = floorOf(a.path);
                const clean: string[] = [];
                for (const raw of a.allow) {
                    const rule = raw.trim();
                    const bad = checkAllowRule(rule);
                    if (bad !== null) throw `the rule "${rule}" is refused: ${bad}`;
                    // A rule that the client check accepts and that the "server" refuses, for the e2e check.
                    if (rule.includes("server-says-no")) throw `the rule "${rule}" is refused by the server: this word is on its list.`;
                    if (!clean.includes(rule)) clean.push(rule);
                }
                if (clean.length > ALLOW_RULES_MAX) throw `there are ${clean.length} rules, and the limit is ${ALLOW_RULES_MAX}`;
                floor.permissions = clean;
                floor.permissionsStored = true;
                emit("floor-changed", { path: a.path });
                // The server gives one warning for each rule that runs the code of the project.
                const warnings = clean.map(ruleWarning).filter((w): w is string => w !== null);
                return { allow: [...clean], warnings } satisfies PermissionsSaved;
            }
            case "floor_permissions_proposed": {
                const a = args<{ path: string }>(raw, ["path"]);
                const floor = floorOf(a.path);
                return floor.permissionsStored ? [] : [...floor.proposed];
            }
            case "team_resume": {
                const a = args<{ path: string }>(raw, ["path"]);
                const floor = floorOf(a.path);
                if (floor.resumeProgress !== null) throw "the last team is resuming now";
                const list = [...floor.resumable];
                if (list.length === 0) throw "there is no session of the last team to resume";
                floor.resumeProgress = { done: 0, total: list.length };
                emit("floor-changed", { path: a.path });
                void (async () => {
                    for (const [index, slug] of list.entries()) {
                        if (index > 0) await sleep(config().resumePauseMs ?? 400);
                        try {
                            if (failResume.delete(slug)) throw "forced test failure";
                            startSession(a.path, slug);
                            floor.resumable = floor.resumable.filter((s) => s !== slug);
                        } catch (e) {
                            emit("notice", { path: a.path, kind: "resume_failed", text: `Could not resume ${slug}: ${String(e)}` });
                        }
                        floor.resumeProgress = { done: index + 1, total: list.length };
                        emit("floor-changed", { path: a.path });
                    }
                    floor.resumeProgress = null;
                    emit("floor-changed", { path: a.path });
                })();
                return { done: 0, total: list.length } satisfies ResumeProgress;
            }
            case "task_report": {
                const a = args<{ path: string; id: number }>(raw, ["path", "id"]);
                const t = taskOf(floorOf(a.path), a.id);
                if (t.view.report === null) return null;
                if (t.reportText === null) throw `cannot read the report file ${t.view.report}: No such file or directory (os error 2)`;
                return t.reportText;
            }
            default:
                throw `unknown command ${command}`;
        }
    }

    (globalThis as Record<string, unknown>).__towerMock = {
        output(sessionId, value) {
            text(sessionById(sessionId), value);
        },
        leaveStarting(sessionId) {
            const s = sessionById(sessionId);
            if (s.view.status === "starting") setStatus(s, s.view.slug === SETUP_SLUG ? "working" : "idle");
        },
        employeeStart(path, bySlug, slug) {
            const floor = floorOf(path);
            const by = floor.employees.find((e) => e.slug === bySlug);
            const target = floor.employees.find((e) => e.slug === slug);
            if (!by || !target) throw `unknown employee ${by ? slug : bySlug}`;
            const view = startSession(path, slug);
            emit("employee-started", {
                path,
                sessionId: view.sessionId,
                slug,
                name: target.name,
                bySlug,
                byName: by.name,
                resumed: false,
            });
            return view.sessionId;
        },
        taskByEmployee(path, bySlug, change) {
            const floor = floorOf(path);
            let id: number;
            if ("create" in change) {
                id = makeTask(floor, bySlug, change.create, change.description ?? "").view.id;
            } else if ("assign" in change) {
                id = change.id;
                assignTask(taskOf(floor, id), bySlug, change.assign);
            } else {
                id = change.id;
                updateTask(taskOf(floor, id), bySlug, change.status, change.note ?? null, change.report ?? null);
            }
            emit("tasks-changed", { path });
            return id;
        },
        taskWorktree(path, id) {
            giveWorktree(taskOf(floorOf(path), id));
            emit("tasks-changed", { path });
        },
        blockMerge(path, reason) {
            floorOf(path).mergeBlock = reason;
        },
        setResumable(path, slugs) {
            floorOf(path).resumable = [...slugs];
            emit("floor-changed", { path });
        },
        failResume(slug) {
            failResume.add(slug);
        },
        freeze(on) {
            frozen = on;
            if (on) {
                for (const s of sessions.values()) {
                    if (s.timer) clearTimeout(s.timer);
                    s.timer = null;
                }
            }
        },
        setStatus(sessionId, status) {
            setStatus(sessionById(sessionId), status);
        },
        startSession(path, slug) {
            return startSession(path, slug).sessionId;
        },
        addEmployees(path, count) {
            const floor = floorOf(path);
            const start = floor.employees.length;
            const pm = floor.employees.find((e) => e.role === "Project Manager")?.slug ?? floor.employees[0].slug;
            for (let i = 0; i < count; i++) {
                const e = employee(`gen-${start + i}`, `Worker ${String(start + i).padStart(2, "0")}`, "Engineer", pm, "claude");
                e.model = null;
                floor.employees.push(e);
            }
            emit("floor-changed", { path });
        },
        deliverMessage(path, fromSlug, toSlug, what) {
            const floor = floorOf(path);
            const nameOf = (slug: string): string =>
                slug === "operator" ? "Operator" : (floor.employees.find((e) => e.slug === slug)?.name ?? slug);
            const body = typeof what === "string" ? what : what.status === undefined ? `Title: ${taskOf(floor, what.task).view.title}` : `Title: ${taskOf(floor, what.task).view.title}\nStatus: ${what.status}`;
            // The same rule as the Rust side: remove control characters, turn a newline into a space, keep 80 characters.
            // eslint-disable-next-line no-control-regex
            const preview = [...body.replace(/\r\n/g, "\n").replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u200b-\u200f\u2028-\u202e\ufeff]/g, "").replace(/[\n\t]/g, " ")]
                .slice(0, 80)
                .join("");
            emit("message-delivered", {
                path,
                fromSlug,
                fromName: nameOf(fromSlug),
                toSlug,
                toName: nameOf(toSlug),
                kind: typeof what === "string" ? "message" : "task",
                taskId: typeof what === "string" ? null : what.task,
                taskStatus: typeof what === "string" || what.status === undefined ? null : what.status,
                preview,
            });
        },
        notice(path, text, kind = "task_blocked") {
            emit("notice", { path, kind, text });
        },
        goneFloor(path) {
            const floor = floorOf(path);
            floor.gone = true;
            const live = [...sessions.values()].filter((s) => s.path === path && s.view.status !== "stopped").length;
            const running = live === 0 ? "" : live === 1 ? " 1 session of it still runs." : ` ${live} sessions of it still run.`;
            emit("floor-changed", { path });
            emit("notice", { path, kind: "floor_gone", text: `The folder of the floor ${floor.name} is gone: ${path}.${running}` });
        },
        backFloor(path) {
            const floor = floorOf(path);
            floor.gone = false;
            emit("floor-changed", { path });
            emit("notice", { path, kind: "floor_back", text: `The folder of the floor ${floor.name} is back: ${path}.` });
        },
        setProposed(path, rules) {
            const floor = floorOf(path);
            floor.proposed = [...rules];
            floor.permissionsStored = false;
        },
        setEmployeeErrors(path, errors) {
            floorOf(path).employeeErrors = [...errors];
            emit("floor-changed", { path });
        },
        setDescription(path, text) {
            floorOf(path).description = text;
            emit("floor-changed", { path });
        },
        patchFloor(path, change) {
            const floor = floorOf(path);
            if (change.defaults) floor.defaults = { ...floor.defaults, ...change.defaults };
            if (change.employee) {
                const e = floor.employees.find((x) => x.slug === change.employee?.slug);
                if (e) Object.assign(e, change.employee.patch);
            }
            emit("floor-changed", { path });
        },
    } satisfies MockControl;

    return {
        kind: "mock",
        async invoke<T>(command: string, a?: Record<string, unknown>): Promise<T> {
            // A little delay, like a real IPC call.
            await new Promise((r) => setTimeout(r, 5));
            // A log of calls, for the e2e check (`window.__towerMockCalls`).
            const log = ((globalThis as Record<string, unknown>).__towerMockCalls ??= []) as unknown[];
            log.push({ command, args: a && "channel" in a ? { ...a, channel: "[channel]" } : a });
            return (await handle(command, a)) as T;
        },
        async listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
            const set = listeners.get(event) ?? new Set();
            set.add(handler as (payload: unknown) => void);
            listeners.set(event, set);
            return () => set.delete(handler as (payload: unknown) => void);
        },
        newChannel(onMessage) {
            return { onmessage: onMessage };
        },
    };
}
