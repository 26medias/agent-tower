// A stable order for the employees of a floor: the order of the hierarchy.
// An employee with no manager comes first, then its reports, depth first. Siblings go by name
// (then by slug). The order depends only on the hierarchy and the names, not on the order in
// which the Rust side lists the employees.
import type { Employee } from "./types";

function byName(a: Employee, b: Employee): number {
    const n = a.name.localeCompare(b.name, "en", { sensitivity: "base" });
    if (n !== 0) return n;
    return a.slug < b.slug ? -1 : a.slug > b.slug ? 1 : 0;
}

export function orderEmployees(employees: readonly Employee[]): Employee[] {
    const slugs = new Set(employees.map((e) => e.slug));
    const children = new Map<string, Employee[]>();
    const roots: Employee[] = [];
    for (const e of employees) {
        // A manager that is not in the floor (or the employee itself) makes a root.
        if (e.manager === null || e.manager === e.slug || !slugs.has(e.manager)) {
            roots.push(e);
        } else {
            const list = children.get(e.manager) ?? [];
            list.push(e);
            children.set(e.manager, list);
        }
    }
    const out: Employee[] = [];
    const seen = new Set<string>();
    const visit = (e: Employee): void => {
        if (seen.has(e.slug)) return;
        seen.add(e.slug);
        out.push(e);
        for (const child of (children.get(e.slug) ?? []).sort(byName)) visit(child);
    };
    for (const root of roots.sort(byName)) visit(root);
    // A manager cycle has no root. Add what is left, so that no employee is lost.
    for (const e of [...employees].sort(byName)) visit(e);
    return out;
}
