// Slug rules from docs/ipc-m1.md: [a-z0-9][a-z0-9-]*, unique in the floor.

const SLUG_RE = /^[a-z0-9][a-z0-9-]*$/;

export function isValidSlug(slug: string): boolean {
    return SLUG_RE.test(slug);
}

/** Make a slug from a display name. Returns "" when the name has no usable character. */
export function slugify(name: string): string {
    return name
        .normalize("NFKD")
        .replace(/[̀-ͯ]/g, "")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "");
}

/** `base`, or `base-2`, `base-3`, ... when `taken` already has it. */
export function uniqueSlug(base: string, taken: Iterable<string>): string {
    if (base === "") return "";
    const used = new Set(taken);
    if (!used.has(base)) return base;
    let n = 2;
    while (used.has(`${base}-${n}`)) n++;
    return `${base}-${n}`;
}
