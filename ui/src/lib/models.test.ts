import { describe, expect, it } from "vitest";
import {
    STANDARD_EFFORTS,
    afterCliChange,
    afterModelChange,
    defaultModelFor,
    effortChoices,
    effortList,
    floorDefaultLabel,
    isValidEffort,
    isValidModelName,
    listFor,
    modelChoices,
    modelDisplay,
    modelText,
} from "./models";
import type { FloorDefaults, ModelCatalog, ModelOption } from "./types";

const opt = (id: string, efforts: string[], label = id, custom = false): ModelOption => ({
    id,
    label,
    efforts,
    defaultEffort: null,
    custom,
});

const catalog: ModelCatalog = {
    claude: [opt("haiku", ["low", "medium"], "Haiku"), opt("opus", ["low", "max"], "Opus")],
    codex: [opt("gpt-a", ["low", "ultra"], "GPT A"), opt("gpt-b", ["medium"], "GPT B"), opt("mine", [...STANDARD_EFFORTS], "mine", true)],
    image: [],
    codexSource: "cache",
};

const none: FloorDefaults = { cli: null, model: null, effort: null, policy: null };

describe("model names", () => {
    it("accepts the contract characters and refuses a leading dash", () => {
        for (const ok of ["haiku", "gpt-6-luna", "a.b_c:d/e[1m]", "a", "a".repeat(100), "x-"]) {
            expect(isValidModelName(ok), ok).toBe(true);
        }
        for (const bad of ["", "-x", "--foo", "a b", "a\nb", "é", "a;b", "a".repeat(101)]) {
            expect(isValidModelName(bad), JSON.stringify(bad)).toBe(false);
        }
    });

    it("accepts efforts of 1 to 20 lowercase letters", () => {
        expect(isValidEffort("xhigh")).toBe(true);
        expect(isValidEffort("High")).toBe(false);
        expect(isValidEffort("")).toBe(false);
        expect(isValidEffort("a".repeat(21))).toBe(false);
    });
});

describe("the list of a CLI", () => {
    it("is the list of that CLI, and empty before the catalog is loaded", () => {
        expect(listFor(catalog, "claude").map((m) => m.id)).toEqual(["haiku", "opus"]);
        expect(listFor(catalog, "codex").map((m) => m.id)).toEqual(["gpt-a", "gpt-b", "mine"]);
        expect(listFor(null, "codex")).toEqual([]);
    });

    it("shows the label and the ID", () => {
        expect(modelText(opt("haiku", [], "Haiku"))).toBe("Haiku (haiku)");
        expect(modelText(opt("x", [], "x"))).toBe("x");
    });
});

describe("the floor default model", () => {
    it("goes to an employee when the floor has no default CLI or the same CLI", () => {
        expect(defaultModelFor({ ...none, model: "m" }, "claude")).toBe("m");
        expect(defaultModelFor({ ...none, model: "m", cli: "claude" }, "claude")).toBe("m");
        expect(defaultModelFor({ ...none, model: "m", cli: "codex" }, "claude")).toBeNull();
        expect(defaultModelFor(none, "claude")).toBeNull();
        expect(defaultModelFor(null, "claude")).toBeNull();
    });

    it("gives the text of the first option of the model select", () => {
        expect(floorDefaultLabel({ ...none, model: "haiku" }, "claude")).toBe("floor default (haiku)");
        expect(floorDefaultLabel({ ...none, model: "haiku", cli: "codex" }, "claude")).toBe("floor default (CLI default)");
        expect(floorDefaultLabel(none, "codex")).toBe("floor default (CLI default)");
    });
});

describe("efforts", () => {
    const list = catalog.codex;

    it("are the efforts of the selected model", () => {
        expect(effortList(list, "gpt-a", null)).toEqual(["low", "ultra"]);
        expect(effortList(list, "gpt-b", "gpt-a")).toEqual(["medium"]);
    });

    it("are the efforts of the floor default model when no model is selected", () => {
        expect(effortList(list, "", "gpt-a")).toEqual(["low", "ultra"]);
    });

    it("are the 5 standard efforts with no model, and for a model that is not in the list", () => {
        expect(effortList(list, "", null)).toEqual([...STANDARD_EFFORTS]);
        expect(effortList(list, "", "unknown-default")).toEqual([...STANDARD_EFFORTS]);
        expect(effortList(list, "unknown", null)).toEqual([...STANDARD_EFFORTS]);
        expect(effortList(list, "mine", null)).toEqual([...STANDARD_EFFORTS]);
    });
});

