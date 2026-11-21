import { describe, expect, it } from "vitest";
import { createMockBackend, type MockControl } from "./mock";
import { settingsSetArgs } from "./ipc";
import type { FloorView, ProgramTestResult, Programs, SandboxProbe, Settings } from "./types";

const FLOOR = "/home/demo/tower";
const control = () => (globalThis as unknown as { __towerMock: MockControl }).__towerMock;

async function rejected(promise: Promise<unknown>): Promise<string> {
    try {
        await promise;
    } catch (e) {
        return String(e);
    }
    return "";
}

describe("mock: the path errors of floor_open and floor_create", () => {
    it("gives the four errors for the simple cases", async () => {
        const b = createMockBackend();
        const open = (path: string) => rejected(b.invoke("floor_open", { path }));
        expect(await open("relative/dir")).toMatch(/^NOT_ABSOLUTE: /);
        expect(await open("/missing/x")).toBe("NOT_FOUND: The folder /missing/x does not exist.");
        expect(await open("/tmp/notes.txt")).toBe("NOT_A_FOLDER: /tmp/notes.txt is a file, not a folder.");
        expect(await open("/readonly/x")).toBe("NO_WRITE: You cannot write in the folder /readonly/x.");
        expect(await rejected(b.invoke("floor_create", { path: "rel", name: "X" }))).toMatch(/^NOT_ABSOLUTE: /);
        expect(await rejected(b.invoke("floor_create", { path: "/readonly/x", name: "X" }))).toMatch(/^NO_WRITE: /);
    });

    it("expands ~ before the check, and keeps NO_FLOOR for a good folder with no floor", async () => {
        const b = createMockBackend();
        expect(await rejected(b.invoke("floor_open", { path: "~/a.txt" }))).toBe("NOT_A_FOLDER: /home/demo/a.txt is a file, not a folder.");
        expect(await rejected(b.invoke("floor_open", { path: "/home/demo/new-project" }))).toMatch(/^NO_FLOOR: /);
    });
});

describe("mock: a gone floor", () => {
    it("gives gone: true, a missing entry, and FLOOR_GONE for the commands on the floor", async () => {
        const b = createMockBackend();
        const before = await b.invoke<FloorView>("floor_open", { path: FLOOR });
        expect(before.gone).toBe(false);
        control().goneFloor(FLOOR);
        const view = await b.invoke<FloorView>("floor_open", { path: FLOOR });
        expect(view.gone).toBe(true);
        expect(view.employees.length).toBe(before.employees.length);
        const list = await b.invoke<{ path: string; missing: boolean }[]>("floor_list");
        expect(list.find((f) => f.path === FLOOR)?.missing).toBe(true);
        const gone = `FLOOR_GONE: The folder of this floor is gone: ${FLOOR}.`;
        expect(await rejected(b.invoke("task_list", { path: FLOOR }))).toBe(gone);
        expect(await rejected(b.invoke("session_start", { path: FLOOR, slug: "ceo", firstMessage: null, rows: 24, cols: 80 }))).toBe(gone);
        expect(await rejected(b.invoke("floor_permissions_get", { path: FLOOR }))).toBe(gone);
        // These still work.
        await b.invoke("floor_list");
        await b.invoke("floor_open", { path: FLOOR });
    });

    it("works again when the folder is back", async () => {
        const b = createMockBackend();
        control().goneFloor(FLOOR);
        control().backFloor(FLOOR);
        expect((await b.invoke<FloorView>("floor_open", { path: FLOOR })).gone).toBe(false);
        expect(await b.invoke<unknown[]>("task_list", { path: FLOOR })).toBeInstanceOf(Array);
    });

    it("sends floor-changed first and then the notice", async () => {
        const b = createMockBackend();
        const seen: string[] = [];
        await b.listen("floor-changed", () => seen.push("floor-changed"));
        await b.listen<{ kind: string }>("notice", (p) => seen.push(p.kind));
        control().goneFloor(FLOOR);
        control().backFloor(FLOOR);
        await new Promise((r) => setTimeout(r, 20));
        expect(seen).toEqual(["floor-changed", "floor_gone", "floor-changed", "floor_back"]);
    });

    it("does not forget a floor with a live session, and forgets a gone floor with none", async () => {
        const b = createMockBackend();
        const id = control().startSession(FLOOR, "ceo");
        control().goneFloor(FLOOR);
        expect(await rejected(b.invoke("floor_forget", { path: FLOOR }))).toMatch(/live sessions/);
        await b.invoke("session_stop", { sessionId: id });
        await b.invoke("floor_forget", { path: FLOOR });
        expect(await rejected(b.invoke("floor_open", { path: FLOOR }))).toMatch(/^NOT_FOUND: /);
    });
});

