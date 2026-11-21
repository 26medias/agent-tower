import { describe, expect, it } from "vitest";
import { layout } from "./layout.svelte";
import { ART, T, buildingHeight } from "./office-layout";
import {
    BUILDING_MIN_W,
    INFO_DEFAULT_MIN,
    INFO_MIN,
    buildingScale,
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
} from "./sizes";

describe("columnRange and defaultColumn", () => {
    it("is 160 px to 50% of the window", () => {
        expect(columnRange(1280)).toEqual({ min: 160, max: 640 });
        expect(columnRange(1001)).toEqual({ min: 160, max: 500 });
    });

    it("keeps the minimum in a very narrow window", () => {
        expect(columnRange(200)).toEqual({ min: 160, max: 160 });
    });

    it("keeps the old default: a quarter of the window, from 260 to 360", () => {
        expect(defaultColumn(1280)).toBe(320);
        expect(defaultColumn(800)).toBe(260);
        expect(defaultColumn(2400)).toBe(360);
    });
});

describe("officeRange and defaultOffice", () => {
    it("leaves 160 px to the terminal area", () => {
        expect(officeRange(700)).toEqual({ min: 120, max: 540 });
    });

    it("keeps the office minimum when the space is small", () => {
        expect(officeRange(250)).toEqual({ min: 120, max: 120 });
    });

    it("keeps the old default: 42% of the window, from 220 to 640", () => {
        expect(defaultOffice(800)).toBe(336);
        expect(defaultOffice(400)).toBe(220);
        expect(defaultOffice(2000)).toBe(640);
    });

    it("gives the terminal at least half of a 1280 x 800 window at the default size", () => {
        const space = 800 - 30 - 22; // tabs and the closed debug bar
        const h = resolveSize(null, defaultOffice(800), officeRange(space));
        expect(space - h).toBeGreaterThanOrEqual(400 - 20);
        expect(h).toBe(336);
    });
});

describe("infoRange and defaultInfo", () => {
    it("keeps 200 px for the info panel and 120 px for the room", () => {
        expect(infoRange(960)).toEqual({ min: 200, max: 840 });
        expect(infoRange(300)).toEqual({ min: 200, max: 200 });
    });

    it("gives the room a 3:2 box at the default height", () => {
        expect(defaultInfo(960, 336)).toBe(456);
        expect(defaultInfo(500, 336)).toBe(300);
    });

    it("gives the room a 3:2 box at the real height of the office, so a taller office gives a wider room", () => {
        // The room box is 1.5 x the height, the info panel takes the rest.
        expect(defaultInfo(960, 400)).toBe(360);
        expect(defaultInfo(960, 420)).toBe(330);
        const room = (h: number) => 960 - defaultInfo(960, h);
        expect(room(336)).toBe(504);
        expect(room(400)).toBe(600);
        expect(room(420)).toBe(630);
        expect(room(420)).toBeGreaterThan(room(336));
    });

    it("stops the growth of the room when the info panel is at its minimum", () => {
        expect(INFO_DEFAULT_MIN).toBeGreaterThanOrEqual(INFO_MIN);
        expect(defaultInfo(960, 440)).toBe(INFO_DEFAULT_MIN);
        expect(defaultInfo(960, 585)).toBe(INFO_DEFAULT_MIN);
        expect(defaultInfo(960, 2000)).toBe(INFO_DEFAULT_MIN);
        // The room never grows more than this, and the range still allows the operator to go lower.
        expect(infoRange(960).min).toBeLessThan(INFO_DEFAULT_MIN);
    });
});

