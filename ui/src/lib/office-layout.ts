// Pure layout logic of the building and the office. No Svelte, no DOM.
// The numbers and the formulas come from the script of prototype/building.html. They are
// measured on the 1536 x 1024 art images. Do not change them without looking at the art.
import type { Status } from "./types";

/** Size of the art images in the SVG coordinate system. */
export const ART = { w: 1536, h: 1024 } as const;

/** The tower floor. `h`: step between two storeys. `cx`, `cy`, `slope`, `sw`: the sticker on the wall. */
export const T = { h: 300, slope: 0.404, cx: 1098, cy: 689, sw: 170 } as const;

/** The floor of the empty room as a diamond: back, right and left corners. */
export const ROOM = { back: [768, 345], right: [1430, 645], left: [105, 645] } as const;

/**
 * Points on the 1024 x 1024 desk image: the centre of the desk (`ax`, `ay`) and the place where
 * the agent stands, behind the desk (`fx`, `fy`). `agent` is the agent height as a part of the desk size.
 */
export const DESK = { size: 1024, ax: 510, ay: 560, fx: 610, fy: 600, agent: 0.47 } as const;

/** Colour of each status (night palette). The same values are in `app.css` as `--st-*`. */
export const STATUS_COLOR: Record<Status, string> = {
    starting: "#93a2c2",
    working: "#5aa9f2",
    idle: "#6fd08c",
    question: "#ffc56b",
    done: "#5ee0c0",
    stopped: "#6b7896",
};

/** Glyph in the name tag. `working` and `starting` show three dots instead. */
export const STATUS_GLYPH: Record<Status, string> = {
    starting: "",
    working: "",
    idle: "z",
    question: "?",
    done: "✓",
    stopped: "■",
};

// ---- the building ----

/** Top of storey `index` (0 = first floor, at the bottom) when the building has `count` storeys. */
export function storeyY(index: number, count: number): number {
    return (count - 1 - index) * T.h;
}

/** Height of the building in SVG units. */
export function buildingHeight(count: number): number {
    return ART.h + (Math.max(1, count) - 1) * T.h;
}

/** The transform that puts a sticker (a box from -sw/2 to sw/2) on the wall of a storey. */
export function stickerTransform(y: number): string {
    return `matrix(1 ${-T.slope} 0 1 ${T.cx} ${T.cy + y})`;
}

// ---- the name plate of a storey ----

/** The most characters of a floor name on the plate of its storey. A longer name is cut with an ellipsis. */
export const PLATE_MAX = 10;

/** The text on the plate of a storey: the floor name, cut. */
export function plateText(name: string): string {
    const clean = name.trim();
    const chars = [...clean];
    return chars.length > PLATE_MAX ? chars.slice(0, PLATE_MAX - 1).join("") + "…" : clean;
}

/**
 * The plate sits under the sticker, on the same wall plane (it uses the transform of the sticker).
 * `top`: where the plate starts under the middle of the sticker. `font` and `height`: the text size
 * and the plate height. All in SVG units. At the natural size of the building (about 0.18 px for
 * one unit) the text is about 10 px high.
 */
export const PLATE = { top: 104, font: 56, height: 72, char: 35, pad: 30 } as const;

/** Width of the plate for a text. */
export function plateWidth(text: string): number {
    return [...text].length * PLATE.char + PLATE.pad;
}

/** The badge of a floor with questions, in SVG units. The radius gives about 25 px at the natural size. */
export const BADGE = { r: 72, font: 84 } as const;

// ---- the office ----

export interface Grid {
    cols: number;
    rows: number;
    /** Scale of the desk image (1 = 1024 units). */
    k: number;
}

/** The grid (columns along the right wall, rows along the left wall) that gives the largest desks. */
export function gridFor(count: number): Grid {
    const n = Math.max(1, count);
    let best: Grid | null = null;
    for (let cols = 1; cols <= n; cols++) {
        const rows = Math.ceil(n / cols);
        const k = Math.min(0.92 / (1.2 * cols), 0.92 / (0.85 * rows), 0.36);
        if (!best || k > best.k + 1e-9) best = { cols, rows, k };
    }
    return best as Grid;
}

export type Point = readonly [number, number];

/** A point on the floor diamond. `u` runs along the right wall, `v` along the left wall (0 to 1). */
export function floorPoint(u: number, v: number): [number, number] {
    const { back, right, left } = ROOM;
    return [
        back[0] + u * (right[0] - back[0]) + v * (left[0] - back[0]),
        back[1] + u * (right[1] - back[1]) + v * (left[1] - back[1]),
    ];
}

/** The inverse of `floorPoint`: the (u, v) of a point. */
export function floorCoords(x: number, y: number): [number, number] {
    const { back, right, left } = ROOM;
    const ax = right[0] - back[0];
    const ay = right[1] - back[1];
    const bx = left[0] - back[0];
    const by = left[1] - back[1];
    const det = ax * by - ay * bx;
    const dx = x - back[0];
    const dy = y - back[1];
    return [(dx * by - dy * bx) / det, (ax * dy - ay * dx) / det];
}

/** True when the point is on the floor diamond (edges count). */
export function insideFloor(x: number, y: number, eps = 1e-6): boolean {
    const [u, v] = floorCoords(x, y);
    return u >= -eps && v >= -eps && u <= 1 + eps && v <= 1 + eps;
}

export interface Placement {
    /** Index in the list of employees that was given to `layoutOffice`. */
    index: number;
    col: number;
    row: number;
    /** Centre of the desk on the floor. */
    cx: number;
    cy: number;
    /** Top-left corner and size of the desk image. */
    x0: number;
    y0: number;
    size: number;
    /** Feet of the agent: the anchor of the character. */
    fx: number;
    fy: number;
    /** Height of the agent. */
    ah: number;
}

