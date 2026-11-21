import { describe, expect, it } from "vitest";
import { createMockBackend, type MockControl } from "./mock";
import type { FloorView } from "./types";

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
    it("floor_permissions_set returns the stored list and one warning for each rule that runs the project code", async () => {
        const b = createMockBackend();
        const saved = await b.invoke<{ allow: string[]; warnings: string[] }>("floor_permissions_set", {
            path: FLOOR,
            allow: ["Bash(rtk cargo test:*)", "Bash(ls:*)", "Bash(rtk cargo test:*)"],
        });
        expect(saved.allow).toEqual(["Bash(rtk cargo test:*)", "Bash(ls:*)"]);
        expect(saved.warnings).toEqual([
            "Bash(rtk cargo test:*): This rule lets each agent of the floor run the project's own code with no question.",
        ]);
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
