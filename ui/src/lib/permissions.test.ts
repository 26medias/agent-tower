import { describe, expect, it } from "vitest";
import { ALLOW_RULES_MAX, EXAMPLE_RULES, WARNING_TEXT, addRule, checkAllowRule, mergeRules, removeRule, ruleWarning } from "./permissions";

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

describe("checkAllowRule: the check of 2026-11-21 (docs/ipc-m3.md)", () => {
    it("unwraps rtk, and refuses rtk alone, rtk with an option and rtk proxy and the like", () => {
        expect(checkAllowRule("Bash(rtk cargo test:*)")).toBeNull();
        expect(checkAllowRule("Bash(rtk git status:*)")).toBeNull();
        expect(checkAllowRule("Bash(rtk:*)")).toMatch(/"rtk" alone/);
        expect(checkAllowRule("Bash(rtk)")).toMatch(/"rtk" alone/);
        expect(checkAllowRule("Bash(rtk -v ls)")).toMatch(/"rtk -v" is not allowed/);
        for (const word of ["proxy", "run", "err", "test", "summary", "pipe", "rewrite", "hook", "init", "trust", "untrust", "config"]) {
            expect(checkAllowRule(`Bash(rtk ${word}:*)`), word).toMatch(new RegExp(`"rtk ${word}" is not allowed`));
        }
        // The check applies to the words after rtk.
        expect(checkAllowRule("Bash(rtk rm -rf x)")).toMatch(/"rm" is not allowed/);
        expect(checkAllowRule("Bash(rtk git push:*)")).toMatch(/"git push" is not allowed/);
    });

    it("refuses the programs of the contract list, with a folder in front, in capitals and with a version", () => {
        for (const program of ["find", "awk", "gawk", "sed", "tar", "cp", "mv", "tee", "ln", "vim", "nano", "docker", "kubectl", "tmux", "screen", "ncat", "socat", "bash", "ksh", "pwsh", "busybox", "xargs", "deno", "bun", "lua", "rsync", "systemd-run", "."]) {
            expect(checkAllowRule(`Bash(${program} x)`), program).toMatch(/is not allowed in a rule/);
        }
        expect(checkAllowRule("Bash(/bin/sh -c x)")).toMatch(/"\/bin\/sh" is not allowed/);
        expect(checkAllowRule("Bash(SH -c x)")).toMatch(/is not allowed/);
        expect(checkAllowRule("Bash(python3.12 x)")).toMatch(/is not allowed/);
        expect(checkAllowRule("Bash(lua5.4 x)")).toMatch(/is not allowed/);
    });

    it("lets the build tools and the other programs pass", () => {
        for (const rule of ["Bash(cargo test:*)", "Bash(npm test:*)", "Bash(make build)", "Bash(pnpm run check)", "Bash(uv run pytest)", "Bash(ls:*)", "Bash(cat README.md)", "Bash(rtk cargo test:*)"]) {
            expect(checkAllowRule(rule), rule).toBeNull();
        }
    });

    it("lets git run only the read-only subcommands, and refuses -c, --output and --exec", () => {
        for (const sub of ["status", "log", "diff", "show", "blame", "ls-files", "rev-parse", "describe"]) {
            expect(checkAllowRule(`Bash(git ${sub}:*)`), sub).toBeNull();
        }
        for (const sub of ["push", "rebase", "commit", "add", "fetch", "branch", "stash", "apply", "reset", "checkout", "config"]) {
            expect(checkAllowRule(`Bash(git ${sub}:*)`), sub).toMatch(new RegExp(`"git ${sub}" is not allowed`));
        }
        expect(checkAllowRule("Bash(git -C x status)")).toMatch(/"git -C" is not allowed/);
        expect(checkAllowRule("Bash(git log --output=x:*)")).toMatch(/--output/);
        expect(checkAllowRule("Bash(git diff --exec=x)")).toMatch(/--exec/);
        expect(checkAllowRule("Bash(git log -c foo=bar)")).toMatch(/-c/);
        expect(checkAllowRule("Bash(git:*)")).toMatch(/needs a subcommand/);
    });

    it("refuses .swarm/state and .swarm/** for Read, Glob and Grep", () => {
        for (const rule of ["Read(.swarm/state)", "Read(./.swarm/state/log)", "Grep(**/.swarm/state/**)", "Glob(.swarm/**)", "Read(.swarm/*)", "Read(./.swarm/**)"]) {
            expect(checkAllowRule(rule), rule).toMatch(/\.swarm/);
        }
        expect(checkAllowRule("Read(./.swarm/employees/**)")).toBeNull();
        expect(checkAllowRule("Read(./src/**)")).toBeNull();
    });

    it("refuses an IP address and a local name for WebFetch", () => {
        for (const host of ["127.0.0.1", "127.1", "0x7f.0.0.1", "10.0.0.5", "app.localhost", "printer.local", "db.internal", "x.localdomain", "App.LOCAL"]) {
            expect(checkAllowRule(`WebFetch(domain:${host})`), host).not.toBeNull();
        }
        expect(checkAllowRule("WebFetch(domain:a..b.com)")).toMatch(/empty part/);
        expect(checkAllowRule("WebFetch(domain:exa_mple.com)")).toMatch(/only letters/);
        expect(checkAllowRule("WebFetch(domain:docs.rs)")).toBeNull();
        expect(checkAllowRule("WebFetch(domain:api.example.co.uk)")).toBeNull();
    });
});

describe("ruleWarning", () => {
    it("warns for a build tool, also after rtk, and for a program that a relative path names", () => {
        for (const rule of ["Bash(cargo test:*)", "Bash(rtk cargo test:*)", "Bash(npm test:*)", "Bash(make:*)", "Bash(./scripts/check.sh)", "Bash(pip3 install x)"]) {
            expect(ruleWarning(rule), rule).toBe(`${rule}: ${WARNING_TEXT}`);
        }
    });

    it("gives no warning for the other rules, and none for a rule that is refused", () => {
        for (const rule of ["Bash(ls:*)", "Bash(git status:*)", "Read(./src/**)", "WebSearch", "Bash(rtk:*)", "Bash(sh x)", "Bash(/usr/bin/ls)"]) {
            expect(ruleWarning(rule), rule).toBeNull();
        }
    });
});

describe("mergeRules", () => {
    it("adds the rules that are not in the list yet", () => {
        expect(mergeRules(["a", "b"], ["b", "c"])).toEqual(["a", "b", "c"]);
        expect(mergeRules([], ["x"])).toEqual(["x"]);
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
