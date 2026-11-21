// Text for the operator. The Rust side gives errors like "NO_KEY: no OpenAI key is known" or
// "I/O error on /x/.swarm: Permission denied (os error 13)". The UI shows them with no code prefix,
// with a capital letter and a full stop, and with a hint about what to do when it knows one.
// The raw text stays for the code that tests it (`startsWith("NO_KEY:")`). Only the display changes.

const CODE_PREFIX = /^(?:NO_FLOOR|NO_KEY|BUSY|REFUSED|NOT_FOUND|INVALID)\s*:\s*/;

/** Where an error comes from, so that the toast can offer a button for it. */
export type ErrorAction = "settings-programs" | null;

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
    if (/Permission denied|os error 13/i.test(text)) hints.push("You cannot write in this folder. Choose a folder that you own.");
    else if (/does not exist or is not a folder/i.test(text)) hints.push("Check the path, and choose a folder that exists.");
    if (programProblem(text) && !/Settings|Programs/i.test(text)) hints.push("Change the program path in Settings > Programs.");
    text = text.charAt(0).toUpperCase() + text.slice(1);
    if (!/[.!?]$/.test(text)) text += ".";
    return hints.length > 0 ? `${text} ${hints.join(" ")}` : text;
}

/** The error that the toast shows, and the action that it offers. */
export function describeError(raw: string): { text: string; action: ErrorAction } {
    return { text: tidyError(raw), action: programProblem(raw) ? "settings-programs" : null };
}

/** A broken employee file: "/home/x/.swarm/employees/broken.md: line 1, column 15: a quote is missing". */
export function splitFileError(text: string): { file: string; folder: string; message: string } {
    const m = /^(.*?)([^/\\\s:]+\.(?:md|toml))\s*:\s*(.*)$/s.exec(text.trim());
    if (!m) return { file: "", folder: "", message: text.trim() };
    return { file: m[2], folder: m[1].replace(/[/\\]+$/, ""), message: m[3].trim() };
}
