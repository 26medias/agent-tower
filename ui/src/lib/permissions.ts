// Pure logic for the section "Commands with no question" of the settings panel (docs/ipc-m3.md, "The
// check of a rule"). It repeats the check of `check_allow_rule` in crates/core/src/model.rs, so that a
// bad rule is refused before the call. The Rust side checks again and stays the judge: when it
// refuses a rule, the form shows its text. A rule that this file does not know how to judge is not
// refused here: the server decides.
//
// The check is NOT a security boundary. It is informed consent: the operator must know what each
// rule gives. A list of blocked programs can never be complete, so each `Bash` rule with a `*`
// gets a warning (`ruleWarning`). That warning is the real control.
//
// The rules (the fourth version of the server check, 2026-11-21):
//   - only printable ASCII;
//   - the entry is `WebSearch` alone, or `Tool(pattern)` with Tool one of Bash, Read, Glob, Grep,
//     WebFetch, and a pattern with no `(` or `)`;
//   - `Bash`: the pattern has none of the shell characters ; & | < > ` and $ (a rule is one command;
//     this check is before the check of the brackets). The pattern does not start with a space. The
//     first word (up to the first space or `:`) has only A-Z a-z 0-9 . _ / + - . `rtk` is unwrapped first (the check applies to the words after
//     it; `rtk` alone, `rtk` with an option and `rtk proxy` and the like are refused). The program is
//     not on the list of programs that run other programs, change or destroy data, or send data out.
//     `git` rules must be exact (no `*`: a wildcard lets the agent add `--output`, which writes a
//     file). The subcommand is on an allow list, and `--output`, `--exec`, `--ext-diff`, `--textconv`
//     and `-c` are refused.
//     Each other program is allowed. A rule with a `*` gets a warning from the server, a stronger
//     one for the build tools and for a script of the project (`ruleWarning` gives the same texts);
//   - `Read`, `Glob`, `Grep`: a relative pattern with no `..`, and not the folder `.swarm/state`. A
//     pattern with a `*`, `?` or `[` gets a warning from the server (`ruleWarning` gives the same text);
//   - `WebFetch`: only `domain:<host>`, with a dot. No IP address, `localhost`, `.local`, `.internal`.
//     Each WebFetch rule gets a warning from the server: the app does not resolve the host name;
//   - `WebSearch`: the bare name only.

export const ALLOW_RULE_MAX_CHARS = 200;
export const ALLOW_RULES_MAX = 100;

const RULE_TOOLS = ["Bash", "Read", "Glob", "Grep", "WebFetch", "WebSearch"];

/** Programs that run other programs, change or destroy data, or send data out (docs/ipc-m3.md). */
export const BLOCKED_COMMANDS: readonly string[] = [
    // shells and wrappers that run another command
    "sh", "bash", "zsh", "dash", "fish", "ksh", "csh", "tcsh", "pwsh", "powershell", "busybox", "env", "xargs", "eval",
    "exec", "sudo", "su", "doas", "pkexec", "nohup", "time", "timeout", "nice", "setsid", "stdbuf", "flock", "unshare",
    "script", "watch", "parallel", "strace", "ltrace", "gdb", "command", "builtin", "source", ".",
    // interpreters
    "python", "python3", "pypy", "pypy3", "node", "deno", "bun", "lua", "perl", "ruby", "php",
    // programs that run a command or write files
    "find", "awk", "gawk", "mawk", "sed", "tar", "tee", "cp", "mv", "ln", "rm", "dd", "mkfs", "shred", "truncate",
    "unlink", "chmod", "chown", "rsync",
    // programs that send data out
    "curl", "wget", "ssh", "scp", "nc", "ncat", "socat", "telnet", "ftp",
    // containers, editors, system control
    "docker", "podman", "kubectl", "vim", "vi", "nano", "emacs", "kill", "pkill", "systemctl", "mount", "umount",
    "crontab", "at",
    // a few more wrappers
    "ssh-agent", "sshpass", "expect", "chroot", "nsenter", "ionice", "taskset", "numactl", "runuser", "setpriv",
    "start-stop-daemon", "run-parts", "systemd-run", "screen", "tmux",
    // a best effort: programs that open a shell, an editor or a pager, or that run another program.
    // The list is NOT complete. The warning of each wildcard rule is the real control.
    "sqlite3", "psql", "mysql", "man", "less", "more", "nvim", "ex", "ed", "nodejs", "tclsh", "julia", "rscript", "irb",
    "ipython", "osascript", "xdg-open", "uvx", "pipx", "nix-shell", "nix", "direnv", "mise", "chrt", "prlimit", "fakeroot",
    "firejail", "bwrap", "valgrind", "perf", "hyperfine", "install", "patch",
];

