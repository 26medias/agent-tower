// In-memory mock of the Rust side, for `pnpm dev` in a normal browser and for the e2e check.
// It follows docs/ipc-m1.md and docs/ipc-visuals-init.md: same command names, same argument names,
// same error texts.
import { base64ToBytes, bytesToBase64, concatBytes } from "./codec";
import type { Backend, Unlisten } from "./ipc";
import { STANDARD_EFFORTS, isValidEffort, isValidModelName } from "./models";
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
    ProgramName,
    Programs,
    SessionView,
    Settings,
    Status,
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
}

interface MockSession {
    view: SessionView;
    path: string;
    log: Uint8Array;
    channel: { onmessage: (m: OutputMessage) => void } | null;
    line: number[];
    timer: ReturnType<typeof setTimeout> | null;
}

const enc = new TextEncoder();
const dec = new TextDecoder();
const LOG_MAX = 256 * 1024;
const NEXT: Record<string, [Status, number]> = {
    starting: ["idle", 1200],
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
    const programs: Programs = {
        claude: "/usr/local/bin/claude",
        codex: null,
        node: "/usr/bin/node",
        hook: "/opt/agent-tower/bin/tower-hook",
    };
    let counter = 0;
    // Errors that the mock gives one time, to show what the UI does when a start fails.
    const failOnce = new Set(["start:eng-sam", "resume:pm"]);

    const settings: Settings = { openaiKeySet: false, openaiKeySource: null, imageModel: "gpt-image-2.6-sunburst" };
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
        };
        floors.set(path, floor);
        if (!known.includes(path)) known.push(path);
        return floor;
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

    // ---- helpers ----

    function emit(event: string, payload: unknown): void {
        for (const handler of listeners.get(event) ?? []) {
            setTimeout(() => handler(payload), 0);
        }
    }

    function viewOf(floor: MockFloor): FloorView {
        const setup = floor.sessions.find((x) => x.slug === SETUP_SLUG && x.status !== "stopped");
        return {
            path: floor.path,
            name: floor.name,
            employees: structuredClone(floor.employees),
            employeeErrors: [],
            sessions: structuredClone(floor.sessions),
            description: floor.description,
            logoPrompt: floor.logoPrompt,
            logoVersion: floor.logoVersion,
            logoBusy: floor.logoBusy,
            logoError: floor.logoError,
            setupSessionId: setup ? setup.sessionId : null,
            defaults: { ...floor.defaults },
        };
    }

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
        s.view.status = status;
        // The setup session cannot resume.
        if (status === "stopped" && s.view.slug !== SETUP_SLUG) s.view.canResume = true;
        changed(s);
        if (status === "stopped" && s.view.slug === SETUP_SLUG) emit("floor-changed", { path: s.path });
        schedule(s);
    }

    function schedule(s: MockSession): void {
        if (s.timer) clearTimeout(s.timer);
        s.timer = null;
        // The setup session goes to `working` and then waits for its own timer (see `startSetup`).
        const next: [Status, number] | undefined =
            s.view.slug === SETUP_SLUG ? (s.view.status === "starting" ? ["working", 1200] : undefined) : NEXT[s.view.status];
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
        };
        sessions.set(s.view.sessionId, s);
        banner(s, emp);
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
        };
        sessions.set(s.view.sessionId, s);
        text(s, `\x1b[1;36mAgent Tower mock setup agent\x1b[0m (${cli}) in \x1b[1m${floor.name}\x1b[0m\r\n`);
        text(s, "Reading CLAUDE.md, README.md and the top-level layout...\r\n");
        changed(s);
        schedule(s);
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
        }, 3500);
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

    function args<T>(raw: Record<string, unknown> | undefined, names: string[]): T {
        const a = raw ?? {};
        for (const name of names) {
            if (!(name in a)) throw `missing argument ${name}`;
        }
        for (const name of Object.keys(a)) {
            if (!names.includes(name)) throw `unexpected argument ${name}`;
        }
        return a as T;
    }

    async function handle(command: string, raw?: Record<string, unknown>): Promise<unknown> {
        switch (command) {
            case "floor_list": {
                args(raw, []);
                return known.map<FloorEntry>((path) => {
                    const floor = floors.get(path);
                    return {
                        path,
                        name: floor ? floor.name : null,
                        missing: !floor,
                        logoVersion: floor ? floor.logoVersion : null,
                    };
                });
            }
            case "floor_open": {
                const { path } = args<{ path: string }>(raw, ["path"]);
                const typed = path.trim();
                if (typed === "") throw "the path is empty";
                const clean = canonicalPath(typed);
                const floor = floors.get(clean);
                if (!floor) throw `NO_FLOOR: ${typed} has no .swarm/ folder`;
                if (!known.includes(clean)) known.push(clean);
                return viewOf(floor);
            }
            case "floor_create": {
                const { path, name } = args<{ path: string; name: string }>(raw, ["path", "name"]);
                const clean = canonicalPath(path);
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
                const i = known.indexOf(path);
                if (i >= 0) known.splice(i, 1);
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
                    const live = enc.encode("\r\n\x1b[2mchannel online ✓ café\x1b[0m\r\n");
                    const cut = live.indexOf(0xe2) + 1; // inside the 3 bytes of U+2713
                    setTimeout(() => {
                        output(s, live.slice(0, cut));
                        setTimeout(() => {
                            output(s, live.slice(cut));
                            showPrompt(s);
                        }, 20);
                    }, 80);
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
                return startSetup(a.path, a.cli);
            }
            case "settings_get": {
                args(raw, []);
                return { ...settings };
            }
            case "settings_set": {
                const a = args<{ openaiKey: string | null; imageModel: string | null }>(raw, ["openaiKey", "imageModel"]);
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
                return { ...programs };
            }
            case "programs_set": {
                const { name, path } = args<{ name: ProgramName; path: string }>(raw, ["name", "path"]);
                if (name !== "claude" && name !== "codex" && name !== "node") throw `unknown program ${name}`;
                if (path.trim() === "") throw "the path is empty";
                programs[name] = path.trim();
                return { ...programs };
            }
            default:
                throw `unknown command ${command}`;
        }
    }

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
