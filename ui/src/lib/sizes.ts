// Pure logic for the sizes that the operator can change: the width of the building column, the
// height of the office, and the width of the info panel in the office. No Svelte, no DOM.
import { ART, buildingHeight } from "./office-layout";

export const COL_MIN = 160;
/** The column is at most this part of the window width. */
export const COL_MAX_FRACTION = 0.5;
export const OFFICE_MIN = 120;
/** The terminal area keeps at least this height. */
export const TERMINAL_MIN = 160;
export const INFO_MIN = 200;
/** The task board column is never narrower than this while it is open. */
export const BOARD_MIN = 200;
/** The task board column width that the operator never changed. */
export const BOARD_DEFAULT = 260;
/** The terminal (the work area) keeps at least this width. The board gives way. */
export const WORK_MIN = 480;
/** The width of the strip that is left of a collapsed board. */
export const BOARD_STRIP = 36;
/** The room keeps at least this width. */
export const ROOM_MIN = 120;
/** One arrow key press moves a handle by this many pixels. */
export const KEY_STEP = 16;
/** The building is never drawn narrower than this, unless the column itself is narrower. */
export const BUILDING_MIN_W = 200;

export interface Range {
    min: number;
    max: number;
}

export interface StoredSizes {
    col: number | null;
    office: number | null;
    info: number | null;
    /** Width of the task board column. */
    board: number | null;
    /** `null`: follow the window (collapsed when the terminal would be narrower than `WORK_MIN`). */
    boardCollapsed: boolean | null;
}

export function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), Math.max(min, max));
}

export function columnRange(winW: number): Range {
    return { min: COL_MIN, max: Math.max(COL_MIN, Math.floor(winW * COL_MAX_FRACTION)) };
}

/** The old fixed width: a quarter of the window, from 260 to 360 px. */
export function defaultColumn(winW: number): number {
    return Math.round(clamp(winW * 0.25, 260, 360));
}

/**
 * `space` is the height that the office and the terminal area share (the height of the work area
 * minus the rows around them). The terminal area keeps `TERMINAL_MIN`.
 */
export function officeRange(space: number): Range {
    return { min: OFFICE_MIN, max: Math.max(OFFICE_MIN, Math.floor(space - TERMINAL_MIN)) };
}

/**
 * The old fixed height: 42% of the window, from 220 to 640 px. A wide window (1800 px or more)
 * gets 50% and up to 760 px: the room picture is wide there and a low office would leave it small.
 * `winW` and `employees` are optional. With none of them the old rule applies.
 */
export function defaultOffice(winH: number, winW = 0, employees = 0): number {
    const wide = winW >= WIDE_WINDOW;
    // A crowded floor gets 10 points more: the characters would be too small otherwise.
    const share = (wide ? 0.5 : 0.42) + (employees > CROWDED ? 0.1 : 0);
    return Math.round(clamp(winH * share, 220, wide ? 760 : 640));
}

/** A floor with more employees than this gets a taller office by default. */
export const CROWDED = 12;

/** From this window width on, the defaults give the room more space. */
export const WIDE_WINDOW = 1800;

/** The info panel keeps `INFO_MIN`. The room keeps `ROOM_MIN`. */
export function infoRange(officeW: number): Range {
    return { min: INFO_MIN, max: Math.max(INFO_MIN, Math.floor(officeW - ROOM_MIN)) };
}

/** The info panel that the operator never resized is not narrower than this (a drag can go down to `INFO_MIN`). */
export const INFO_DEFAULT_MIN = 300;

/**
 * The width of the info panel while the operator has not set it by hand. The room gets a 3:2 box
 * at the REAL height of the office (`officeH`), so the room picture grows when the operator makes
 * the office taller. The info panel takes the rest, but never less than `INFO_DEFAULT_MIN`.
 */
export function defaultInfo(officeW: number, officeH: number): number {
    const rest = Math.round(officeW - officeH * 1.5);
    // A wide office: the info panel is at most 38% of it. The room gets the rest.
    const capped = officeW >= WIDE_OFFICE ? Math.min(rest, Math.round(officeW * 0.38)) : rest;
    return Math.max(INFO_DEFAULT_MIN, capped);
}

/** From this office width on, the info panel stops at 38% of the width. */
export const WIDE_OFFICE = 1500;

/**
 * The width range of the open task board. The terminal keeps `WORK_MIN`: the board is at most
 * what is left of the window after the building column and `WORK_MIN`.
 */
export function boardRange(winW: number, colW: number): Range {
    return { min: BOARD_MIN, max: Math.max(BOARD_MIN, Math.floor(winW - colW - WORK_MIN)) };
}

/** True when an open board of the default width would leave the terminal narrower than `WORK_MIN`. */
export function boardCollapsedByDefault(winW: number, colW: number): boolean {
    return winW - colW - BOARD_DEFAULT < WORK_MIN;
}

/** The size to use: the custom value when there is one, else the default. Always inside the range. */
export function resolveSize(custom: number | null, fallback: number, range: Range): number {
    const raw = custom !== null && Number.isFinite(custom) ? custom : fallback;
    return Math.round(clamp(raw, range.min, range.max));
}

function numberOrNull(value: unknown): number | null {
    return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.round(value) : null;
}

/** Read the stored text. Anything that is not a clean object gives only nulls (all defaults). */
export function parseStored(text: string | null): StoredSizes {
    const none: StoredSizes = { col: null, office: null, info: null, board: null, boardCollapsed: null };
    if (!text) return none;
    try {
        const data: unknown = JSON.parse(text);
        if (typeof data !== "object" || data === null) return none;
        const o = data as Record<string, unknown>;
        return {
            col: numberOrNull(o.col),
            office: numberOrNull(o.office),
            info: numberOrNull(o.info),
            board: numberOrNull(o.board),
            boardCollapsed: typeof o.boardCollapsed === "boolean" ? o.boardCollapsed : null,
        };
    } catch {
        return none;
    }
}

export function serializeStored(sizes: StoredSizes): string {
    return JSON.stringify(sizes);
}

/**
 * Screen pixels for one SVG unit of the building. The building fits the width and the height
 * of its area. When the height would make it narrower than `BUILDING_MIN_W`, the height does not
 * count, and the area scrolls. The width always counts: the building never overflows sideways.
 * Returns 0 while the area has no size yet.
 */
export function buildingScale(availW: number, availH: number, count: number): number {
    if (!(availW > 0)) return 0;
    const byWidth = availW / ART.w;
    if (!(availH > 0)) return byWidth;
    const byHeight = availH / buildingHeight(count);
    return Math.min(byWidth, Math.max(byHeight, BUILDING_MIN_W / ART.w));
}