/** The only git subcommands that a rule can name. The rule must be exact: with a `*` an agent can add `--output`. */
export const ALLOWED_GIT: readonly string[] = ["status", "log", "diff", "show", "blame", "ls-files", "rev-parse", "describe"];

/** Build and package tools. A rule for one is allowed, and the server adds a warning: they run the code of the project. */
export const BUILD_TOOLS: readonly string[] = [
    "cargo", "npm", "npx", "pnpm", "yarn", "make", "just", "uv", "pip", "go", "gradle", "mvn", "pytest", "tox", "bundle",
    "rake", "composer", "dotnet", "cmake", "ninja", "bazel", "poetry", "meson", "scons",
];

/** The words after `rtk` that run any command or change the settings of `rtk`. */
const RTK_REFUSED: readonly string[] = ["proxy", "run", "err", "test", "summary", "pipe", "rewrite", "hook", "init", "trust", "untrust", "config"];

const FORM = "Write a rule as Tool(pattern), for example Bash(cargo test:*). The tools are Bash, Read, Glob, Grep and WebFetch. WebSearch has no pattern.";

/** The text of a warning of the server for the code of the project, without the rule: `<rule>: <this text>`. */
export const WARNING_TEXT = "This rule lets each agent of the floor run the project's own code with no question.";

/** The text of the warning of the server for a `Bash` rule with a `*`, without the rule. */
export const WILDCARD_WARNING_TEXT =
    "This rule lets each agent of the floor run this program with any arguments and no question. Many programs can write files or run other commands through their options.";

/** The refusal of a `git` rule with a `*`. */
export const GIT_WILDCARD_REFUSAL =
    "A git rule must be the exact command, for example Bash(git status) or Bash(git diff --stat). A rule with * lets an agent add options such as --output, which writes files.";