describe("the entries of the selects", () => {
    it("add a stored model that is not in the list, with the text '(not in the list)'", () => {
        const choices = modelChoices(catalog.claude, "ghost");
        expect(choices.map((c) => c.value)).toEqual(["haiku", "opus", "ghost"]);
        expect(choices[2]).toEqual({ value: "ghost", label: "ghost (not in the list)", unknown: true });
        expect(choices[0].label).toBe("Haiku (haiku)");
        expect(modelChoices(catalog.claude, "opus").some((c) => c.unknown)).toBe(false);
        expect(modelChoices(catalog.claude, "").some((c) => c.unknown)).toBe(false);
    });

    it("add a stored effort that is not in the list", () => {
        const choices = effortChoices(["low", "high"], "ultra");
        expect(choices.map((c) => c.label)).toEqual(["low", "high", "ultra (not in the list)"]);
        expect(effortChoices(["low"], "low").some((c) => c.unknown)).toBe(false);
        expect(effortChoices(["low"], "").some((c) => c.unknown)).toBe(false);
    });
});

describe("a change of CLI", () => {
    const codexList = catalog.codex;
    const claudeList = catalog.claude;

    it("resets a model of the other CLI to the floor default", () => {
        expect(afterCliChange(codexList, "haiku", "", null)).toEqual({ model: "", effort: "" });
    });

    it("keeps a model that is in the new list", () => {
        expect(afterCliChange(codexList, "gpt-a", "ultra", null)).toEqual({ model: "gpt-a", effort: "ultra" });
    });

    it("resets a stored unknown model too", () => {
        expect(afterCliChange(claudeList, "ghost", "low", null).model).toBe("");
    });

    it("resets an effort that the new model does not have", () => {
        expect(afterCliChange(codexList, "gpt-b", "ultra", null)).toEqual({ model: "gpt-b", effort: "" });
        // The model went to the floor default: its efforts decide.
        expect(afterCliChange(codexList, "haiku", "max", "gpt-a")).toEqual({ model: "", effort: "" });
        expect(afterCliChange(codexList, "haiku", "low", "gpt-a")).toEqual({ model: "", effort: "low" });
    });
});

describe("a change of model", () => {
    it("keeps an effort that the new model has and resets one that it does not have", () => {
        expect(afterModelChange(catalog.codex, "gpt-a", "ultra", null)).toBe("ultra");
        expect(afterModelChange(catalog.codex, "gpt-b", "ultra", null)).toBe("");
        expect(afterModelChange(catalog.codex, "gpt-b", "", null)).toBe("");
    });
});

describe("modelDisplay", () => {
    const none: FloorDefaults = { cli: null, model: null, effort: null, policy: null };
    it("shows the model of the employee first", () => {
        expect(modelDisplay({ cli: "claude", model: "opus" }, { ...none, model: "haiku" })).toBe("opus");
    });
    it("shows the floor default when the employee has no model", () => {
        expect(modelDisplay({ cli: "claude", model: null }, { ...none, model: "haiku" })).toBe("haiku (floor default)");
        expect(modelDisplay({ cli: "claude", model: null }, { cli: "claude", model: "haiku", effort: null, policy: null })).toBe(
            "haiku (floor default)",
        );
    });
    it("shows CLI default when no default applies", () => {
        expect(modelDisplay({ cli: "claude", model: null }, none)).toBe("CLI default");
        expect(modelDisplay({ cli: "codex", model: null }, { cli: "claude", model: "haiku", effort: null, policy: null })).toBe(
            "CLI default",
        );
        expect(modelDisplay({ cli: "claude", model: null }, null)).toBe("CLI default");
    });
});
