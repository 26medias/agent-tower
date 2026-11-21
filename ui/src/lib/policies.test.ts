import { describe, expect, it } from "vitest";
import { POLICY_IDS, POLICY_INFO, defaultManager, policyLabel } from "./policies";

describe("policies", () => {
    it("names and explains each policy", () => {
        expect(POLICY_IDS).toEqual(["read_only", "edit_in_worktree", "ask_always"]);
        expect(policyLabel("read_only")).toBe("Read only");
        expect(policyLabel("edit_in_worktree")).toBe("Edits in its task folder");
        expect(policyLabel("ask_always")).toBe("Asks before each action");
        for (const id of POLICY_IDS) expect(POLICY_INFO[id].help.length).toBeGreaterThan(20);
    });
});

describe("defaultManager", () => {
    it("picks the Project Manager, else the CEO, else nobody", () => {
        const ceo = { slug: "c", role: "CEO" };
        const pm = { slug: "p", role: "Project Manager" };
        const eng = { slug: "e", role: "Engineer" };
        expect(defaultManager([ceo, eng, pm])).toBe("p");
        expect(defaultManager([ceo, eng])).toBe("c");
        expect(defaultManager([eng])).toBe("");
        expect(defaultManager([])).toBe("");
    });
});
