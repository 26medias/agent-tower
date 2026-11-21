import { describe, expect, it } from "vitest";
import { resumeButton } from "./resume";

describe("resumeButton", () => {
    it("is hidden when no team waits and none resumes", () => {
        expect(resumeButton([], null)).toEqual({ visible: false, text: "", busy: false });
    });

    it("shows the number of employees that wait", () => {
        expect(resumeButton(["ceo", "pm", "eng"], null)).toEqual({
            visible: true,
            text: "Resume the last team (3)",
            busy: false,
        });
    });

    it("shows the progress while the app resumes, also when the list is empty", () => {
        expect(resumeButton([], { done: 1, total: 3 })).toEqual({
            visible: true,
            text: "Resuming the team (1/3)",
            busy: true,
        });
        expect(resumeButton(["pm"], { done: 5, total: 3 }).text).toBe("Resuming the team (3/3)");
    });
});
