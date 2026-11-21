// Pure logic for the button "Resume the last team" in the office header (docs/ipc-m3.md).
import type { ResumeProgress } from "./types";

export interface ResumeButton {
    /** The button is in the header. */
    visible: boolean;
    text: string;
    /** The app resumes the team now. The button shows the progress and does nothing. */
    busy: boolean;
}

/** The text and the state of the button. It shows only when a team waits or a resume runs. */
export function resumeButton(resumable: string[], progress: ResumeProgress | null): ResumeButton {
    if (progress !== null) {
        const done = Math.min(progress.done, progress.total);
        return { visible: true, text: `Resuming the team (${done}/${progress.total})`, busy: true };
    }
    if (resumable.length === 0) return { visible: false, text: "", busy: false };
    return { visible: true, text: `Resume the last team (${resumable.length})`, busy: false };
}
