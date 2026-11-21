// Pure logic for the section "Commands with no question" of the settings panel (docs/ipc-m3.md).
// It repeats the check of `check_allow_rule` in crates/core/src/model.rs, so that a bad rule is
// refused before the call. The Rust side checks again and stays the judge: when it refuses a rule,
// the form shows its text.
//
// The rules (the second version of the server check):
//   - only printable ASCII;
//   - the form is exactly `Name(pattern)`, and the pattern has no `(` or `)`;
//   - `Bash`: the pattern does not start with a space. The first word (up to the first space or `:`)
//     has only A-Z a-z 0-9 . _ / + - and is not a shell, an interpreter or a risky program.
//     `git` needs a subcommand that is not push, reset, clean, checkout or config;
//   - `Read`, `Glob`, `Grep`: a relative pattern with no `..`. A bare name is refused;
//   - `WebFetch`: only `WebFetch(domain:<host>)`, with a dot and no `*` in the host;
//   - `WebSearch`: the bare name only.

export const ALLOW_RULE_MAX_CHARS = 200;
export const ALLOW_RULES_MAX = 100;

const RULE_TOOLS = ["Bash", "Read", "Glob", "Grep", "WebFetch", "WebSearch"];
const PATH_TOOLS = ["Read", "Glob", "Grep"];

/** A program that can run other programs, or that can change or delete too much. */
export const BLOCKED_COMMANDS: readonly string[] = [
    "sh", "bash", "zsh", "dash", "fish", "env", "xargs", "eval", "exec", "sudo", "su", "doas", "nohup", "time",
    "timeout", "nice", "python", "python3", "node", "perl", "ruby", "php", "rm", "dd", "mkfs", "chmod", "chown",
    "curl", "wget", "ssh", "scp", "nc",
];

/** The git subcommands that a rule cannot name: they change history, files or the config. */
export const BLOCKED_GIT: readonly string[] = ["push", "reset", "clean", "checkout", "config"];

const FORM = "Write a rule as Tool(pattern), for example Bash(cargo test:*). The tools are Bash, Read, Glob, Grep and WebFetch. WebSearch has no pattern.";

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

function checkBash(rule: string, pattern: string): string | null {
    if (/^[*:]+$/.test(pattern)) {
        return `"${rule}" would allow every shell command. Name a command, for example Bash(cargo test:*).`;
    }
    if (pattern.startsWith(" ")) return "The pattern of a Bash rule must not start with a space.";
    const first = pattern.split(/[ :]/, 1)[0];
    if (first === "" || !/^[A-Za-z0-9._/+-]+$/.test(first)) {
        return `The first word of a Bash rule, "${first}", may have only letters, digits and . _ / + - . Name the command first, for example Bash(cargo test:*).`;
    }
    if (BLOCKED_COMMANDS.includes(first)) {
        return `"${first}" is not allowed in a rule. It can run other programs or change too much.`;
    }
    if (first === "git") {
        const rest = pattern.slice(first.length);
        const sub = rest.startsWith(" ") ? rest.slice(1).split(/[ :]/, 1)[0] : "";
        if (sub === "") return `A git rule needs a subcommand, for example Bash(git status:*).`;
        if (BLOCKED_GIT.includes(sub)) return `"git ${sub}" is not allowed in a rule. It changes history, files or the config.`;
    }
    return null;
}

function checkWebFetch(pattern: string): string | null {
    const form = "Write WebFetch(domain:example.com).";
    if (!pattern.startsWith("domain:")) return `A WebFetch rule must start with domain:. ${form}`;
    const host = pattern.slice("domain:".length);
    if (host === "" || /\s/.test(host)) return `The host of a WebFetch rule is empty or has a space. ${form}`;
    if (host.includes("*")) return `The host of a WebFetch rule must not have a *. ${form}`;
    if (!host.includes(".")) return `The host of a WebFetch rule needs a dot. ${form}`;
    return null;
}

function checkPath(tool: string, pattern: string): string | null {
    if (pattern.startsWith("/") || pattern.startsWith("~") || /^[A-Za-z]:[\\/]/.test(pattern)) {
        return `A ${tool} rule needs a relative pattern, for example ${tool}(./src/**). "${pattern}" is not relative.`;
    }
    if (pattern.split(/[\\/]/).includes("..")) return `A ${tool} rule must not have .. in its pattern.`;
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

/** Rules that the operator can add with one click. Each one passes `checkAllowRule`. */
export const EXAMPLE_RULES: readonly string[] = ["Bash(cargo test:*)", "Bash(npm test:*)", "Bash(ls:*)"];
