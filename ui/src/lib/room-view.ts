// The part of the room picture that shows, for the zoom of the room. Pure logic: no Svelte, no DOM.
import { ART } from "./office-layout";

export interface View {
    x: number;
    y: number;
    w: number;
    h: number;
}

/** The most that the room zooms in: the view is 1/4 of the picture. */
export const ZOOM_MAX = 4;

/** The whole picture. */
export function fullView(): View {
    return { x: 0, y: 0, w: ART.w, h: ART.h };
}

/** Keep a view of the same size inside the picture. */
export function clampView(v: View): View {
    return {
        ...v,
        x: Math.min(Math.max(v.x, 0), ART.w - v.w),
        y: Math.min(Math.max(v.y, 0), ART.h - v.h),
    };
}

/**
 * Zoom by `factor` (above 1 zooms in) around the point `at` (SVG units). The point stays where it
 * is on the screen. The zoom is from 1 (the whole room) to `ZOOM_MAX`.
 */
export function zoomView(view: View, factor: number, at: { x: number; y: number }): View {
    if (!(factor > 0) || !Number.isFinite(factor)) return view;
    const w = Math.min(ART.w, Math.max(ART.w / ZOOM_MAX, view.w / factor));
    const h = (w * ART.h) / ART.w;
    const ratio = w / view.w;
    return clampView({ x: at.x - (at.x - view.x) * ratio, y: at.y - (at.y - view.y) * ratio, w, h });
}

/** Move the view by `dx`, `dy` SVG units to the left and up, as a drag does. */
export function panView(view: View, dx: number, dy: number): View {
    return clampView({ ...view, x: view.x - dx, y: view.y - dy });
}
