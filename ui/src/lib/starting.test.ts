import { describe, expect, it } from "vitest";
import {
    elapsedSeconds,
    formatElapsed,
    startingTitle,
    startingView,
    stripText,
} from "./starting";

describe("elapsedSeconds and formatElapsed", () => {
    it("counts whole seconds", () => {
        expect(elapsedSeconds(1000, 1000)).toBe(0);
        expect(elapsedSeconds(1000, 1999)).toBe(0);
        expect(elapsedSeconds(1000, 2000)).toBe(1);
        expect(elapsedSeconds(0, 34_500)).toBe(34);
    });

    it("never goes below zero", () => {
        expect(elapsedSeconds(5000, 1000)).toBe(0);
        expect(elapsedSeconds(Number.NaN, 1000)).toBe(0);
    });

    it("formats seconds", () => {
        expect(formatElapsed(0)).toBe("0 s");
        expect(formatElapsed(34)).toBe("34 s");
        expect(formatElapsed(75.9)).toBe("75 s");
        expect(formatElapsed(-3)).toBe("0 s");
    });
});

describe("texts", () => {
    it("names the CLI and the employee", () => {
        expect(startingTitle("claude", "Ada")).toBe("Starting claude for Ada...");
        expect(startingTitle("codex", "Setup")).toBe("Starting codex for Setup...");
    });

    it("uses a plain word when the CLI is not known", () => {
        expect(startingTitle(null, "Ada")).toBe("Starting the CLI for Ada...");
    });

    it("writes the strip text", () => {
        expect(stripText(34)).toBe("starting... 34 s");
    });
});

describe("startingView", () => {
    it("shows nothing when the status is not starting", () => {
        for (const status of ["working", "idle", "question", "done", "stopped"] as const) {
            expect(startingView(status, false, 5)).toEqual({ mode: "none", slowHint: false, stuckLine: false });
            expect(startingView(status, true, 200).mode).toBe("none");
        }
    });

    it("shows the centre overlay while starting with no output", () => {
        expect(startingView("starting", false, 0)).toEqual({ mode: "centre", slowHint: false, stuckLine: false });
    });

    it("turns into the strip from the first output on", () => {
        expect(startingView("starting", true, 0).mode).toBe("strip");
        expect(startingView("starting", true, 40).mode).toBe("strip");
    });

    it("adds the slow hint after 10 s, in the centre overlay only", () => {
        expect(startingView("starting", false, 9).slowHint).toBe(false);
        expect(startingView("starting", false, 10).slowHint).toBe(true);
        expect(startingView("starting", true, 15).slowHint).toBe(false);
    });

    it("adds the still-starting line after 90 s, in both modes", () => {
        expect(startingView("starting", false, 89).stuckLine).toBe(false);
        expect(startingView("starting", false, 90).stuckLine).toBe(true);
        expect(startingView("starting", true, 90).stuckLine).toBe(true);
    });
});
