import { describe, expect, it } from "vitest";
import { ALLOW_RULES_MAX, addRule, checkAllowRule, removeRule } from "./permissions";

describe("checkAllowRule", () => {
    it("accepts the good forms", () => {
        for (const rule of [
            "Bash(cargo test:*)",
            "Bash(git status:*)",
            "Bash(npm run build)",
            "Read",
            "Glob",
            "Grep",
            "Read(./src/**)",
            "Glob(**/*.rs)",
            "WebFetch(domain:example.com)",
            "WebSearch(rust)",
            "Bash(*.sh)",
            `Bash(${"x".repeat(191)})`,
        ]) {
            expect(checkAllowRule(rule), rule).toBeNull();
        }
    });

    it("refuses each bad form with a reason", () => {
        const cases: [string, RegExp][] = [
            ["", /empty/],
            ["Bash", /no pattern/],
            ["WebFetch", /no pattern/],
            ["WebSearch", /no pattern/],
            ["Bash(*)", /every shell command/],
            ["Bash(:*)", /every shell command/],
            ["Bash(**)", /every shell command/],
            ["Bash(*:*)", /every shell command/],
            ["Bash()", /empty pattern/],
            ["Read()", /empty pattern/],
            ["Edit(./x)", /not a tool/],
            ["Write", /not a rule/],
            ["Bash(cargo test:*", /not a rule/],
            ["bash(ls)", /not a tool/],
            ["Bash(a\nb)", /control character/],
            ["Bash(a\u0007b)", /control character/],
            ["Bash(a\u0000b)", /control character/],
            ["Bash(a\u009fb)", /control character/],
            [`Bash(${"x".repeat(200)})`, /too long/],
        ];
        for (const [rule, reason] of cases) {
            const error = checkAllowRule(rule);
            expect(error, JSON.stringify(rule)).not.toBeNull();
            expect(error, JSON.stringify(rule)).toMatch(reason);
        }
    });
});

describe("addRule and removeRule", () => {
    it("adds a trimmed rule at the end", () => {
        const first = addRule([], "  Bash(ls:*) ");
        expect(first).toEqual({ list: ["Bash(ls:*)"], error: null });
        expect(addRule(first.list, "Read").list).toEqual(["Bash(ls:*)", "Read"]);
    });

    it("keeps the list when the rule is bad, a duplicate, or one too many", () => {
        const list = ["Bash(ls:*)"];
        const bad = addRule(list, "Bash(*)");
        expect(bad.list).toBe(list);
        expect(bad.error).toMatch(/every shell command/);
        const twice = addRule(list, "Bash(ls:*)");
        expect(twice.list).toBe(list);
        expect(twice.error).toMatch(/already/);
        const full = Array.from({ length: ALLOW_RULES_MAX }, (_, i) => `Bash(cmd${i}:*)`);
        expect(addRule(full, "Read").error).toMatch(/limit/);
    });

    it("removes one rule", () => {
        expect(removeRule(["a", "b", "c"], "b")).toEqual(["a", "c"]);
        expect(removeRule(["a"], "z")).toEqual(["a"]);
    });
});
