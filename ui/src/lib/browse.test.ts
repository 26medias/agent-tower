import { describe, expect, it } from "vitest";
import { BROWSE_MOCK_TITLE, browseFolder, canBrowse, pickedPath } from "./browse";

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
