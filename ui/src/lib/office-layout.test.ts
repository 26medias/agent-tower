import { describe, expect, it } from "vitest";
import {
    ROOM,
    SPRITE_COUNT,
    T,
    buildingHeight,
    floorCoords,
    floorPoint,
    gridFor,
    insideFloor,
    layoutOffice,
    readableTagScale,
    spriteFor,
    stickerTransform,
    storeyY,
    roleColor,
    TAG_ROLE_MAX_EMPLOYEES,
    CHIP_GAP,
    TAG_NATURAL_PX,
    chipBoxWidth,
    chipWidth,
    nameWidth,
    rolePopWidth,
    roleChip,
    tagBoxWidth,
    tagLabel,
    tagRole,
    tagScale,
    tagWidth,
    tagY,
} from "./office-layout";

describe("gridFor", () => {
    it("gives one column for one desk, with the largest desk scale", () => {
        expect(gridFor(1)).toEqual({ cols: 1, rows: 1, k: 0.36 });
        expect(gridFor(0)).toEqual({ cols: 1, rows: 1, k: 0.36 });
    });

    it("has room for every desk and gets smaller desks with more employees", () => {
        let last = Infinity;
        for (let n = 1; n <= 20; n++) {
            const g = gridFor(n);
            expect(g.cols * g.rows).toBeGreaterThanOrEqual(n);
            expect(g.k).toBeLessThanOrEqual(last + 1e-9);
            last = g.k;
        }
    });

    it("matches the prototype for 4, 5 and 13 desks", () => {
        expect(gridFor(4)).toMatchObject({ cols: 2, rows: 2 });
        expect(gridFor(5)).toMatchObject({ cols: 2, rows: 3 });
        const g13 = gridFor(13);
        expect(g13).toMatchObject({ cols: 3, rows: 5 });
        expect(g13.k).toBeCloseTo(0.92 / (0.85 * 5), 6);
    });
});

describe("floorPoint and floorCoords", () => {
    it("map the corners of the unit square to the corners of the diamond", () => {
        expect(floorPoint(0, 0)).toEqual([...ROOM.back]);
        expect(floorPoint(1, 0)).toEqual([...ROOM.right]);
        expect(floorPoint(0, 1)).toEqual([...ROOM.left]);
    });

    it("are inverse to each other", () => {
        for (const [u, v] of [
            [0.2, 0.7],
            [0.9, 0.1],
            [0.5, 0.5],
        ]) {
            const [x, y] = floorPoint(u, v);
            const [u2, v2] = floorCoords(x, y);
            expect(u2).toBeCloseTo(u, 9);
            expect(v2).toBeCloseTo(v, 9);
        }
    });

    it("insideFloor tells inside from outside", () => {
        expect(insideFloor(768, 600)).toBe(true);
        expect(insideFloor(...floorPoint(1, 1))).toBe(true); // the front corner
        expect(insideFloor(768, 300)).toBe(false); // above the back corner
        expect(insideFloor(60, 645)).toBe(false); // left of the left corner
        expect(insideFloor(768, 990)).toBe(false); // below the front corner
        expect(insideFloor(1500, 645)).toBe(false);
    });
});

describe("layoutOffice", () => {
    it("returns one placement for each desk, each index once", () => {
        for (const n of [0, 1, 2, 5, 13, 20]) {
            const l = layoutOffice(n);
            expect(l.placements).toHaveLength(n);
            expect(l.placements.map((p) => p.index).sort((a, b) => a - b)).toEqual(Array.from({ length: n }, (_, i) => i));
        }
    });

    it("puts every character anchor inside the floor diamond, for 1 to 20 employees", () => {
        for (let n = 1; n <= 20; n++) {
            for (const p of layoutOffice(n).placements) {
                expect(insideFloor(p.fx, p.fy), `n=${n} index=${p.index} at ${p.fx},${p.fy}`).toBe(true);
            }
        }
    });

    it("gives each character its own anchor", () => {
        const seen = new Set<string>();
        for (const p of layoutOffice(13).placements) seen.add(`${Math.round(p.fx)},${Math.round(p.fy)}`);
        expect(seen.size).toBe(13);
    });

    it("lists the placements back to front, so that a near desk covers a far one", () => {
        const l = layoutOffice(13);
        const depth = l.placements.map((p) => {
            const [u, v] = floorCoords(p.cx, p.cy);
            return u + v;
        });
        for (let i = 1; i < depth.length; i++) expect(depth[i]).toBeGreaterThanOrEqual(depth[i - 1] - 1e-9);
    });

    it("keeps the prototype numbers for the feet of the agent", () => {
        const [p] = layoutOffice(1).placements;
        const k = 0.36;
        expect(p.x0).toBeCloseTo(p.cx - 510 * k, 9);
        expect(p.fx).toBeCloseTo(p.x0 + 610 * k, 9);
        expect(p.fy).toBeCloseTo(p.y0 + 600 * k, 9);
        expect(p.ah).toBeCloseTo(0.47 * 1024 * k, 9);
    });
});

