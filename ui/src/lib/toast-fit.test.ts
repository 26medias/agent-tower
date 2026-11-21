import { describe, expect, it } from "vitest";
import { toastsThatFit } from "./toast-fit";

const base = { gap: 6, moreHeight: 22, max: 4 };

describe("toastsThatFit", () => {
    it("shows no toast when there is none", () => {
        expect(toastsThatFit({ ...base, available: 251, heights: [], total: 0 })).toBe(0);
    });

    it("shows all four when they fit and none waits", () => {
        // 4 x 40 + 3 x 6 = 178
        expect(toastsThatFit({ ...base, available: 251, heights: [40, 40, 40, 40], total: 4 })).toBe(4);
    });

    it("cuts the third folded toast of 85 px at 251 px (the case of 1280x800)", () => {
        // 3 x 85 + 2 x 6 = 267 > 251, so only 2 show. 2 x 85 + 6 + 6 + 22 = 204 fits with the line.
        expect(toastsThatFit({ ...base, available: 251, heights: [85, 85, 85, 85], total: 4 })).toBe(2);
    });

    it("shows all four 85 px toasts when the stack is tall enough (the case of 2560x1400)", () => {
        // 4 x 85 + 3 x 6 = 358
        expect(toastsThatFit({ ...base, available: 400, heights: [85, 85, 85, 85], total: 4 })).toBe(4);
    });

    it("keeps room for the line of the older toasts", () => {
        // 3 x 85 + 2 x 6 = 267 fits in 270, but with 5 toasts the line needs 6 + 22 more: 295.
        expect(toastsThatFit({ ...base, available: 270, heights: [85, 85, 85, 85], total: 5 })).toBe(2);
        expect(toastsThatFit({ ...base, available: 270, heights: [85, 85, 85, 85], total: 3 })).toBe(3);
    });

    it("uses the real height of each toast: a toast with a button can be taller", () => {
        expect(toastsThatFit({ ...base, available: 251, heights: [60, 60, 60, 60], total: 4 })).toBe(3);
        expect(toastsThatFit({ ...base, available: 251, heights: [60, 100, 60, 60], total: 4 })).toBe(2);
    });

    it("shows one toast even when it does not fit", () => {
        expect(toastsThatFit({ ...base, available: 50, heights: [85, 85], total: 2 })).toBe(1);
        expect(toastsThatFit({ ...base, available: 0, heights: [85], total: 1 })).toBe(1);
    });

    it("never shows more than max, and counts only the heights that it has", () => {
        expect(toastsThatFit({ ...base, available: 1000, heights: [40, 40, 40, 40, 40, 40], total: 6 })).toBe(4);
        expect(toastsThatFit({ ...base, available: 1000, heights: [40, 40], total: 6 })).toBe(2);
    });

    it("does not count a toast of a height that is not known yet", () => {
        expect(toastsThatFit({ ...base, available: 1000, heights: [40, Infinity, 40], total: 3 })).toBe(1);
    });
});