describe("mock: the rules for commands with no question", () => {
    it("floor_permissions_set returns the stored list and one warning for each rule that runs the project code or has a wildcard", async () => {
        const b = createMockBackend();
        const saved = await b.invoke<{ allow: string[]; warnings: string[] }>("floor_permissions_set", {
            path: FLOOR,
            allow: ["Bash(rtk cargo test:*)", "Bash(ls:*)", "Bash(rtk cargo test:*)"],
        });
        expect(saved.allow).toEqual(["Bash(rtk cargo test:*)", "Bash(ls:*)"]);
        expect(saved.warnings).toEqual([
            "Bash(rtk cargo test:*): This rule lets each agent of the floor run the project's own code with no question.",
            "Bash(ls:*): This rule lets each agent of the floor run this program with any arguments and no question. Many programs can write files or run other commands through their options.",
        ]);
        expect(await b.invoke("floor_permissions_get", { path: FLOOR })).toEqual(saved.allow);
    });

    it("floor_permissions_set gives the path warning and the web warning, and refuses a shell character", async () => {
        const b = createMockBackend();
        const saved = await b.invoke<{ allow: string[]; warnings: string[] }>("floor_permissions_set", {
            path: FLOOR,
            allow: ["Read(./src/**)", "Read(./README.md)", "WebFetch(domain:example.com)"],
        });
        expect(saved.warnings).toEqual([
            "Read(./src/**): This rule lets each agent of the floor read every file that the pattern matches, with no question. A pattern with a wildcard can match more files than you expect.",
            "WebFetch(domain:example.com): This rule lets each agent of the floor get pages from this host with no question. The app does not check where the host name points.",
        ]);
        expect(await rejected(b.invoke("floor_permissions_set", { path: FLOOR, allow: ["Bash(ls && rm)"] }))).toMatch(/shell character &/);
        expect(await b.invoke("floor_permissions_get", { path: FLOOR })).toEqual(saved.allow);
    });

    it("refuses Bash(rtk proxy:*) and writes nothing", async () => {
        const b = createMockBackend();
        expect(await rejected(b.invoke("floor_permissions_set", { path: FLOOR, allow: ["Bash(rtk proxy:*)"] }))).toMatch(/refused/);
        expect(await b.invoke("floor_permissions_get", { path: FLOOR })).toEqual([]);
    });

    it("offers the proposal until a list is stored, and not for a gone floor", async () => {
        const b = createMockBackend();
        expect(await b.invoke("floor_permissions_proposed", { path: FLOOR })).toEqual([]);
        control().setProposed(FLOOR, ["Bash(cargo test:*)"]);
        expect(await b.invoke("floor_permissions_proposed", { path: FLOOR })).toEqual(["Bash(cargo test:*)"]);
        await b.invoke("floor_permissions_set", { path: FLOOR, allow: [] });
        expect(await b.invoke("floor_permissions_proposed", { path: FLOOR })).toEqual([]);
        control().goneFloor(FLOOR);
        expect(await rejected(b.invoke("floor_permissions_proposed", { path: FLOOR }))).toMatch(/^FLOOR_GONE: /);
    });
});