describe("spriteFor", () => {
    it("gives the CEO sprite 0 and the Project Manager sprite 1", () => {
        expect(spriteFor("CEO", "ceo")).toBe(0);
        expect(spriteFor(" ceo ", "x")).toBe(0);
        expect(spriteFor("Project Manager", "pm")).toBe(1);
    });

    it("gives any other role a stable sprite from 2 to 5", () => {
        for (const slug of ["eng-sam", "eng-lin", "a", "zed-zebra", "qa", "x1", "x2", "x3", "x4"]) {
            const s = spriteFor("Engineer", slug);
            expect(s).toBeGreaterThanOrEqual(2);
            expect(s).toBeLessThan(SPRITE_COUNT);
            expect(spriteFor("Engineer", slug)).toBe(s);
        }
    });

    it("uses all four sprites over many slugs", () => {
        const used = new Set<number>();
        for (let i = 0; i < 60; i++) used.add(spriteFor("Tester", `emp-${i}`));
        expect([...used].sort()).toEqual([2, 3, 4, 5]);
    });
});

describe("building", () => {
    it("puts the first floor at the bottom", () => {
        expect(storeyY(0, 3)).toBe(2 * T.h);
        expect(storeyY(2, 3)).toBe(0);
        expect(storeyY(0, 1)).toBe(0);
    });

    it("grows by one storey step for each floor", () => {
        expect(buildingHeight(1)).toBe(1024);
        expect(buildingHeight(4)).toBe(1024 + 3 * T.h);
    });

    it("moves the sticker with the storey", () => {
        expect(stickerTransform(0)).toBe(`matrix(1 ${-T.slope} 0 1 ${T.cx} ${T.cy})`);
        expect(stickerTransform(300)).toBe(`matrix(1 ${-T.slope} 0 1 ${T.cx} ${T.cy + 300})`);
    });
});

