// Pure rules for the "starting" state of a session: the text, the elapsed time, and which
// overlay the terminal pane shows. No Svelte, no DOM.
import type { Status } from "./types";

/** After this many seconds the overlay adds a line that says that the load can be slow. */
export const SLOW_HINT_S = 10;
/** After this many seconds the overlay adds the "still starting" line. */
export const STUCK_S = 90;

export const SLOW_HINT_TEXT =
    "The CLI loads its tools and the project. In a large project this can take more than one minute.";
export const STUCK_TEXT = "Still starting. If nothing shows, stop the session and look at the program paths in Settings.";

export type OverlayMode = "none" | "centre" | "strip";

export interface StartingView {
    /** `centre`: no output yet. `strip`: the terminal has output, so the overlay is a small strip. */
    mode: OverlayMode;
    slowHint: boolean;
    stuckLine: boolean;
}

/** Whole seconds from `sinceMs` to `nowMs`. Never negative. */
export function elapsedSeconds(sinceMs: number, nowMs: number): number {
    const s = Math.floor((nowMs - sinceMs) / 1000);
    return Number.isFinite(s) && s > 0 ? s : 0;
}

export function formatElapsed(seconds: number): string {
    return `${Math.max(0, Math.floor(seconds))} s`;
}

/** "Starting claude for Ada..." The CLI name can be unknown. */
export function startingTitle(cli: string | null, name: string): string {
    return `Starting ${cli ?? "the CLI"} for ${name}...`;
}

/** The text of the strip: "starting... 34 s". */
export function stripText(seconds: number): string {
    return `starting... ${formatElapsed(seconds)}`;
}

/**
 * What the pane shows. Only the status `starting` shows anything. The first output byte turns the
 * centre overlay into the strip, so that the overlay never hides output (a trust dialog, for example).
 */
export function startingView(status: Status, hasOutput: boolean, elapsed: number): StartingView {
    if (status !== "starting") return { mode: "none", slowHint: false, stuckLine: false };
    return {
        mode: hasOutput ? "strip" : "centre",
        slowHint: !hasOutput && elapsed >= SLOW_HINT_S,
        stuckLine: elapsed >= STUCK_S,
    };
}
