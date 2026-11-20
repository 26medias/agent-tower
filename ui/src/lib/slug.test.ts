import { describe, expect, it } from "vitest";
import { isValidSlug, slugify, uniqueSlug } from "./slug";

describe("slugify", () => {
    it("lowercases and joins words with a dash", () => {
        expect(slugify("Maya Chen")).toBe("maya-chen");
    });

    it("removes accents", () => {
        expect(slugify("Zoë Élan")).toBe("zoe-elan");
    });

    it("collapses symbols and trims dashes", () => {
        expect(slugify("  --Eng #2 (backend)!  ")).toBe("eng-2-backend");
    });

    it("keeps digits and gives an empty slug for no usable character", () => {
        expect(slugify("3D artist")).toBe("3d-artist");
        expect(slugify("✓✓")).toBe("");
        expect(slugify("")).toBe("");
    });

    it("always makes a valid slug when it is not empty", () => {
        for (const name of ["Ada", "  x ", "A_B", "ßa", "Mr. O'Neil"]) {
            const slug = slugify(name);
            if (slug !== "") expect(isValidSlug(slug)).toBe(true);
        }
    });
});

describe("isValidSlug", () => {
    it("follows [a-z0-9][a-z0-9-]*", () => {
        expect(isValidSlug("ceo")).toBe(true);
        expect(isValidSlug("eng-2")).toBe(true);
        expect(isValidSlug("-x")).toBe(false);
        expect(isValidSlug("A")).toBe(false);
        expect(isValidSlug("a b")).toBe(false);
        expect(isValidSlug("")).toBe(false);
    });
});

describe("uniqueSlug", () => {
    it("keeps a free slug", () => {
        expect(uniqueSlug("sam", ["ceo", "pm"])).toBe("sam");
    });

    it("adds a number when the slug is taken", () => {
        expect(uniqueSlug("sam", ["sam"])).toBe("sam-2");
        expect(uniqueSlug("sam", ["sam", "sam-2"])).toBe("sam-3");
    });

    it("gives an empty slug for an empty base", () => {
        expect(uniqueSlug("", ["sam"])).toBe("");
    });
});