describe("name tags", () => {
    it("sizes the name by the width of its letters, not by a flat width", () => {
        // The tag is the glyph area (58) plus the width of the name.
        expect(tagWidth("Ada")).toBeCloseTo(58 + nameWidth("Ada"), 9);
        // "Maya" has wide letters: it needs more than 4 flat units of 17.
        expect(nameWidth("Maya")).toBeGreaterThan(4 * 17);
        // Narrow letters take less than wide ones.
        expect(nameWidth("Ili")).toBeLessThan(nameWidth("Moo"));
        expect(nameWidth("")).toBe(0);
    });

    it("cuts a long name", () => {
        expect(tagLabel("Ada")).toBe("Ada");
        expect(tagLabel("  Ada  ")).toBe("Ada");
        const cut = tagLabel("Bartholomew the Fourth");
        expect([...cut]).toHaveLength(12);
        expect(cut.endsWith("…")).toBe(true);
    });

    it("scales the tag with the desk, between 0.72 and 1", () => {
        expect(tagScale(0.36)).toBe(1);
        expect(tagScale(0.1)).toBe(0.72);
        expect(tagScale(0.24)).toBeCloseTo(0.8, 9);
    });

    it("keeps the text readable when the SVG is drawn small", () => {
        // At 0.33 screen pixels for each unit, 26 units are 8.6 px: the tag must grow.
        const s = readableTagScale(0.216, 0.33, 10);
        expect(s).toBeGreaterThan(tagScale(0.216));
        expect(26 * s * 0.33).toBeGreaterThanOrEqual(10 - 1e-9);
        // A large drawing keeps the prototype scale.
        expect(readableTagScale(0.36, 1, 10)).toBe(1);
        // A bad measure (0) does not break the scale.
        expect(readableTagScale(0.36, 0, 10)).toBe(1);
        // The tag never grows without a limit.
        expect(readableTagScale(0.2, 0.01, 10)).toBe(1.6);
    });

    it("makes the tag grow with the zoom up to its natural size", () => {
        // 25 employees: small desks. The room is drawn at 0.26 px for one unit when the whole room shows.
        const k = 0.15;
        const px = (zoom: number): number => 26 * readableTagScale(k, 0.26 * zoom, 10, zoom) * 0.26 * zoom;
        expect(px(1)).toBeCloseTo(10, 5);
        // A zoom of 1.2 makes the text bigger than at zoom 1 (the old rule kept it at 10 px up to a zoom of 2).
        expect(px(1.2)).toBeGreaterThan(px(1) + 1);
        expect(px(1.2)).toBeLessThanOrEqual(TAG_NATURAL_PX + 1e-9);
        // It reaches the natural size and does not pass it while the desks are still small.
        expect(px(1.4)).toBeCloseTo(TAG_NATURAL_PX, 5);
        expect(px(2)).toBeCloseTo(TAG_NATURAL_PX, 5);
        // At a high zoom the tag follows the desks (its scale in SVG units does not shrink below the base).
        expect(readableTagScale(k, 0.26 * 4, 10, 4)).toBe(tagScale(k));
        // The old call (no zoom argument) still means zoom 1.
        expect(readableTagScale(k, 0.26, 10)).toBe(readableTagScale(k, 0.26, 10, 1));
    });

    it("puts the tag above the head", () => {
        const [p] = layoutOffice(1).placements;
        expect(tagY(p, 1)).toBeLessThan(p.fy - p.ah);
    });
});

describe("role line of the tag", () => {
    it("shows the job title in capitals and cuts a long title", () => {
        expect(tagRole(" Project Manager ")).toBe("PROJECT MANAGER");
        const cut = tagRole("Senior Database Reliability Engineer");
        expect([...cut].length).toBe(16);
        expect(cut.endsWith("…")).toBe(true);
    });

    it("makes the tag as wide as the wider line", () => {
        expect(tagBoxWidth("Ada", "CEO")).toBe(tagWidth("Ada"));
        expect(tagBoxWidth("Ada", "PROJECT MANAGER")).toBeGreaterThan(tagWidth("Ada"));
    });

    it("gives a chip to the CEO and the Project Manager only, and widens the tag for it", () => {
        expect(roleChip("CEO")).toEqual({ text: "CEO", color: "#ffd166" });
        expect(roleChip(" Project Manager ")).toEqual({ text: "PM", color: "#7cc4ff" });
        expect(roleChip("Engineer")).toBeNull();
        expect(chipWidth(null)).toBe(0);
        const chip = roleChip("CEO");
        expect(tagBoxWidth("Ada", "", chip)).toBe(tagWidth("Ada") + chipWidth(chip));
        expect(tagBoxWidth("Ada", "", null)).toBe(tagWidth("Ada"));
        expect(rolePopWidth("ENGINEER")).toBeGreaterThan(rolePopWidth("QA"));
    });

    it("gives the chip its own space: the tag is wider by the gap and the chip", () => {
        const chip = roleChip("Project Manager");
        expect(chipWidth(chip)).toBe(CHIP_GAP + chipBoxWidth(chip));
        expect(CHIP_GAP).toBeGreaterThan(0);
        // The chip starts after the end of the name and ends inside the tag.
        const name = "Maya";
        const content = tagWidth(name) + chipWidth(chip);
        const nameStart = -content / 2 + 24 + 20;
        const chipStart = nameStart + nameWidth(name) + CHIP_GAP;
        expect(chipStart + chipBoxWidth(chip)).toBeLessThanOrEqual(content / 2);
    });

    it("shows the role line only for a small team", () => {
        expect(TAG_ROLE_MAX_EMPLOYEES).toBe(5);
    });

    it("gives the CEO and the Project Manager their own colour", () => {
        const colors = new Set([roleColor("CEO"), roleColor("project manager"), roleColor("Engineer")]);
        expect(colors.size).toBe(3);
        expect(roleColor("QA")).toBe(roleColor("Engineer"));
    });
});
