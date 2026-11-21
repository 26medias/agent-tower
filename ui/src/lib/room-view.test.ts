import { describe, expect, it } from "vitest";
import { ART } from "./office-layout";
import { ZOOM_MAX, clampView, fullView, panView, zoomView } from "./room-view";

describe("room view", () => {
    it("starts with the whole picture", () => {
        expect(fullView()).toEqual({ x: 0, y: 0, w: ART.w, h: ART.h });
    });

    it("zooms in around a point and keeps that point where it is", () => {
        const at = { x: 1000, y: 600 };
        const v = zoomView(fullView(), 2, at);
        expect(v.w).toBeCloseTo(ART.w / 2);
        expect(v.h).toBeCloseTo(ART.h / 2);
        // The point has the same relative place in the view as before.
        expect((at.x - v.x) / v.w).toBeCloseTo(at.x / ART.w);
        expect((at.y - v.y) / v.h).toBeCloseTo(at.y / ART.h);
    });

    it("never zooms out past the whole picture or in past the limit", () => {
        const out = zoomView(fullView(), 0.5, { x: 100, y: 100 });
        expect(out).toEqual(fullView());
        let v = fullView();
        for (let i = 0; i < 20; i++) v = zoomView(v, 2, { x: 100, y: 100 });
        expect(v.w).toBeCloseTo(ART.w / ZOOM_MAX);
    });

    it("keeps the view inside the picture", () => {
        const v = zoomView(fullView(), 3, { x: ART.w, y: ART.h });
        expect(v.x + v.w).toBeLessThanOrEqual(ART.w + 1e-6);
        expect(v.y + v.h).toBeLessThanOrEqual(ART.h + 1e-6);
        expect(clampView({ x: -50, y: -50, w: 100, h: 100 })).toMatchObject({ x: 0, y: 0 });
    });

    it("moves with a drag and stops at the edge", () => {
        const v = zoomView(fullView(), 2, { x: 768, y: 512 });
        const moved = panView(v, -100, -50);
        expect(moved.x).toBeCloseTo(v.x + 100);
        expect(moved.y).toBeCloseTo(v.y + 50);
        expect(panView(v, 99999, 99999)).toMatchObject({ x: 0, y: 0 });
    });

    it("ignores a bad factor", () => {
        const v = fullView();
        expect(zoomView(v, 0, { x: 1, y: 1 })).toBe(v);
        expect(zoomView(v, NaN, { x: 1, y: 1 })).toBe(v);
    });
});
