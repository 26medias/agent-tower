import { describe, expect, it } from "vitest";
import { BROWSE_MOCK_TITLE, browseFolder, browseStart, canBrowse, parentFolder, pickedPath } from "./browse";

describe("browse: the folder dialog", () => {
    it("asks for one directory", async () => {
        let seen: unknown = null;
        const path = await browseFolder(async (options) => {
            seen = options;
            return "/home/demo/project";
        });
        expect(path).toBe("/home/demo/project");
        expect(seen).toMatchObject({ directory: true, multiple: false });
    });

    it("gives null when the operator cancels", async () => {
        expect(await browseFolder(async () => null)).toBeNull();
    });

    it("takes the first path of a list and ignores an empty one", () => {
        expect(pickedPath(["/a", "/b"])).toBe("/a");
        expect(pickedPath([])).toBeNull();
        expect(pickedPath("")).toBeNull();
        expect(pickedPath("   ")).toBeNull();
        expect(pickedPath("/a b")).toBe("/a b");
    });

    it("does not work in a browser, and says why", () => {
        // The tests run in node: there is no `__TAURI_INTERNALS__`.
        expect(canBrowse()).toBe(false);
        expect(BROWSE_MOCK_TITLE).toContain("desktop app");
    });
});

describe("browse: where the dialog opens", () => {
    it("is the parent folder of a path", () => {
        expect(parentFolder("/home/me/work/app")).toBe("/home/me/work");
        expect(parentFolder("/home/me/work/app/")).toBe("/home/me/work");
        expect(parentFolder("/app")).toBe("/");
        expect(parentFolder("/")).toBeNull();
        expect(parentFolder("app")).toBeNull();
    });

    it("is the parent of the last floor that is not missing, else the home folder, else nothing", () => {
        const floors = [
            { path: "/home/me/a/one", missing: false },
            { path: "/home/me/b/two", missing: false },
            { path: "/gone/three", missing: true },
        ];
        expect(browseStart(floors, "/home/me")).toBe("/home/me/b");
        expect(browseStart([{ path: "/gone/x", missing: true }], "/home/me")).toBe("/home/me");
        expect(browseStart([], "/home/me")).toBe("/home/me");
        expect(browseStart([], null)).toBeNull();
        expect(browseStart([], "  ")).toBeNull();
    });

    it("gives the folder to the dialog as defaultPath, and gives none when there is none", async () => {
        const seen: unknown[] = [];
        await browseFolder(async (options) => { seen.push(options); return null; }, "/home/me/b");
        await browseFolder(async (options) => { seen.push(options); return null; });
        expect(seen[0]).toMatchObject({ directory: true, defaultPath: "/home/me/b" });
        expect(seen[1]).not.toHaveProperty("defaultPath");
    });
});

describe("browse: the hook of the e2e check", () => {
    it("lets a page with no Tauri give a path, and only then", async () => {
        const g = globalThis as { __towerMockBrowse?: () => string };
        try {
            g.__towerMockBrowse = () => "/home/demo/picked";
            expect(canBrowse()).toBe(true);
            expect(await browseFolder()).toBe("/home/demo/picked");
        } finally {
            delete g.__towerMockBrowse;
        }
        expect(canBrowse()).toBe(false);
    });
});
