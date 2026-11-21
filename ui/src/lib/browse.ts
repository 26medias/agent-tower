// The "Browse..." button of the two "open folder" fields (docs/ipc-final.md). It opens the
// system folder dialog (the Tauri plugin `dialog`, in directory mode) and gives back the chosen
// path. It never opens the floor: the operator presses "open".
import { isTauri } from "./ipc";

/** What the plugin function `open` needs and gives. A test passes its own. */
export type OpenDialog = (options: {
    directory: boolean;
    multiple: boolean;
    title: string;
    defaultPath?: string;
}) => Promise<string | string[] | null>;

/** Why the button does nothing in a browser. It is the `title` of the disabled button. */
export const BROWSE_MOCK_TITLE = "The folder dialog needs the desktop app. A browser cannot open it. Type the path.";

/**
 * Only for the e2e check of the page: a page that has no Tauri can set `globalThis.__towerMockBrowse`
 * (a function that gives a path). The button then works, with that function as the dialog.
 */
type MockBrowse = (start: string | null) => string | string[] | null | Promise<string | string[] | null>;

function mockBrowse(): MockBrowse | null {
    const hook = (globalThis as { __towerMockBrowse?: unknown }).__towerMockBrowse;
    return typeof hook === "function" ? (hook as MockBrowse) : null;
}

/** The button works only inside Tauri (the mock has no system dialog), or with the hook of the e2e check. */
export function canBrowse(): boolean {
    return isTauri() || mockBrowse() !== null;
}

/** The path that the dialog gave, or null if the operator cancelled. */
export function pickedPath(result: string | string[] | null): string | null {
    const first = Array.isArray(result) ? result[0] : result;
    return typeof first === "string" && first.trim() !== "" ? first : null;
}

/** The parent folder of an absolute path: "/home/me/work/app" gives "/home/me/work". `null` for "/" and for a path with no slash. */
export function parentFolder(path: string): string | null {
    const trimmed = path.replace(/\/+$/, "");
    const cut = trimmed.lastIndexOf("/");
    if (cut < 0) return null;
    return cut === 0 ? (trimmed === "" ? null : "/") : trimmed.slice(0, cut);
}

/**
 * The folder where the dialog opens: the parent of the last floor of the list that is not missing (the floor
 * that the operator added last), or the home folder when there is no such floor. `null`: the dialog chooses.
 */
export function browseStart(floors: readonly { path: string; missing: boolean }[], home: string | null): string | null {
    for (let i = floors.length - 1; i >= 0; i--) {
        if (floors[i].missing) continue;
        const parent = parentFolder(floors[i].path);
        if (parent !== null) return parent;
    }
    return home !== null && home.trim() !== "" ? home : null;
}

/** Open the dialog. Returns the chosen folder, or null on cancel. `start` is the folder where it opens. */
export async function browseFolder(open?: OpenDialog, start: string | null = null): Promise<string | null> {
    const mock = mockBrowse();
    if (!open && !isTauri() && mock) return pickedPath(await mock(start));
    const dialog = open ?? (await import("@tauri-apps/plugin-dialog")).open;
    const options = { directory: true, multiple: false, title: "Choose the project folder" };
    return pickedPath(await dialog(start === null ? options : { ...options, defaultPath: start }));
}
