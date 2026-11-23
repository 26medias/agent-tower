// The organisation chart of a floor: a tree built from the `manager` field of each employee.
// Pure logic: no Svelte, no Tauri.
//   - A top employee has no manager, or a manager that is not in the floor.
//   - An employee in a manager cycle is a top employee too. Its own `manager` field is ignored.
//   - The direct reports of an employee are its children. Siblings keep the order of `order.ts`.
import { orderEmployees } from "./order";
import type { Employee } from "./types";

export interface HierarchyNode {
    employee: Employee;
    /** 0 for a top employee, 1 for its direct reports, and so on. */
    level: number;
    children: HierarchyNode[];
}

/** True when the manager chain of the employee comes back to the employee (a self-manager counts). */
function inCycle(start: Employee, bySlug: ReadonlyMap<string, Employee>): boolean {
    const seen = new Set<string>();
    let manager = start.manager;
    while (manager !== null && bySlug.has(manager)) {
        if (manager === start.slug) return true;
        // The chain entered a cycle that does not hold `start`. It never comes back to `start`.
        if (seen.has(manager)) return false;
        seen.add(manager);
        manager = (bySlug.get(manager) as Employee).manager;
    }
    return false;
}

/**
 * The top employees, each with its tree of reports. The top employees and each list of siblings are in the order of
 * `orderEmployees`. Each employee is in the tree exactly one time, and the walk always stops.
 */
export function buildHierarchy(employees: readonly Employee[]): HierarchyNode[] {
    const ordered = orderEmployees(employees);
    // The slug of an employee is unique in a floor (docs/ipc-m1.md).
    const bySlug = new Map(ordered.map((e) => [e.slug, e]));

    const childrenOf = new Map<string, Employee[]>();
    const tops: Employee[] = [];
    for (const e of ordered) {
        const parent = e.manager !== null && bySlug.has(e.manager) && !inCycle(e, bySlug) ? e.manager : null;
        if (parent === null) {
            tops.push(e);
        } else {
            const list = childrenOf.get(parent) ?? [];
            list.push(e);
            childrenOf.set(parent, list);
        }
    }

    const make = (employee: Employee, level: number): HierarchyNode => ({
        employee,
        level,
        children: (childrenOf.get(employee.slug) ?? []).map((child) => make(child, level + 1)),
    });
    return tops.map((e) => make(e, 0));
}

/** The nodes of a forest, level by level (the top employees first). */
export function hierarchyLevels(roots: readonly HierarchyNode[]): HierarchyNode[][] {
    const levels: HierarchyNode[][] = [];
    let row = [...roots];
    while (row.length > 0) {
        levels.push(row);
        row = row.flatMap((node) => node.children);
    }
    return levels;
}

/** From this many reports with no reports of their own, the chart draws those reports as one block (a grid), not as one long row. */
export const GROUP_MIN = 5;

/** The most columns of such a block. */
export const GROUP_COLS_MAX = 6;

/**
 * Split the reports of one employee for the chart. The reports that have no reports of their own go in one block when
 * there are `GROUP_MIN` of them or more: a long row of cards would be wider than any window. `nodes` keeps the other
 * reports, and all of them when there is no block. Each list keeps the order of `order.ts`.
 */
export function splitReports(children: readonly HierarchyNode[], min = GROUP_MIN): { nodes: HierarchyNode[]; group: HierarchyNode[] } {
    const leaves = children.filter((c) => c.children.length === 0);
    if (leaves.length < min) return { nodes: [...children], group: [] };
    return { nodes: children.filter((c) => c.children.length > 0), group: leaves };
}

/** The columns of a block of `count` cards: two rows or more for a big block, at most `GROUP_COLS_MAX` columns. */
export function groupColumns(count: number): number {
    return Math.max(1, Math.min(GROUP_COLS_MAX, Math.ceil(count / 2)));
}
