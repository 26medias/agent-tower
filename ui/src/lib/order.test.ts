import { describe, expect, it } from "vitest";
import { orderEmployees, roleRank } from "./order";
import type { Employee } from "./types";

function emp(slug: string, name: string, manager: string | null, role = "R"): Employee {
    return { slug, name, role, manager, cli: "claude", model: null, effort: null, policy: "read_only", roleText: "" };
}

const slugs = (list: Employee[]) => list.map((e) => e.slug);

const team = [
    emp("eng-sam", "Sam", "pm", "Engineer"),
    emp("pm", "Maya", "ceo", "Project Manager"),
    emp("eng-lin", "Lin", "pm", "Engineer"),
    emp("ceo", "Ada", null, "CEO"),
];

describe("roleRank", () => {
    it("ranks the CEO first, the Project Manager second, and ignores case and spaces", () => {
        expect(roleRank("CEO")).toBe(0);
        expect(roleRank("  ceo ")).toBe(0);
        expect(roleRank("project manager")).toBe(1);
        expect(roleRank("Engineer")).toBe(2);
    });
});

describe("orderEmployees", () => {
    it("puts the CEO first, then the Project Manager, then the others by name", () => {
        expect(slugs(orderEmployees(team))).toEqual(["ceo", "pm", "eng-lin", "eng-sam"]);
    });

    it("puts the CEO and the PM first even when their names sort last", () => {
        const list = [emp("a", "Ann", "pm", "Engineer"), emp("pm", "Zed", "ceo", "Project Manager"), emp("ceo", "Zoe", null, "CEO")];
        expect(slugs(orderEmployees(list))).toEqual(["ceo", "pm", "a"]);
    });

    it("groups the others by the depth of their manager chain, then by name", () => {
        const list = [
            emp("ceo", "Ada", null, "CEO"),
            emp("pm", "Maya", "ceo", "Project Manager"),
            emp("deep", "Abe", "lead", "Engineer"), // depth 3
            emp("lead", "Zoe", "pm", "Tech lead"), // depth 2
            emp("eng-b", "Bob", "pm", "Engineer"), // depth 2
            emp("eng-a", "Al", "pm", "Engineer"), // depth 2
            emp("boss", "Yan", "ceo", "Advisor"), // depth 1
        ];
        expect(slugs(orderEmployees(list))).toEqual(["ceo", "pm", "boss", "eng-a", "eng-b", "lead", "deep"]);
    });

    it("does not depend on the input order", () => {
        const a = slugs(orderEmployees(team));
        expect(slugs(orderEmployees([...team].reverse()))).toEqual(a);
        expect(slugs(orderEmployees([team[2], team[0], team[3], team[1]]))).toEqual(a);
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