describe("mock: settings switch, program test and reset", () => {
    it("has the notification switch on by default, and settings_set changes only it", async () => {
        const b = createMockBackend();
        expect((await b.invoke<Settings>("settings_get")).notifyOnQuestion).toBe(true);
        const off = await b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, notifyOnQuestion: false });
        expect(off.notifyOnQuestion).toBe(false);
        // The old call with no such argument keeps the switch, and null is "no change".
        const same = await b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: "m-1" });
        expect(same).toMatchObject({ notifyOnQuestion: false, imageModel: "m-1" });
        const nul = await b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, notifyOnQuestion: null });
        expect(nul.notifyOnQuestion).toBe(false);
        const on = await b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, notifyOnQuestion: true });
        expect(on.notifyOnQuestion).toBe(true);
    });

    it("has the Codex sandbox switch off by default and turns it on only when the probe says works", async () => {
        const g = globalThis as Record<string, unknown>;
        const b = createMockBackend();
        const probe = () => b.invoke<SandboxProbe>("codex_sandbox_probe");
        const set = (codexSandbox?: boolean | null) =>
            b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, ...(codexSandbox === undefined ? {} : { codexSandbox }) });
        try {
            expect((await b.invoke<Settings>("settings_get")).codexSandbox).toBe(false);
            // Each state of the probe.
            g.__towerMockConfig = { sandboxProbe: { state: "fails" } };
            expect(await probe()).toEqual({ state: "fails", detail: "bwrap: setting up uid map: Permission denied" });
            g.__towerMockConfig = { sandboxProbe: { state: "missing" } };
            expect(await probe()).toMatchObject({ state: "missing" });
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            expect(await probe()).toMatchObject({ state: "works" });
            // A probe that does not say works refuses the switch, with the text of the problem.
            g.__towerMockConfig = { sandboxProbe: { state: "fails", detail: "bwrap: no way" } };
            expect(await rejected(set(true))).toMatch(/does not work on this machine.*bwrap: no way/);
            g.__towerMockConfig = { sandboxProbe: { state: "missing" } };
            expect(await rejected(set(true))).toMatch(/cannot be turned on/);
            expect((await b.invoke<Settings>("settings_get")).codexSandbox).toBe(false);
            // A probe that says works accepts it. A missing argument and null change nothing. Off is always allowed.
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            expect((await set(true)).codexSandbox).toBe(true);
            expect((await set()).codexSandbox).toBe(true);
            expect((await set(null)).codexSandbox).toBe(true);
            g.__towerMockConfig = { sandboxProbe: { state: "fails" } };
            expect((await set(false)).codexSandbox).toBe(false);
        } finally {
            delete g.__towerMockConfig;
        }
    });

    it("refuses the start of a Codex employee when the setting is on and the probe does not say works", async () => {
        const g = globalThis as Record<string, unknown>;
        const b = createMockBackend();
        const set = (codexSandbox: boolean) => b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, codexSandbox });
        const start = (slug: string) => b.invoke("session_start", { path: FLOOR, slug, firstMessage: null, rows: 24, cols: 80 });
        try {
            // `eng-sam` has a start that fails once for the demo. Use another employee as the Codex one.
            control().patchFloor(FLOOR, { employee: { slug: "eng-lin", patch: { cli: "codex" } } });
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            await set(true);
            // The machine changes after the switch was turned on. The stored value stays on.
            g.__towerMockConfig = { sandboxProbe: { state: "fails", detail: "bwrap: no way" } };
            const refused = await rejected(start("eng-lin"));
            expect(refused).toBe(
                'SANDBOX_DOWN: The Codex sandbox does not work on this machine now (bwrap: no way). The start of Lin is refused, because the setting "Use the Codex sandbox" is on. Repair the sandbox, or turn the setting off in Settings > Codex sandbox to start Codex with no sandbox.',
            );
            expect((await b.invoke<Settings>("settings_get")).codexSandbox).toBe(true);
            // A Claude employee is not affected.
            await start("pm");
            // Turning it off is always allowed, and then the Codex start has no sandbox.
            expect((await set(false)).codexSandbox).toBe(false);
            await start("eng-lin");
        } finally {
            delete g.__towerMockConfig;
        }
    });

    it("sends codexSandbox in settings_set only when it has a value", () => {
        expect(settingsSetArgs(null, null, false, null)).toEqual({ openaiKey: null, imageModel: null, notifyOnQuestion: false });
        expect(Object.keys(settingsSetArgs(null, null, null, null))).not.toContain("codexSandbox");
        expect(settingsSetArgs(null, null, null, true)).toEqual({ openaiKey: null, imageModel: null, notifyOnQuestion: null, codexSandbox: true });
        expect(settingsSetArgs(null, null, null, false)).toMatchObject({ codexSandbox: false });
    });

    it("tests a program: a good result, a bad result and a program that is not found", async () => {
        const b = createMockBackend();
        const test = (name: string) => b.invoke<ProgramTestResult>("program_test", { name });
        expect(await test("claude")).toEqual({ ok: true, path: "/usr/local/bin/claude", output: "2.1.330 (Claude Code)" });
        expect(await test("codex")).toMatchObject({ ok: false, path: null });
        await b.invoke("programs_set", { name: "node", path: "/opt/broken/node-x" });
        expect(await test("node")).toMatchObject({ ok: true });
        await b.invoke("programs_set", { name: "node", path: "/opt/tools/broken-node" });
        expect(await test("node")).toEqual({ ok: false, path: "/opt/tools/broken-node", output: "exit code 2: unknown option --version" });
        expect(await rejected(test("rm"))).toMatch(/unknown program/);
    });

    it("resets a program with an empty path and shows the path that the app finds", async () => {
        const b = createMockBackend();
        const set = await b.invoke<Programs>("programs_set", { name: "claude", path: "/my/claude" });
        expect(set.claude).toBe("/my/claude");
        expect(set.overrides.claude).toBe("/my/claude");
        expect(set.overrides.node).toBeNull();
        const reset = await b.invoke<Programs>("programs_set", { name: "claude", path: "" });
        expect(reset.overrides.claude).toBeNull();
        expect(reset.claude).toBe("/usr/local/bin/claude");
        // A program that the app does not find has no path after the reset.
        await b.invoke("programs_set", { name: "codex", path: "/my/codex" });
        const none = await b.invoke<Programs>("programs_set", { name: "codex", path: "  " });
        expect(none.codex).toBeNull();
        expect(none.overrides.codex).toBeNull();
    });
});
