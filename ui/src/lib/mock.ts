// In-memory mock of the Rust side, for `pnpm dev` in a normal browser and for the e2e check.
// It follows docs/ipc-m1.md: same command names, same argument names, same error texts.
import { base64ToBytes, bytesToBase64, concatBytes } from "./codec";
import type { Backend, Unlisten } from "./ipc";
import type {
    AttachResult,
    Employee,
    FloorEntry,
    FloorView,
    OutputMessage,
    ProgramName,
    Programs,
    SessionView,
    Status,
} from "./types";

interface MockFloor {
    path: string;
    name: string;
    employees: Employee[];
    sessions: SessionView[];
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

    function addFloor(path: string, name: string, employees: Employee[]): MockFloor {
        const floor = { path, name, employees, sessions: [] };
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
    // Lin has a stopped session from "an earlier run" that can resume.
    tower.sessions.push({
        sessionId: "s-old-lin",
        slug: "eng-lin",
        status: "stopped",
        draft: false,
        messageWaiting: false,
        canResume: true,
    });
    addFloor("/home/demo/second", "Second Floor", [employee("ceo", "Bo", "CEO", null, "claude")]);
    // A path in the list that does not exist any more.
    known.push("/home/demo/gone");

    // ---- helpers ----

    function emit(event: string, payload: unknown): void {
        for (const handler of listeners.get(event) ?? []) {
            setTimeout(() => handler(payload), 0);
        }
    }

    function viewOf(floor: MockFloor): FloorView {
        return {
            path: floor.path,
            name: floor.name,
            employees: structuredClone(floor.employees),
            employeeErrors: [],
            sessions: structuredClone(floor.sessions),
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
        if (status === "stopped") s.view.canResume = true;
        changed(s);
        schedule(s);
    }

    function schedule(s: MockSession): void {
        if (s.timer) clearTimeout(s.timer);
        s.timer = null;
        const next = NEXT[s.view.status];
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
                    return { path, name: floor ? floor.name : null, missing: !floor };
                });
            }
            case "floor_open": {
                const { path } = args<{ path: string }>(raw, ["path"]);
                const clean = path.trim();
                if (clean === "") throw "the path is empty";
                const floor = floors.get(clean);
                if (!floor) throw `NO_FLOOR: ${clean} has no .swarm/ folder`;
                if (!known.includes(clean)) known.push(clean);
                return viewOf(floor);
            }
            case "floor_create": {
                const { path, name } = args<{ path: string; name: string }>(raw, ["path", "name"]);
                const clean = path.trim();
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
                if (!/^[a-z0-9][a-z0-9-]*$/.test(emp.slug)) throw `bad slug "${emp.slug}"`;
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
                return startSession(a.path, a.slug);
            }
            case "session_resume": {
                const a = args<{ path: string; slug: string; message: string | null; rows: number; cols: number }>(
                    raw,
                    ["path", "slug", "message", "rows", "cols"],
                );
                const old = floorOf(a.path).sessions.find((x) => x.slug === a.slug);
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
                s.view.draft = false;
                // The mock delivers the waiting message when it can.
                if (s.view.messageWaiting && (s.view.status === "idle" || s.view.status === "done")) {
                    s.view.messageWaiting = false;
                    text(s, "\r\n\x1b[35m[held message delivered]\x1b[0m\r\n\x1b[32m$\x1b[0m ");
                }
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
