# Contract: settings panel and model lists

This file adds to `docs/ipc-m1.md` and `docs/ipc-visuals-init.md`. All rules of those files stay in
effect (snake_case command names, camelCase JSON fields, string errors).

## 1. Scope

1. One settings panel in the UI: API keys, image model, model lists, floor defaults, program paths.
2. The operator selects a model and an effort from a list. The operator does not type a model name
   in the employee form or in the floor defaults.

## 2. Where the model lists come from

- **Codex:** the file `models_cache.json` in the Codex home (`$CODEX_HOME`, or `~/.codex`). The app
  only reads this file. It uses the entries with `visibility` = `"list"`, in the file order:
  `slug` (the ID), `display_name` (the label), `supported_reasoning_levels[].effort` (the efforts),
  `default_reasoning_level`. If the file is missing or does not parse, the app uses a built-in list
  and `codexSource` is `"builtin"`.
- **Claude:** a built-in list of the aliases `fable`, `opus`, `sonnet`, `haiku`, each with the
  efforts `low`, `medium`, `high`, `xhigh`, `max` (from `claude --help`). Claude Code has no local
  model file.
- **Image:** a built-in list: `gpt-image-2.6-sunburst`, `gpt-image-2.6-flare`.
- **Custom models:** the operator can add a full model name one time in the settings panel. The app
  stores it in the app config file (`programs.toml`), table `[models]`, keys `claude`, `codex`,
  `image` (arrays of strings). A custom model comes after the other models of its list. A custom
  model of `claude` or `codex` has the efforts `low`, `medium`, `high`, `xhigh`, `max`.
- A model name is valid if it has 1 to 100 characters, each from `A-Z a-z 0-9 . _ : / [ ] -`, and
  the first character is not `-`. The Rust side refuses other names in `models_set_custom`, in
  `employee_save` (model), in `floor_defaults_set` (model) and in `settings_set` (image model).
  An effort is valid if it has 1 to 20 characters from `a-z`.

## 3. Types (additions)

```ts
interface ModelOption {
    id: string;                    // the value that goes to the CLI
    label: string;                 // text for the list
    efforts: string[];             // empty for an image model
    defaultEffort: string | null;
    custom: boolean;               // from the [models] table of the app config
}

interface ModelCatalog {
    claude: ModelOption[];
    codex: ModelOption[];
    image: ModelOption[];
    codexSource: "cache" | "builtin";
}

interface FloorDefaults {          // [defaults] of floor.toml
    cli: CliKind | null;
    model: string | null;
    effort: string | null;
    policy: Policy | null;
}

interface FloorView {              // existing fields stay
    defaults: FloorDefaults;
}
```

## 4. Commands (additions)

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `models_get` | none | `ModelCatalog` | Reads the Codex file again on each call |
| `models_set_custom` | `kind` (`claude`, `codex` or `image`), `models` (string[]) | `ModelCatalog` | Replaces the custom list of that kind. Removes duplicates and names that are already in the list |
| `floor_defaults_set` | `path`, `defaults` (`FloorDefaults`) | `FloorView` | Writes `[defaults]` of `floor.toml`. A null field goes out of the file. Keeps the other content. Sends `floor-changed` |

`settings_get` and `settings_set` do not change.

## 5. Behaviour change at session start

The model and the effort of `[defaults]` go to an employee (or to the setup session) only if the
employee has no model of its own AND (`defaults.cli` is not set OR `defaults.cli` is the CLI of the
employee). Reason: a Claude model name must not go to Codex. An employee with its own model and no
effort gets no default effort from another CLI either (same condition).

## 6. UI behaviour

- A "Settings" button (gear) in the building column opens a modal panel. Escape and a click on the
  backdrop close it. Sections:
    1. **API keys.** OpenAI key: shows "set (app config)", "set (environment)" or "not set". A
       password field sets it; a "remove" button removes it from the app config (only when the
       source is the config). The panel never shows the key.
    2. **Logo image model.** A select with the `image` list. A stored model that is not in the list
       shows as one more option.
    3. **Models.** For Claude and for Codex: the list (label and ID), with where the Codex list
       comes from. A text field plus "add" adds a custom model; each custom model has "remove".
       The same for image models.
    4. **Floor defaults** (only when a floor is open): selects for CLI, model, effort, policy. Each
       has the first option "(not set)". The model list is the list of the selected CLI; with no
       CLI selected the model and effort selects are disabled and their values go to null.
    5. **Programs.** The content of the old programs panel moves here.
- **Employee form.** Model: a select with "floor default (X)" first (X = the default model that
  section 5 gives to this CLI, or "CLI default" when there is none), then the list of the selected
  CLI. Effort: a select with "default" first, then the efforts of the selected model (for "floor
  default": the efforts of that model, or the 5 standard efforts). A stored model or effort that is
  not in the list shows as one more option with the text "(not in the list)", so that a save does
  not change it silently. When the operator changes the CLI, a model that is not in the new list
  goes to "floor default", and an effort that the new model does not have goes to "default".
- The key field that `OfficeHeader` shows on `NO_KEY:` stays.
- The mock (`ui/src/lib/mock.ts`) supports the new commands.
