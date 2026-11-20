import { describe, expect, it } from "vitest";
import { orderEmployees } from "./order";
import type { Employee } from "./types";

function emp(slug: string, name: string, manager: string | null): Employee {
    return { slug, name, role: "R", manager, cli: "claude", model: null, effort: null, policy: "read_only", roleText: "" };
}

const slugs = (list: Employee[]) => list.map((e) => e.slug);

const team = [
    emp("eng-sam", "Sam", "pm"),
    emp("pm", "Maya", "ceo"),
    emp("eng-lin", "Lin", "pm"),
    emp("ceo", "Ada", null),
];

describe("orderEmployees", () => {
    it("puts the root first, then the reports, depth first, siblings by name", () => {
        expect(slugs(orderEmployees(team))).toEqual(["ceo", "pm", "eng-lin", "eng-sam"]);
    });

    it("goes depth first: a whole branch comes before the next sibling", () => {
        const list = [
            emp("a", "Ann", null),
            emp("b", "Bob", "a"),
            emp("c", "Cy", "a"),
            emp("d", "Di", "b"),
        ];
        expect(slugs(orderEmployees(list))).toEqual(["a", "b", "d", "c"]);
    });

    it("does not depend on the input order", () => {
        const a = slugs(orderEmployees(team));
        expect(slugs(orderEmployees([...team].reverse()))).toEqual(a);
        expect(slugs(orderEmployees([team[2], team[0], team[3], team[1]]))).toEqual(a);
    });

    it("keeps the order of the others when an unrelated employee is added", () => {
        const before = slugs(orderEmployees(team));
        const added = slugs(orderEmployees([...team, emp("zed", "Zed", null)]));
        expect(added).toEqual([...before, "zed"]);
        // A new report under a leaf does not move the others either.
        const withReport = slugs(orderEmployees([...team, emp("intern", "Ivy", "eng-sam")]));
        expect(withReport.filter((s) => s !== "intern")).toEqual(before);
    });

    it("breaks name ties by slug, ignoring case", () => {
        const list = [emp("b2", "sam", null), emp("b1", "Sam", null), emp("a", "Zoe", null)];
        expect(slugs(orderEmployees(list))).toEqual(["b1", "b2", "a"]);
    });

    it("treats a manager that is not in the floor as no manager", () => {
        const list = [emp("x", "Xi", "ghost"), emp("a", "Ann", null)];
        expect(slugs(orderEmployees(list))).toEqual(["a", "x"]);
    });

    it("loses no employee in a manager cycle", () => {
        const list = [emp("a", "Ann", "b"), emp("b", "Bob", "a"), emp("c", "Cy", null)];
        const out = slugs(orderEmployees(list));
        expect(out[0]).toBe("c");
        expect([...out].sort()).toEqual(["a", "b", "c"]);
    });

    it("does not change its input", () => {
        const copy = slugs(team);
        orderEmployees(team);
        expect(slugs(team)).toEqual(copy);
    });
});
