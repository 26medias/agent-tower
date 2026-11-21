// Pure logic for the section "Commands with no question" of the settings panel (docs/ipc-m3.md).
// It repeats the check of `check_allow_rule` in crates/core/src/model.rs, so that a bad rule is
// refused before the call. The Rust side checks again.

export const ALLOW_RULE_MAX_CHARS = 200;
export const ALLOW_RULES_MAX = 100;

const RULE_TOOLS = ["Bash", "Read", "Glob", "Grep", "WebFetch", "WebSearch"];
const BARE_RULES = ["Read", "Glob", "Grep"];

// A control character: C0, DEL and C1.
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** `null` when the rule is good. Else the text for the operator. */
export function checkAllowRule(rule: string): string | null {
    const count = [...rule].length;
    if (count === 0) return "The rule is empty.";
    if (count > ALLOW_RULE_MAX_CHARS) return `The rule is too long: ${count} characters. The limit is ${ALLOW_RULE_MAX_CHARS}.`;
    if (CONTROL.test(rule)) return "The rule has a control character.";
    if (BARE_RULES.includes(rule)) return null;
    if (RULE_TOOLS.includes(rule)) {
        return `"${rule}" with no pattern would allow too much. Write a pattern, for example ${rule}(name:*).`;
    }
    const open = rule.indexOf("(");
    const form =
        "Use Read, Glob, Grep, or Tool(pattern) with Tool one of Bash, Read, Glob, Grep, WebFetch, WebSearch.";
    if (open < 0 || !rule.endsWith(")")) return `"${rule}" is not a rule. ${form}`;
    const tool = rule.slice(0, open);
    const pattern = rule.slice(open + 1, -1);
    if (!RULE_TOOLS.includes(tool)) return `"${tool}" is not a tool that a rule can name. ${form}`;
    if (pattern.trim() === "") return `"${rule}" has an empty pattern.`;
    if (tool === "Bash" && /^[*:]+$/.test(pattern)) {
        return `"${rule}" would allow every shell command. Name a command, for example Bash(cargo test:*).`;
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
