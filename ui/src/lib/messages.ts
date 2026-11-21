// Text for the operator. The Rust side gives errors like "NO_KEY: no OpenAI key is known" or
// "I/O error on /x/.swarm: Permission denied (os error 13)". The UI shows them with no code prefix,
// with a capital letter and a full stop, and with a hint about what to do when it knows one.
// The raw text stays for the code that tests it (`startsWith("NO_KEY:")`). Only the display changes.

const CODE_PREFIX = /^(?:NO_FLOOR|NO_KEY|BUSY|REFUSED|NOT_FOUND|INVALID|NOT_ABSOLUTE|NOT_A_FOLDER|NO_WRITE|FLOOR_GONE|SANDBOX_DOWN)\s*:\s*/;

/**
 * The four errors of `floor_open` and `floor_create` for a path that cannot be a floor (docs/ipc-final.md).
 * The field "open folder" shows their text under the field. They do not offer "create".
 */
const PATH_ERROR = /^(?:NOT_ABSOLUTE|NOT_FOUND|NOT_A_FOLDER|NO_WRITE)\s*:/;

/** True for an error text of a path that cannot be a floor. */
export function isPathError(raw: string): boolean {
    return PATH_ERROR.test(raw.trim());
}

/** True for the error text of a command on a floor whose folder is gone. */
export function isFloorGone(raw: string): boolean {
    return /^FLOOR_GONE\s*:/.test(raw.trim());
}

/** How a toast shows a notice (`notice` event). `stay`: an information toast that the operator must close. */
export interface NoticePlan {
    tone: "error" | "info";
    stay: boolean;
}

/**
 * The kinds that the UI knows (docs/ipc-m3.md and docs/ipc-final.md). A kind that it does not know
 * gives an information toast with the text, so that a new kind of the Rust side is never hidden.
 */
export function noticePlan(kind: string): NoticePlan {
    switch (kind) {
        case "task_blocked":
        case "resume_failed":
        case "floor_gone":
        case "sandbox_down":
            return { tone: "error", stay: true };
        case "sandbox_auto_off":
            // An information toast that stays: with "Automatic" the app started Codex with full access, and the
            // operator should know it. The toast has the button "Open Settings" (the store adds it).
            return { tone: "info", stay: true };
        case "task_done_unmerged":
            // It carries a warning, so it stays until the operator closes it.
            return { tone: "info", stay: true };
        default:
            // "floor_back" and a kind that the UI does not know (also `permissions_restored`: the app removed it).
            return { tone: "info", stay: false };
    }
}

/** Where an error comes from, so that the toast can offer a button for it. */
export type ErrorAction = "settings-programs" | "settings-sandbox" | null;

/**
 * The app refused the start of a Codex employee, because the setting "Codex sandbox" is "Always" and the sandbox does
 * not work now (docs/ipc-final.md, error prefix SANDBOX_DOWN). The fix is in Settings > Codex sandbox.
 */
export function sandboxDown(text: string): boolean {
    return /^\s*SANDBOX_DOWN\s*:/.test(text);
}

/** The program was not found or cannot start: the fix is in Settings > Programs. */
export function programProblem(text: string): boolean {
    return (
        /(Unable to spawn|ENOENT|was not found|No such file or directory)/i.test(text) &&
        /(cannot (start|resume)|Unable to spawn|was not found)/i.test(text)
    );
}

/** One error as text for a person. */
export function tidyError(raw: string): string {
    let text = raw.trim().replace(CODE_PREFIX, "").trim();
    if (text === "") return "Something went wrong.";
    const hints: string[] = [];
    // The sandbox text has its own advice, and its detail ("Permission denied") is not about a folder.
    if (sandboxDown(raw)) return text.charAt(0).toUpperCase() + text.slice(1);
    if (/Permission denied|os error 13/i.test(text)) hints.push("You cannot write in this folder. Choose a folder that you own.");
    else if (/does not exist or is not a folder/i.test(text)) hints.push("Check the path, and choose a folder that exists.");
    if (programProblem(text) && !/Settings|Programs/i.test(text)) hints.push("Change the program path in Settings > Programs.");
    text = text.charAt(0).toUpperCase() + text.slice(1);
    if (!/[.!?]$/.test(text)) text += ".";
    return hints.length > 0 ? `${text} ${hints.join(" ")}` : text;
}

/** The error that the toast shows, and the action that it offers. */
export function describeError(raw: string): { text: string; action: ErrorAction } {
    const action: ErrorAction = sandboxDown(raw) ? "settings-sandbox" : programProblem(raw) ? "settings-programs" : null;
    return { text: tidyError(raw), action };
}

/** A broken employee file: "/home/x/.swarm/employees/broken.md: line 1, column 15: a quote is missing". */
export function splitFileError(text: string): { file: string; folder: string; message: string } {
    const m = /^(.*?)([^/\\\s:]+\.(?:md|toml))\s*:\s*(.*)$/s.exec(text.trim());
    if (!m) return { file: "", folder: "", message: text.trim() };
    return { file: m[2], folder: m[1].replace(/[/\\]+$/, ""), message: m[3].trim() };
}

/**
 * The task of a `task_blocked` notice. The text is `Task T-12 is blocked: <reason>` (docs/ipc-m3.md).
 * `null` when the text has another form.
 */
export function blockedTaskId(text: string): number | null {
    const m = /^Task T-(\d+) is blocked\b/.exec(text.trim());
    return m ? Number(m[1]) : null;
}