/** `null` when the rule is good. Else the text for the operator. */
export function checkAllowRule(rule: string): string | null {
    const count = [...rule].length;
    if (count === 0) return "The rule is empty.";
    if (count > ALLOW_RULE_MAX_CHARS) return `The rule is too long: ${count} characters. The limit is ${ALLOW_RULE_MAX_CHARS}.`;
    if (!/^[\x20-\x7e]+$/.test(rule)) return "The rule must use only printable ASCII characters. It has a control character or a letter that is not ASCII.";
    if (rule === "WebSearch") return null;
    if (RULE_TOOLS.includes(rule)) {
        return `"${rule}" with no pattern is not allowed. ${FORM}`;
    }
    const open = rule.indexOf("(");
    if (open < 0 || !rule.endsWith(")")) return `"${rule}" is not a rule. ${FORM}`;
    const tool = rule.slice(0, open);
    const pattern = rule.slice(open + 1, -1);
    if (!RULE_TOOLS.includes(tool)) return `"${tool}" is not a tool that a rule can name. ${FORM}`;
    if (tool === "Bash") {
        // A rule is one command. Shell text in it would hide a program from the checks of `checkBash`.
        // This check is before the check of the brackets, so that `Bash(echo $(rm))` gets this text.
        const shell = /[;&|<>`$]/.exec(pattern);
        if (shell) return `"${rule}" has the shell character ${shell[0]}. A rule is one command: write no ; & | < > \` or $ in it`;
    }
    if (pattern.includes("(") || pattern.includes(")")) return `The pattern of "${rule}" must not have a ( or a ) inside.`;
    if (pattern.trim() === "") return `"${rule}" has an empty pattern.`;
    switch (tool) {
        case "Bash":
            return checkBash(rule, pattern);
        case "WebFetch":
            return checkWebFetch(pattern);
        case "WebSearch":
            return "WebSearch takes no pattern. Write WebSearch alone.";
        default:
            return checkPath(tool, pattern);
    }
}

/** The first word of a Bash pattern: up to the first space or `:`. */
function firstWord(text: string): string {
    return text.split(/[ :]/, 1)[0];
}

/**
 * The name of a program for the lists: no folder in front, no letter case, no version at the end
 * (`/bin/SH` is `sh`, `python3.12` is `python`, `lua5.4` is `lua`).
 */
function programName(word: string): { plain: string; bare: string } {
    const plain = (word.split("/").pop() ?? word).toLowerCase();
    return { plain, bare: plain.replace(/[0-9]+(\.[0-9]+)*$/, "") };
}

function isBlocked(word: string): boolean {
    const { plain, bare } = programName(word);
    return BLOCKED_COMMANDS.includes(plain) || (bare !== "" && BLOCKED_COMMANDS.includes(bare));
}

function isBuildTool(word: string): boolean {
    const { plain, bare } = programName(word);
    return BUILD_TOOLS.includes(plain) || (bare !== "" && BUILD_TOOLS.includes(bare));
}

/**
 * Split a Bash pattern after the unwrap of `rtk`. Returns the words to check, or the refusal text.
 * `rtk` alone, `rtk` with an option and `rtk` with one of the words of RTK_REFUSED are refused.
 */
function unwrapRtk(pattern: string): { rest: string; error: string | null } {
    if (firstWord(pattern) !== "rtk") return { rest: pattern, error: null };
    const after = pattern.slice("rtk".length);
    if (!after.startsWith(" ")) {
        return { rest: "", error: `"rtk" alone is not allowed in a rule: it runs any command. Name the command after it, for example Bash(rtk cargo test:*).` };
    }
    const rest = after.slice(1);
    const next = firstWord(rest);
    if (next === "") return { rest: "", error: `"rtk" alone is not allowed in a rule: it runs any command. Name the command after it, for example Bash(rtk cargo test:*).` };
    if (next.startsWith("-")) return { rest: "", error: `"rtk ${next}" is not allowed in a rule. An option of rtk can change what it runs. Name the command after rtk.` };
    if (RTK_REFUSED.includes(next)) {
        return { rest: "", error: `"rtk ${next}" is not allowed in a rule. It runs any command or it changes the settings of rtk.` };
    }
    return { rest, error: null };
}

function checkBash(rule: string, pattern: string): string | null {
    if (/^[*:]+$/.test(pattern)) {
        return `"${rule}" would allow every shell command. Name a command, for example Bash(cargo test:*).`;
    }
    if (pattern.startsWith(" ")) return "The pattern of a Bash rule must not start with a space.";
    const unwrapped = unwrapRtk(pattern);
    if (unwrapped.error) return unwrapped.error;
    const body = unwrapped.rest;
    const first = firstWord(body);
    if (first === "" || !/^[A-Za-z0-9._/+-]+$/.test(first)) {
        return `The first word of a Bash rule, "${first}", may have only letters, digits and . _ / + - . Name the command first, for example Bash(cargo test:*).`;
    }
    if (isBlocked(first)) {
        return `"${first}" is not allowed in a rule. It can run other programs, change or destroy data, or send data out.`;
    }
    if (programName(first).plain === "git") {
        const rest = body.slice(first.length);
        const sub = rest.startsWith(" ") ? firstWord(rest.slice(1)) : "";
        if (sub === "") return `A git rule needs a subcommand, for example Bash(git status).`;
        if (!ALLOWED_GIT.includes(sub)) {
            return `"git ${sub}" is not allowed in a rule. A rule can name only these git subcommands: ${ALLOWED_GIT.join(", ")}.`;
        }
        for (const word of rest.split(/[ :]/)) {
            if (word === "-c" || ["--output", "--exec", "--ext-diff", "--textconv"].some((option) => word.startsWith(option))) {
                return `"git ${sub} ${word}" is not allowed in a rule. The options -c, --output, --exec, --ext-diff and --textconv can run a program or write a file.`;
            }
        }
        if (pattern.includes("*")) return GIT_WILDCARD_REFUSAL;
    }
    return null;
}

/** The text of the warning of the server for a `Read`, `Glob` or `Grep` rule with a wildcard, without the rule. */
export const PATH_WILDCARD_WARNING_TEXT =
    "This rule lets each agent of the floor read every file that the pattern matches, with no question. A pattern with a wildcard can match more files than you expect.";
/** The text of the warning of the server for each `WebFetch` rule, without the rule. */
export const WEB_FETCH_WARNING_TEXT =
    "This rule lets each agent of the floor get pages from this host with no question. The app does not check where the host name points.";

/**
 * The warning that the server gives for a rule, or `null`. The server is the judge; the mock uses this to
 * give the same warning. One warning at most for each rule, and the stronger one wins:
 *   - `<rule>: <WARNING_TEXT>` for a build tool (also an exact rule, also after `rtk`) and for a script
 *     of the project (a relative path);
 *   - `<rule>: <WILDCARD_WARNING_TEXT>` for each other `Bash` rule with a `*`;
 *   - `<rule>: <PATH_WILDCARD_WARNING_TEXT>` for a `Read`, `Glob` or `Grep` rule with a `*`, `?` or `[`;
 *   - `<rule>: <WEB_FETCH_WARNING_TEXT>` for each `WebFetch` rule.
 * An exact rule of another program has none. A rule that is refused has none.
 */
export function ruleWarning(rule: string): string | null {
    if (checkAllowRule(rule) !== null || !rule.endsWith(")")) return null;
    for (const tool of ["Read(", "Glob(", "Grep("]) {
        if (rule.startsWith(tool)) {
            return /[*?[]/.test(rule.slice(tool.length, -1)) ? `${rule}: ${PATH_WILDCARD_WARNING_TEXT}` : null;
        }
    }
    if (rule.startsWith("WebFetch(")) return `${rule}: ${WEB_FETCH_WARNING_TEXT}`;
    if (!rule.startsWith("Bash(")) return null;
    const pattern = rule.slice("Bash(".length, -1);
    const unwrapped = unwrapRtk(pattern);
    if (unwrapped.error) return null;
    const first = firstWord(unwrapped.rest);
    // A program that a relative path names (./scripts/check.sh) is the code of the project too.
    const relative = first.includes("/") && !first.startsWith("/");
    if (relative || isBuildTool(first)) return `${rule}: ${WARNING_TEXT}`;
    return pattern.includes("*") ? `${rule}: ${WILDCARD_WARNING_TEXT}` : null;
}

function checkWebFetch(pattern: string): string | null {
    const form = "Write WebFetch(domain:example.com).";
    if (!pattern.startsWith("domain:")) return `A WebFetch rule must start with domain:. ${form}`;
    const host = pattern.slice("domain:".length);
    if (host === "" || /\s/.test(host)) return `The host of a WebFetch rule is empty or has a space. ${form}`;
    if (host.includes("*")) return `The host of a WebFetch rule must not have a *. ${form}`;
    if (!/^[A-Za-z0-9.-]+$/.test(host)) return `The host of a WebFetch rule may have only letters, digits, . and - . ${form}`;
    if (!host.includes(".")) return `The host of a WebFetch rule needs a dot. ${form}`;
    const parts = host.split(".");
    if (parts.some((p) => p === "")) return `The host of a WebFetch rule has an empty part. ${form}`;
    if (/^[0-9]/.test(parts[parts.length - 1])) return `The host of a WebFetch rule must not be an IP address. ${form}`;
    const lower = host.toLowerCase();
    if ([".localhost", ".local", ".internal", ".localdomain"].some((end) => lower.endsWith(end))) {
        return `The host of a WebFetch rule must not be a local or internal name. ${form}`;
    }
    return null;
}

function checkPath(tool: string, pattern: string): string | null {
    if (pattern.startsWith("/") || pattern.startsWith("~") || /^[A-Za-z]:[\\/]/.test(pattern)) {
        return `A ${tool} rule needs a relative pattern, for example ${tool}(./src/**). "${pattern}" is not relative.`;
    }
    if (pattern.split(/[\\/]/).includes("..")) return `A ${tool} rule must not have .. in its pattern.`;
    const plain = pattern.startsWith("./") ? pattern.slice(2) : pattern;
    if (plain.startsWith(".swarm/state") || plain.includes("**/.swarm/state") || plain === ".swarm/**" || plain === ".swarm/*") {
        return `A ${tool} rule must not name the folder .swarm/state or all of .swarm/: it has the database and the logs of the floor.`;
    }
    return null;
}

/** Add a rule to a list. A rule that is bad or already in the list leaves the list as it was. */
export function addRule(list: string[], raw: string): { list: string[]; error: string | null } {
    const rule = raw.trim();
    const error = checkAllowRule(rule);
    if (error) return { list, error };
    if (list.includes(rule)) return { list, error: `"${rule}" is in the list already.` };
    if (list.length >= ALLOW_RULES_MAX) return { list, error: `The list has ${ALLOW_RULES_MAX} rules. That is the limit.` };
    return { list: [...list, rule], error: null };
}

export function removeRule(list: string[], rule: string): string[] {
    return list.filter((r) => r !== rule);
}

/** Add rules to a list, skipping those that are in it. The server checks them (and the limit of 100). */
export function mergeRules(list: string[], extra: readonly string[]): string[] {
    return [...list, ...extra.filter((r) => !list.includes(r))];
}

/** Rules that the operator can add with one click. Each one passes `checkAllowRule`. */
export const EXAMPLE_RULES: readonly string[] = ["Bash(cargo test:*)", "Bash(npm test:*)", "Bash(ls:*)"];
