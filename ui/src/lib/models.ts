// Pure rules for the model and effort selects (docs/ipc-settings.md, sections 2, 5 and 6).
// The components only draw what these functions return, so a unit test can check the rules.
import type { CliKind, FloorDefaults, ModelCatalog, ModelOption } from "./types";

/** The efforts of a custom model, and the list for a model that has no list of its own. */
export const STANDARD_EFFORTS: readonly string[] = ["low", "medium", "high", "xhigh", "max"];

/** 1 to 100 characters from A-Z a-z 0-9 . _ : / [ ] -, and the first one is not `-`. */
export function isValidModelName(name: string): boolean {
    return /^[A-Za-z0-9._:/[\]][A-Za-z0-9._:/[\]-]{0,99}$/.test(name);
}

/** 1 to 20 characters from a-z. */
export function isValidEffort(effort: string): boolean {
    return /^[a-z]{1,20}$/.test(effort);
}

/** One entry of a select. `unknown` marks a stored value that is not in the list. */
export interface Choice {
    value: string;
    label: string;
    unknown: boolean;
}

/** The text of a model in a list: the label, and the ID when it is not the same. */
export function modelText(model: ModelOption): string {
    return model.label === model.id ? model.id : `${model.label} (${model.id})`;
}

/** The list of the models of a CLI. An empty list while the catalog is not loaded. */
export function listFor(catalog: ModelCatalog | null, cli: CliKind): ModelOption[] {
    if (!catalog) return [];
    return cli === "claude" ? catalog.claude : catalog.codex;
}

/**
 * The model that the floor defaults give to an employee of this CLI (section 5): the default
 * model, when the floor has no default CLI or its default CLI is this one. Otherwise null.
 */
export function defaultModelFor(defaults: FloorDefaults | null | undefined, cli: CliKind): string | null {
    if (!defaults || !defaults.model) return null;
    return defaults.cli === null || defaults.cli === cli ? defaults.model : null;
}

/** The efforts of a model of the list, or null when the model is not in the list. */
export function effortsOf(list: ModelOption[], id: string): string[] | null {
    const found = list.find((m) => m.id === id);
    return found ? found.efforts : null;
}

/**
 * The efforts to offer for a model. `model` is the selected model, "" for "no model chosen".
 * With no model chosen, `fallbackModel` is the model that will be used (the floor default), or null.
 * A model that is not in the list (a custom one, or a stored unknown one) gets the standard efforts.
 */
export function effortList(list: ModelOption[], model: string, fallbackModel: string | null): string[] {
    const id = model !== "" ? model : fallbackModel;
    if (id === null) return [...STANDARD_EFFORTS];
    return effortsOf(list, id) ?? [...STANDARD_EFFORTS];
}

/** The entries of a model select (after the first fixed option). A stored unknown model is added. */
export function modelChoices(list: ModelOption[], current: string): Choice[] {
    const choices: Choice[] = list.map((m) => ({ value: m.id, label: modelText(m), unknown: false }));
    if (current !== "" && !list.some((m) => m.id === current)) {
        choices.push({ value: current, label: `${current} (not in the list)`, unknown: true });
    }
    return choices;
}

/** The entries of an effort select (after the first fixed option). A stored unknown effort is added. */
export function effortChoices(efforts: string[], current: string): Choice[] {
    const choices: Choice[] = efforts.map((e) => ({ value: e, label: e, unknown: false }));
    if (current !== "" && !efforts.includes(current)) {
        choices.push({ value: current, label: `${current} (not in the list)`, unknown: true });
    }
    return choices;
}

/**
 * The operator changed the CLI. A model that is not in the new list goes to "" (floor default).
 * An effort that the new model does not have goes to "" (default). `fallbackModel` is the model of
 * the floor defaults for the new CLI.
 */
export function afterCliChange(
    list: ModelOption[],
    model: string,
    effort: string,
    fallbackModel: string | null,
): { model: string; effort: string } {
    const nextModel = model !== "" && list.some((m) => m.id === model) ? model : "";
    return { model: nextModel, effort: afterModelChange(list, nextModel, effort, fallbackModel) };
}

/** The operator changed the model: an effort that the new model does not have goes to "". */
export function afterModelChange(list: ModelOption[], model: string, effort: string, fallbackModel: string | null): string {
    if (effort === "") return "";
    return effortList(list, model, fallbackModel).includes(effort) ? effort : "";
}

/** "floor default (X)", or "floor default (CLI default)" when the floor gives no model. */
export function floorDefaultLabel(defaults: FloorDefaults | null | undefined, cli: CliKind): string {
    return `floor default (${defaultModelFor(defaults, cli) ?? "CLI default"})`;
}

/**
 * The model text for an employee: the model that its session will get. A model of its own wins.
 * With none, the floor default applies when it is set for this CLI (see `defaultModelFor`):
 * "haiku (floor default)". Otherwise the CLI picks its own model: "CLI default".
 */
export function modelDisplay(
    employee: { cli: CliKind; model: string | null },
    defaults: FloorDefaults | null | undefined,
): string {
    if (employee.model) return employee.model;
    const fallback = defaultModelFor(defaults, employee.cli);
    return fallback ? `${fallback} (floor default)` : "CLI default";
}
