import { describe, expect, it } from "vitest";
import { GROUP_MIN, buildHierarchy, groupColumns, hierarchyLevels, splitReports, type HierarchyNode } from "./hierarchy";
import { orderEmployees } from "./order";
import type { Employee } from "./types";

function emp(slug: string, name: string, manager: string | null, role = "Engineer"): Employee {
    return { slug, name, role, manager, cli: "claude", model: null, effort: null, policy: "read_only", roleText: "" };
}

const slugs = (nodes: readonly HierarchyNode[]) => nodes.map((n) => n.employee.slug);

/** Every node of the forest, depth first. */
function all(nodes: readonly HierarchyNode[]): HierarchyNode[] {
    return nodes.flatMap((n) => [n, ...all(n.children)]);
}

const team = [
    emp("eng-sam", "Sam", "pm"),
    emp("pm", "Maya", "ceo", "Project Manager"),
    emp("eng-lin", "Lin", "pm"),
    emp("ceo", "Ada", null, "CEO"),
];

describe("buildHierarchy", () => {
    it("makes one root for one employee", () => {
        const roots = buildHierarchy([emp("a", "Ann", null)]);
        expect(slugs(roots)).toEqual(["a"]);
        expect(roots[0].level).toBe(0);
        expect(roots[0].children).toEqual([]);
    });

    it("gives an empty forest for no employee", () => {
        expect(buildHierarchy([])).toEqual([]);
    });

    it("puts the reports under their manager, with the level of each node", () => {
        const roots = buildHierarchy(team);
        expect(slugs(roots)).toEqual(["ceo"]);
        const ceo = roots[0];
        expect(slugs(ceo.children)).toEqual(["pm"]);
        const pm = ceo.children[0];
        expect([ceo.level, pm.level]).toEqual([0, 1]);
        expect(slugs(pm.children)).toEqual(["eng-lin", "eng-sam"]);
        expect(pm.children.map((n) => n.level)).toEqual([2, 2]);
    });

    it("keeps the order of order.ts for the top employees and for the siblings", () => {
        const list = [
            emp("z", "Zoe", null, "CEO"),
            emp("b", "Bob", null, "Advisor"),
            emp("a", "Al", null, "Advisor"),
            emp("k3", "Kim", "z"),
            emp("k1", "Abe", "z"),
            emp("k2", "Cy", "z"),
        ];
        const roots = buildHierarchy(list);
        const order = orderEmployees(list).map((e) => e.slug);
        expect(slugs(roots)).toEqual(order.filter((s) => ["z", "a", "b"].includes(s)));
        expect(slugs(roots)).toEqual(["z", "a", "b"]);
        expect(slugs(roots[0].children)).toEqual(["k1", "k2", "k3"]);
    });

    it("does not depend on the input order", () => {
        const shape = (nodes: readonly HierarchyNode[]): unknown => nodes.map((n) => [n.employee.slug, shape(n.children)]);
        const a = shape(buildHierarchy(team));
        expect(shape(buildHierarchy([...team].reverse()))).toEqual(a);
    });

    it("treats an unknown manager as no manager", () => {
        const roots = buildHierarchy([emp("x", "Xi", "ghost"), emp("a", "Ann", null), emp("y", "Yu", "x")]);
        expect(slugs(roots)).toEqual(["a", "x"]);
        expect(slugs(roots[1].children)).toEqual(["y"]);
        expect(roots[1].level).toBe(0);
        expect(roots[1].children[0].level).toBe(1);
    });

    it("treats the employees of a two-employee cycle as top employees, and keeps the reports below them", () => {
        const list = [emp("a", "Ann", "b"), emp("b", "Bob", "a"), emp("c", "Cy", null), emp("d", "Di", "a")];
        const roots = buildHierarchy(list);
        expect(slugs(roots).sort()).toEqual(["a", "b", "c"]);
        const a = roots.find((n) => n.employee.slug === "a") as HierarchyNode;
        expect(slugs(a.children)).toEqual(["d"]);
        expect(roots.every((n) => n.level === 0)).toBe(true);
    });

    it("treats an employee that is its own manager as a top employee", () => {
        const roots = buildHierarchy([emp("a", "Ann", "a")]);
        expect(slugs(roots)).toEqual(["a"]);
        expect(roots[0].children).toEqual([]);
    });

    it("lists every employee one time, also when everybody is in a cycle", () => {
        const list = [emp("a", "Ann", "c"), emp("b", "Bob", "a"), emp("c", "Cy", "b")];
        const nodes = all(buildHierarchy(list));
        expect(nodes.map((n) => n.employee.slug).sort()).toEqual(["a", "b", "c"]);
    });

    it("works with 21 employees in 4 levels", () => {
        const list: Employee[] = [emp("ceo", "Ada", null, "CEO"), emp("pm", "Maya", "ceo", "Project Manager")];
        for (let i = 0; i < 18; i++) list.push(emp(`w${String(i).padStart(2, "0")}`, `Worker ${String(i).padStart(2, "0")}`, "pm"));
        // One report of w03: the fourth level.
        list.push(emp("deep", "Deep", "w03"));
        expect(list.length).toBe(21);
        const roots = buildHierarchy(list);
        expect(all(roots).length).toBe(21);
        const levels = hierarchyLevels(roots);
        expect(levels.map((row) => row.length)).toEqual([1, 1, 18, 1]);
    });
});

