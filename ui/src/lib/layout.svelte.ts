// The sizes that the operator can change by dragging: the width of the building column, the
// height of the office, and the width of the info panel in the office.
// A size that the operator never changed is `null`: it follows the window (the old fixed rules).
// A custom size is kept in localStorage. Each read of a size clamps it to the limits of the
// current window, so a window that shrinks never leaves a handle out of reach.
import {
    clamp,
    columnRange,
    defaultColumn,
    defaultInfo,
    defaultOffice,
    infoRange,
    officeRange,
    parseStored,
    resolveSize,
    serializeStored,
    type StoredSizes,
} from "./sizes";

const KEY = "agent-tower:layout";

function read(): StoredSizes {
    try {
        return parseStored(localStorage.getItem(KEY));
    } catch {
        return parseStored(null);
    }
}

function write(sizes: StoredSizes): void {
    try {
        localStorage.setItem(KEY, serializeStored(sizes));
    } catch {
        // Storage can be blocked. The sizes work for this run only.
    }
}

class Layout {
    /** Window size. The page keeps it up to date. */
    winW = $state(typeof window !== "undefined" ? window.innerWidth : 1280);
    winH = $state(typeof window !== "undefined" ? window.innerHeight : 800);
    /** Width of the work area (the office is as wide). */
    workW = $state(960);
    /** Height that the office and the terminal area share. */
    space = $state(740);

    /** Custom sizes. `null`: the default. */
    private custom = $state.raw<StoredSizes>({ col: null, office: null, info: null });

    colRange = $derived(columnRange(this.winW));
    colW = $derived(resolveSize(this.custom.col, defaultColumn(this.winW), this.colRange));
    officeRangeNow = $derived(officeRange(this.space));
    officeH = $derived(resolveSize(this.custom.office, defaultOffice(this.winH), this.officeRangeNow));
    infoRangeNow = $derived(infoRange(this.workW));
    // The default width follows the REAL height of the office (`officeH`), so a taller office gives
    // the room a wider box. A width that the operator set by hand (`custom.info`) is kept as it is.
    infoW = $derived(resolveSize(this.custom.info, defaultInfo(this.workW, this.officeH), this.infoRangeNow));

    constructor() {
        this.custom = read();
    }

    /** Called by the page when the window or the work area changes size. */
    measure(winW: number, winH: number, workW: number, space: number): void {
        if (winW > 0) this.winW = winW;
        if (winH > 0) this.winH = winH;
        if (workW > 0) this.workW = workW;
        if (space > 0) this.space = space;
    }

    private store(next: StoredSizes): void {
        this.custom = next;
        write(next);
    }

    setColumn(px: number): void {
        this.store({ ...this.custom, col: Math.round(clamp(px, this.colRange.min, this.colRange.max)) });
    }

    setOffice(px: number): void {
        this.store({ ...this.custom, office: Math.round(clamp(px, this.officeRangeNow.min, this.officeRangeNow.max)) });
    }

    setInfo(px: number): void {
        this.store({ ...this.custom, info: Math.round(clamp(px, this.infoRangeNow.min, this.infoRangeNow.max)) });
    }

    resetColumn(): void {
        this.store({ ...this.custom, col: null });
    }

    resetOffice(): void {
        this.store({ ...this.custom, office: null });
    }

    resetInfo(): void {
        this.store({ ...this.custom, info: null });
    }
}

export const layout = new Layout();
