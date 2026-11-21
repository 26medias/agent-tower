// One stable order for the employees of a floor. The room, the roster and the tabs all use it.
// The CEO comes first, then the Project Manager, then the others. The others go by the depth of
// their manager chain (an employee with no manager has depth 0), then by name, then by slug.
// The order depends only on the roles, the hierarchy and the names, not on the order in which
// the Rust side lists the employees.
import type { Employee } from "./types";

function byName(a: Employee, b: Employee): number {
    const n = a.name.localeCompare(b.name, "en", { sensitivity: "base" });
    if (n !== 0) return n;
    return a.slug < b.slug ? -1 : a.slug > b.slug ? 1 : 0;
}

/** 0 for the CEO, 1 for the Project Manager, 2 for any other role. */
export function roleRank(role: string): number {
    const r = role.trim().toLowerCase();
    if (r === "ceo") return 0;
    if (r === "project manager") return 1;
    return 2;
}

export function orderEmployees(employees: readonly Employee[]): Employee[] {
    const bySlug = new Map(employees.map((e) => [e.slug, e]));
    // The number of managers above an employee. A manager that is not in the floor ends the chain.
    // A manager cycle ends the chain too, so that the walk always stops.
    const depthOf = (e: Employee): number => {
        let depth = 0;
        const seen = new Set<string>([e.slug]);
        let manager = e.manager;
        while (manager !== null && bySlug.has(manager) && !seen.has(manager)) {
            seen.add(manager);
            depth += 1;
            manager = (bySlug.get(manager) as Employee).manager;
        }
        return depth;
    };
    const keyed = employees.map((e) => ({ e, rank: roleRank(e.role), depth: depthOf(e) }));
    keyed.sort((a, b) => a.rank - b.rank || a.depth - b.depth || byName(a.e, b.e));
    return keyed.map((k) => k.e);
}