describe("hierarchyLevels", () => {
    it("lists the nodes level by level", () => {
        const levels = hierarchyLevels(buildHierarchy(team));
        expect(levels.map((row) => slugs(row))).toEqual([["ceo"], ["pm"], ["eng-lin", "eng-sam"]]);
    });

    it("gives no level for an empty forest", () => {
        expect(hierarchyLevels([])).toEqual([]);
    });
});

describe("splitReports", () => {
    const leaf = (slug: string): HierarchyNode => ({ employee: emp(slug, slug, "boss"), level: 1, children: [] });
    const parent = (slug: string): HierarchyNode => ({ employee: emp(slug, slug, "boss"), level: 1, children: [leaf(`${slug}-child`)] });

    it("keeps all reports in the row when there are fewer than GROUP_MIN reports with no reports of their own", () => {
        const kids = [leaf("a"), parent("b"), leaf("c"), leaf("d"), leaf("e")];
        expect(kids.filter((k) => k.children.length === 0)).toHaveLength(GROUP_MIN - 1);
        const split = splitReports(kids);
        expect(split.group).toEqual([]);
        expect(split.nodes.map((n) => n.employee.slug)).toEqual(["a", "b", "c", "d", "e"]);
    });

    it("moves the reports with no reports of their own in one block, and keeps the other reports in order", () => {
        const kids = [leaf("a"), parent("b"), leaf("c"), leaf("d"), parent("e"), leaf("f"), leaf("g")];
        const split = splitReports(kids);
        expect(split.group.map((n) => n.employee.slug)).toEqual(["a", "c", "d", "f", "g"]);
        expect(split.nodes.map((n) => n.employee.slug)).toEqual(["b", "e"]);
    });

    it("makes one block of 20 leaves", () => {
        const kids = Array.from({ length: 20 }, (_, i) => leaf(`w${i}`));
        const split = splitReports(kids);
        expect(split.nodes).toEqual([]);
        expect(split.group).toHaveLength(20);
    });

    it("gives nothing for no reports", () => {
        expect(splitReports([])).toEqual({ nodes: [], group: [] });
    });
});

describe("groupColumns", () => {
    it("uses two rows or more, with at most 6 columns", () => {
        expect(groupColumns(5)).toBe(3);
        expect(groupColumns(8)).toBe(4);
        expect(groupColumns(12)).toBe(6);
        expect(groupColumns(18)).toBe(6);
        expect(groupColumns(1)).toBe(1);
    });
});
