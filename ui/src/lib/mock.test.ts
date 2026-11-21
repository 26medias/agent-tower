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

    it("has the Codex sandbox on Automatic by default and takes Always only when the probe says works", async () => {
        const g = globalThis as Record<string, unknown>;
        const b = createMockBackend();
        const probe = () => b.invoke<SandboxProbe>("codex_sandbox_probe");
        const get = () => b.invoke<Settings>("settings_get");
        const set = (args: Record<string, unknown> = {}) => b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, ...args });
        try {
            // The default: the mode is auto. The probe works (the mock default), so it is on in effect.
            expect(await get()).toMatchObject({ codexSandboxMode: "auto", codexSandboxEffective: "on", codexSandbox: true });
            // Each state of the probe.
            g.__towerMockConfig = { sandboxProbe: { state: "fails" } };
            expect(await probe()).toEqual({ state: "fails", detail: "bwrap: setting up uid map: Permission denied" });
            g.__towerMockConfig = { sandboxProbe: { state: "missing" } };
            expect(await probe()).toMatchObject({ state: "missing" });
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            expect(await probe()).toMatchObject({ state: "works" });
            // The effective state follows the last probe: Automatic with a failing probe is off.
            g.__towerMockConfig = { sandboxProbe: { state: "fails", detail: "bwrap: no way" } };
            await probe();
            expect(await get()).toMatchObject({ codexSandboxMode: "auto", codexSandboxEffective: "off", codexSandbox: false });
            // A probe that does not say works refuses "Always", with the text of the problem, for the new and the old argument.
            expect(await rejected(set({ codexSandboxMode: "on" }))).toMatch(/does not work on this machine.*bwrap: no way/);
            expect(await rejected(set({ codexSandbox: true }))).toMatch(/does not work on this machine.*bwrap: no way/);
            g.__towerMockConfig = { sandboxProbe: { state: "missing" } };
            expect(await rejected(set({ codexSandboxMode: "on" }))).toMatch(/cannot be set to "Always"/);
            expect((await get()).codexSandboxMode).toBe("auto");
            // A word that is not one of the three is refused.
            expect(await rejected(set({ codexSandboxMode: "maybe" }))).toMatch(/codexSandboxMode must be/);
            // A probe that says works accepts "Always". A missing argument and null change nothing.
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            expect(await set({ codexSandboxMode: "on" })).toMatchObject({ codexSandboxMode: "on", codexSandboxEffective: "on", codexSandbox: true });
            expect((await set()).codexSandboxMode).toBe("on");
            expect((await set({ codexSandbox: null, codexSandboxMode: null })).codexSandboxMode).toBe("on");
            // The machine fails: "Always" is "refused" in effect, and the old field is false.
            g.__towerMockConfig = { sandboxProbe: { state: "fails" } };
            await probe();
            expect(await get()).toMatchObject({ codexSandboxMode: "on", codexSandboxEffective: "refused", codexSandbox: false });
            // "Never" and "Automatic" are always allowed. The old boolean false is "Never".
            expect(await set({ codexSandbox: false })).toMatchObject({ codexSandboxMode: "off", codexSandboxEffective: "off", codexSandbox: false });
            expect((await set({ codexSandboxMode: "auto" })).codexSandboxMode).toBe("auto");
            // "Never" keeps the effective state off, also when the probe works. The new argument wins over the old one.
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            await probe();
            expect(await set({ codexSandboxMode: "off" })).toMatchObject({ codexSandboxEffective: "off", codexSandbox: false });
            expect(await set({ codexSandbox: false, codexSandboxMode: "auto" })).toMatchObject({ codexSandboxMode: "auto", codexSandboxEffective: "on" });
        } finally {
            delete g.__towerMockConfig;
        }
    });

    it("refuses the start of a Codex employee only when the mode is Always and the probe does not say works", async () => {
        const g = globalThis as Record<string, unknown>;
        const b = createMockBackend();
        const set = (args: Record<string, unknown>) => b.invoke<Settings>("settings_set", { openaiKey: null, imageModel: null, ...args });
        const start = (slug: string) => b.invoke("session_start", { path: FLOOR, slug, firstMessage: null, rows: 24, cols: 80 });
        const stop = async (slug: string) => {
            const view = await b.invoke<{ sessions: { slug: string; sessionId: string }[] }>("floor_open", { path: FLOOR });
            const live = view.sessions.find((x) => x.slug === slug);
            if (live) await b.invoke("session_stop", { sessionId: live.sessionId });
        };
        const starts = () => control().codexStarts.map((x) => `${x.slug}:${x.sandbox}`);
        try {
            // `eng-sam` has a start that fails once for the demo. Use another employee as the Codex one.
            control().patchFloor(FLOOR, { employee: { slug: "eng-lin", patch: { cli: "codex" } } });
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            await set({ codexSandboxMode: "on" });
            // The machine changes after "Always" was chosen. The stored value stays "on".
            g.__towerMockConfig = { sandboxProbe: { state: "fails", detail: "bwrap: no way" } };
            const refused = await rejected(start("eng-lin"));
            expect(refused).toBe(
                'SANDBOX_DOWN: Lin did not start: the Codex sandbox does not work on this machine now (bwrap: no way). The setting "Codex sandbox" is "Always". Repair the sandbox, or choose "Automatic" or "Never" in Settings > Codex sandbox.',
            );
            expect((await b.invoke<Settings>("settings_get")).codexSandboxMode).toBe("on");
            expect(starts()).toEqual([]);
            // A Claude employee is not affected.
            await start("pm");
            // "Automatic" with the same failing probe: the start works with full access.
            await set({ codexSandboxMode: "auto" });
            await start("eng-lin");
            expect(starts()).toEqual(["eng-lin:false"]);
            // "Automatic" with a probe that works: the start gets the sandbox.
            await stop("eng-lin");
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            await start("eng-lin");
            expect(starts()).toEqual(["eng-lin:false", "eng-lin:true"]);
            // "Never": no sandbox, also when the probe works.
            await stop("eng-lin");
            await set({ codexSandboxMode: "off" });
            await start("eng-lin");
            expect(starts()).toEqual(["eng-lin:false", "eng-lin:true", "eng-lin:false"]);
        } finally {
            delete g.__towerMockConfig;
        }
    });

    it("sends one information notice in a run when Automatic starts Codex with full access", async () => {
        const g = globalThis as Record<string, unknown>;
        const b = createMockBackend();
        const notices: { kind: string; text: string }[] = [];
        b.listen("notice", (payload) => notices.push(payload as { kind: string; text: string }));
        const start = (slug: string) => b.invoke("session_start", { path: FLOOR, slug, firstMessage: null, rows: 24, cols: 80 });
        try {
            control().patchFloor(FLOOR, { employee: { slug: "eng-lin", patch: { cli: "codex" } } });
            control().patchFloor(FLOOR, { employee: { slug: "pm", patch: { cli: "codex" } } });
            g.__towerMockConfig = { sandboxProbe: { state: "works" } };
            await start("eng-lin");
            await new Promise((r) => setTimeout(r, 20));
            expect(notices).toEqual([]);
            g.__towerMockConfig = { sandboxProbe: { state: "fails" } };
            await start("pm");
            // A second Codex start in the same run, with the same failing probe: no second notice.
            const view = await b.invoke<{ sessions: { slug: string; sessionId: string }[] }>("floor_open", { path: FLOOR });
            await b.invoke("session_stop", { sessionId: view.sessions.find((x) => x.slug === "eng-lin")!.sessionId });
            await start("eng-lin");
            await new Promise((r) => setTimeout(r, 20));
            expect(notices).toEqual([
                {
                    path: FLOOR,
                    kind: "sandbox_auto_off",
                    text: "The Codex sandbox does not work on this machine, so Codex employees run with full access. See Settings > Codex sandbox.",
                },
            ]);
        } finally {
            delete g.__towerMockConfig;
        }
    });

    it("sends codexSandbox and codexSandboxMode in settings_set only when they have a value", () => {
        expect(settingsSetArgs(null, null, false, null)).toEqual({ openaiKey: null, imageModel: null, notifyOnQuestion: false });
        expect(Object.keys(settingsSetArgs(null, null, null, null))).not.toContain("codexSandbox");
        expect(Object.keys(settingsSetArgs(null, null, null, null, null))).not.toContain("codexSandboxMode");
        expect(settingsSetArgs(null, null, null, true)).toEqual({ openaiKey: null, imageModel: null, notifyOnQuestion: null, codexSandbox: true });
        expect(settingsSetArgs(null, null, null, false)).toMatchObject({ codexSandbox: false });
        expect(settingsSetArgs(null, null, null, null, "auto")).toEqual({ openaiKey: null, imageModel: null, notifyOnQuestion: null, codexSandboxMode: "auto" });
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