export interface OfficeLayout extends Grid {
    /** In draw order: back to front. */
    placements: Placement[];
}

/** The layout of `count` desks. The result is in draw order (the farthest desk first). */
export function layoutOffice(count: number): OfficeLayout {
    const grid = gridFor(count);
    const { cols, rows, k } = grid;
    const size = DESK.size * k;
    const cells = Array.from({ length: Math.max(0, count) }, (_, index) => {
        const col = index % cols;
        const row = Math.floor(index / cols);
        const u = 0.05 + ((col + 0.5) / cols) * 0.9;
        const v = 0.04 + ((row + 0.6) / rows) * 0.94;
        return { index, col, row, u, v };
    }).sort((p, q) => p.u + p.v - (q.u + q.v));
    const placements = cells.map(({ index, col, row, u, v }): Placement => {
        const [cx, cy] = floorPoint(u, v);
        const x0 = cx - DESK.ax * k;
        const y0 = cy - DESK.ay * k;
        return {
            index,
            col,
            row,
            cx,
            cy,
            x0,
            y0,
            size,
            fx: x0 + DESK.fx * k,
            fy: y0 + DESK.fy * k,
            ah: DESK.agent * size,
        };
    });
    return { ...grid, placements };
}

// ---- name tags ----

const TAG_NAME_MAX = 12;

/** The name in the tag. A long name is cut, so that the tag stays small. */
export function tagLabel(name: string): string {
    const clean = name.trim();
    const chars = [...clean];
    return chars.length > TAG_NAME_MAX ? chars.slice(0, TAG_NAME_MAX - 1).join("") + "…" : clean;
}

/** Width of the tag in SVG units, for a label (see `tagLabel`). */
export function tagWidth(label: string): number {
    return 58 + [...label].length * 17;
}

const TAG_ROLE_MAX = 16;

/** Font size of the role line before scaling, in SVG units. */
export const TAG_ROLE_FONT = 20;

/**
 * The tags show the role line up to this number of employees. Above it, a taller tag covers the
 * characters behind it. The role then shows in a small box above the tag, only for the selected
 * employee or when the pointer is on the tag. The CEO and the Project Manager keep a chip.
 */
export const TAG_ROLE_MAX_EMPLOYEES = 5;

/** The job title in the tag, in capitals. A long title is cut. */
export function tagRole(role: string): string {
    const chars = [...role.trim().toUpperCase()];
    return chars.length > TAG_ROLE_MAX ? chars.slice(0, TAG_ROLE_MAX - 1).join("") + "…" : chars.join("");
}

/** A small chip on the tag of the CEO ("CEO", gold) and of the Project Manager ("PM", blue). */
export interface RoleChip {
    text: string;
    /** Fill of the chip. */
    color: string;
}

export function roleChip(role: string): RoleChip | null {
    const r = role.trim().toLowerCase();
    if (r === "ceo") return { text: "CEO", color: "#ffd166" };
    if (r === "project manager") return { text: "PM", color: "#7cc4ff" };
    return null;
}

/** Width of a chip in SVG units, with the gap before it. */
export function chipWidth(chip: RoleChip | null): number {
    return chip ? 16 + [...chip.text].length * 14 : 0;
}

/** Width of the tag for the name label, the role line and the chip: the wider of the lines. */
export function tagBoxWidth(label: string, role: string, chip: RoleChip | null = null): number {
    return Math.max(tagWidth(label) + chipWidth(chip), 28 + [...role].length * 14.5);
}

/** Width of the small box that shows the role above a tag that has no role line. */
export function rolePopWidth(role: string): number {
    return 28 + [...role].length * 14.5;
}

/** Colour of the role line. The CEO and the Project Manager have their own colour. */
export function roleColor(role: string): string {
    const r = role.trim().toLowerCase();
    if (r === "ceo") return "#ffd166";
    if (r === "project manager") return "#7cc4ff";
    return "#9fb0cc";
}

/** Scale of the tag from the scale of the desk (the prototype rule). */
export function tagScale(k: number): number {
    return Math.max(0.72, Math.min(1, k / 0.3));
}

/** Font size of the tag text before scaling, in SVG units. */
export const TAG_FONT = 26;

/**
 * Scale of the tag when the SVG is drawn at `viewScale` (screen pixels for one SVG unit). The text
 * never gets smaller than `minFontPx` on the screen, and the tag never grows past 1.6.
 */
export function readableTagScale(k: number, viewScale: number, minFontPx = 10): number {
    const base = tagScale(k);
    if (!(viewScale > 0)) return base;
    return Math.min(1.6, Math.max(base, minFontPx / (TAG_FONT * viewScale)));
}

/** Y of the tag centre above the feet of the agent. */
export function tagY(p: Pick<Placement, "fy" | "ah">, scale: number): number {
    return p.fy - p.ah - 36 * scale;
}

// ---- characters ----

/** Number of sprites: `agent-0` to `agent-5`. */
export const SPRITE_COUNT = 6;

function hash(text: string): number {
    // FNV-1a, 32 bits. Stable in every JS engine.
    let h = 0x811c9dc5;
    for (let i = 0; i < text.length; i++) {
        h ^= text.charCodeAt(i);
        h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h >>> 0;
}

/** The sprite of an employee: CEO 0, Project Manager 1, any other role a stable sprite from 2 to 5. */
export function spriteFor(role: string, slug: string): number {
    const r = role.trim().toLowerCase();
    if (r === "ceo") return 0;
    if (r === "project manager") return 1;
    return 2 + (hash(slug) % (SPRITE_COUNT - 2));
}
