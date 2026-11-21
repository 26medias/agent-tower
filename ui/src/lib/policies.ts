// The words for the three policies of an employee (docs/spec.md, the table of policies).
// The files keep the ids (`read_only`). The UI shows these names and help lines.
import type { Policy } from "./types";

export const POLICY_IDS: Policy[] = ["read_only", "edit_in_worktree", "ask_always"];

export const POLICY_INFO: Record<Policy, { label: string; help: string }> = {
    read_only: {
        label: "Read only",
        help: "Reads the project. Cannot change files, except its own report files.",
    },
    edit_in_worktree: {
        label: "Edits in its task folder",
        help: "Changes files in its own task folder (a git worktree) with no question. Other actions ask you.",
    },
    ask_always: {
        label: "Asks before each action",
        help: "Asks you before each command and each file change. Claude only.",
    },
};

export function policyLabel(policy: Policy): string {
    return POLICY_INFO[policy].label;
}

/** The first Project Manager of a floor, else the CEO: the default manager of a new employee. */
export function defaultManager(employees: readonly { slug: string; role: string }[]): string {
    const by = (role: string) => employees.find((e) => e.role.trim().toLowerCase() === role)?.slug;
    return by("project manager") ?? by("ceo") ?? "";
}