describe("layout: the default info width follows the real office height", () => {
    it("is the old default at the default height, and the room grows with the office", () => {
        layout.resetOffice();
        layout.resetInfo();
        layout.measure(1280, 800, 960, 740);
        expect(layout.officeH).toBe(336);
        expect(layout.infoW).toBe(456);
        const roomAt = (h: number) => {
            layout.setOffice(h);
            return 960 - layout.infoW;
        };
        // Measured before the repair at 1280 x 800: the office at 585 px gave a 503 px room.
        expect(roomAt(336)).toBe(504);
        expect(roomAt(400)).toBe(600);
        expect(roomAt(500)).toBe(960 - INFO_DEFAULT_MIN);
        expect(layout.infoW).toBe(INFO_DEFAULT_MIN);
    });

    it("keeps a width that the operator set by hand when the office height changes", () => {
        layout.resetOffice();
        layout.resetInfo();
        layout.measure(1280, 800, 960, 740);
        layout.setInfo(380);
        layout.setOffice(500);
        expect(layout.infoW).toBe(380);
        layout.setOffice(250);
        expect(layout.infoW).toBe(380);
        // A reset gives the default back, and it follows the office again.
        layout.resetInfo();
        expect(layout.infoW).toBe(960 - Math.round(250 * 1.5));
        layout.resetOffice();
        layout.resetInfo();
    });
});

describe("resolveSize", () => {
    const range = { min: 160, max: 640 };

    it("uses the default when there is no custom value", () => {
        expect(resolveSize(null, 320, range)).toBe(320);
    });

    it("clamps a custom value to the range", () => {
        expect(resolveSize(50, 320, range)).toBe(160);
        expect(resolveSize(9000, 320, range)).toBe(640);
        expect(resolveSize(400.4, 320, range)).toBe(400);
    });

    it("clamps again for a smaller window (the restored value follows the window)", () => {
        expect(resolveSize(600, 320, columnRange(1280))).toBe(600);
        expect(resolveSize(600, 320, columnRange(800))).toBe(400);
    });

    it("ignores a value that is not a number", () => {
        expect(resolveSize(Number.NaN, 320, range)).toBe(320);
    });

    it("clamp: a range with max below min gives min", () => {
        expect(clamp(5, 10, 3)).toBe(10);
    });
});

describe("parseStored", () => {
    it("reads a good object", () => {
        expect(parseStored(serializeStored({ col: 200, office: 300, info: null }))).toEqual({
            col: 200,
            office: 300,
            info: null,
        });
    });

    it("gives defaults for bad text", () => {
        const none = { col: null, office: null, info: null };
        expect(parseStored(null)).toEqual(none);
        expect(parseStored("")).toEqual(none);
        expect(parseStored("{not json")).toEqual(none);
        expect(parseStored("[1,2]")).toEqual({ col: null, office: null, info: null });
        expect(parseStored("5")).toEqual(none);
    });

    it("drops a field that is not a positive number", () => {
        expect(parseStored('{"col":"x","office":-4,"info":0}')).toEqual({ col: null, office: null, info: null });
    });
});

describe("buildingScale", () => {
    it("is 0 while the area has no width", () => {
        expect(buildingScale(0, 500, 4)).toBe(0);
    });

    it("fits the width when the height is large", () => {
        expect(buildingScale(300, 5000, 4)).toBeCloseTo(300 / ART.w, 6);
    });

    it("fits the height when the height is the limit", () => {
        const s = buildingScale(900, 400, 4);
        expect(s).toBeCloseTo(400 / buildingHeight(4), 6);
        expect(ART.w * s).toBeLessThanOrEqual(900);
    });

    it("never draws the building wider than the area", () => {
        for (const w of [100, 144, 300, 700]) {
            for (const h of [100, 400, 900]) {
                expect(ART.w * buildingScale(w, h, 6)).toBeLessThanOrEqual(w + 1e-9);
            }
        }
    });

    it("keeps a readable width with many floors, so the column scrolls", () => {
        const s = buildingScale(300, 400, 13);
        expect(ART.w * s).toBeCloseTo(BUILDING_MIN_W, 6);
        expect(buildingHeight(13) * s).toBeGreaterThan(400);
        expect(buildingHeight(13)).toBe(ART.h + 12 * T.h);
    });

    it("lets the width win when the column is narrower than the readable width", () => {
        const s = buildingScale(144, 400, 13);
        expect(ART.w * s).toBeCloseTo(144, 6);
    });
});
