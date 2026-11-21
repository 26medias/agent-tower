import { describe, expect, it } from "vitest";
import { ALLOW_RULES_MAX, EXAMPLE_RULES, addRule, checkAllowRule, removeRule } from "./permissions";

describe("checkAllowRule", () => {
    it("accepts the good forms", () => {
        for (const rule of [
            "Bash(cargo test:*)",
            "Bash(git status:*)",
            "Bash(git diff)",
            "Bash(npm run build)",
            "Bash(./scripts/check.sh)",
            "Bash(g++ -v)",
            "Read(./src/**)",
            "Glob(**/*.rs)",
            "Grep(src/lib)",
            "WebFetch(domain:example.com)",
            "WebSearch",
            `Bash(x${"y".repeat(190)})`,
        ]) {
            expect(checkAllowRule(rule), rule).toBeNull();
        }
    });

    it("refuses each bad form with a reason", () => {
        const cases: [string, RegExp][] = [
            ["", /empty/],
            ["Bash", /no pattern/],
            ["Read", /no pattern/],
            ["Glob", /no pattern/],
            ["Grep", /no pattern/],
            ["WebFetch", /no pattern/],
            ["WebSearch(rust)", /no pattern/],
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
            ["Bash(a\nb)", /printable ASCII/],
            ["Bash(a\u0007b)", /printable ASCII/],
            ["Bash(caf\u00e9)", /printable ASCII/],
            [`Bash(${"x".repeat(200)})`, /too long/],
            ["Bash(echo (hi))", /\( or a \)/],
            ["Bash( ls:*)", /start with a space/],
            ["Bash(*.sh)", /first word/],
            ["Bash(ls;rm:*)", /first word/],
            ["Bash(sh -c x)", /"sh" is not allowed/],
            ["Bash(rm:*)", /"rm" is not allowed/],
            ["Bash(sudo ls)", /"sudo" is not allowed/],
            ["Bash(python3:*)", /"python3" is not allowed/],
            ["Bash(git:*)", /needs a subcommand/],
            ["Bash(git)", /needs a subcommand/],
            ["Bash(git push:*)", /"git push" is not allowed/],
            ["Bash(git checkout main)", /"git checkout" is not allowed/],
            ["Bash(git config:*)", /"git config" is not allowed/],
            ["Read(/etc/passwd)", /relative/],
            ["Read(~/x)", /relative/],
            ["Read(../x)", /\.\./],
            ["Glob(src/../x)", /\.\./],
            ["Grep(a\\..\\b)", /\.\./],
            ["WebFetch(example.com)", /domain:/],
            ["WebFetch(domain:*.example.com)", /\*/],
            ["WebFetch(domain:localhost)", /dot/],
            ["WebFetch(domain:)", /empty/],
        ];
        for (const [rule, reason] of cases) {
            const error = checkAllowRule(rule);
            expect(error, JSON.stringify(rule)).not.toBeNull();
            expect(error, JSON.stringify(rule)).toMatch(reason);
        }
    });

    it("keeps a word that only starts like a blocked program", () => {
        expect(checkAllowRule("Bash(shellcheck x)")).toBeNull();
        expect(checkAllowRule("Bash(nodejs-lint)")).toBeNull();
        expect(checkAllowRule("Bash(git status)")).toBeNull();
        expect(checkAllowRule("Bash(gitk)")).toBeNull();
    });

    it("accepts every example rule", () => {
        for (const rule of EXAMPLE_RULES) expect(checkAllowRule(rule), rule).toBeNull();
    });
});

describe("addRule and removeRule", () => {
    it("adds a trimmed rule at the end", () => {
        const first = addRule([], "  Bash(ls:*) ");
        expect(first).toEqual({ list: ["Bash(ls:*)"], error: null });
        expect(addRule(first.list, "Read(./src/**)").list).toEqual(["Bash(ls:*)", "Read(./src/**)"]);
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
        expect(addRule(full, "Read(./a)").error).toMatch(/limit/);
    });

    it("removes one rule", () => {
        expect(removeRule(["a", "b", "c"], "b")).toEqual(["a", "c"]);
        expect(removeRule(["a"], "z")).toEqual(["a"]);
    });
});
