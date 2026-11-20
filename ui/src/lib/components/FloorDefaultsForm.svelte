<script lang="ts">
    // The floor defaults of the settings panel: selects for CLI, model, effort and policy. Each
    // select has "(not set)" first. The model list is the list of the selected CLI. With no CLI
    // the model and effort selects are disabled and their values go to null.
    import { untrack } from "svelte";
    import { afterCliChange, afterModelChange, effortChoices, effortList, listFor, modelChoices } from "$lib/models";
    import { store } from "$lib/store.svelte";
    import type { CliKind, FloorDefaults, Policy } from "$lib/types";

    const initial = untrack(() => store.floorView?.defaults ?? null);

    let cli = $state<CliKind | "">(initial?.cli ?? "");
    let model = $state(initial?.model ?? "");
    let effort = $state(initial?.effort ?? "");
    let policy = $state<Policy | "">(initial?.policy ?? "");
    let busy = $state(false);

    const list = $derived(cli === "" ? [] : listFor(store.models, cli));
    const modelOptions = $derived(modelChoices(list, model));
    const effortOptions = $derived(effortChoices(effortList(list, model, null), effort));

    const wanted = $derived<FloorDefaults>({
        cli: cli === "" ? null : cli,
        model: cli === "" || model === "" ? null : model,
        effort: cli === "" || effort === "" ? null : effort,
        policy: policy === "" ? null : policy,
    });
    const saved = $derived(store.floorView?.defaults ?? null);
    const dirty = $derived(
        !saved ||
            saved.cli !== wanted.cli ||
            saved.model !== wanted.model ||
            saved.effort !== wanted.effort ||
            saved.policy !== wanted.policy,
    );

    function changeCli(value: string): void {
        const next: CliKind | "" = value === "claude" || value === "codex" ? value : "";
        cli = next;
        if (next === "") {
            model = "";
            effort = "";
            return;
        }
        const fixed = afterCliChange(listFor(store.models, next), model, effort, null);
        model = fixed.model;
        effort = fixed.effort;
    }

    function changeModel(value: string): void {
        model = value;
        effort = afterModelChange(list, value, effort, null);
    }

    async function save(): Promise<void> {
        if (busy || !dirty) return;
        busy = true;
        await store.setFloorDefaults(wanted);
        busy = false;
    }
</script>

<form
    class="defaults"
    aria-label="Floor defaults"
    onsubmit={(e) => {
        e.preventDefault();
        void save();
    }}
>
    <div class="grid">
        <label>
            <span>CLI</span>
            <select name="defaultCli" value={cli} onchange={(e) => changeCli(e.currentTarget.value)}>
                <option value="">(not set)</option>
                <option value="claude">claude</option>
                <option value="codex">codex</option>
            </select>
        </label>
        <label>
            <span>Policy</span>
            <select name="defaultPolicy" value={policy} onchange={(e) => (policy = e.currentTarget.value as Policy | "")}>
                <option value="">(not set)</option>
                <option value="read_only">read_only</option>
                <option value="edit_in_worktree">edit_in_worktree</option>
                <option value="ask_always">ask_always</option>
            </select>
        </label>
        <label>
            <span>Model</span>
            <select name="defaultModel" disabled={cli === ""} value={model} onchange={(e) => changeModel(e.currentTarget.value)}>
                <option value="">(not set)</option>
                {#each modelOptions as choice (choice.value)}
                    <option value={choice.value}>{choice.label}</option>
                {/each}
            </select>
        </label>
        <label>
            <span>Effort</span>
            <select name="defaultEffort" disabled={cli === ""} value={effort} onchange={(e) => (effort = e.currentTarget.value)}>
                <option value="">(not set)</option>
                {#each effortOptions as choice (choice.value)}
                    <option value={choice.value}>{choice.label}</option>
                {/each}
            </select>
        </label>
    </div>
    <p class="hint">
        An employee with no model or effort of its own gets these. With a CLI set, only employees of that CLI get them.
    </p>
    <div class="buttons">
        <button type="submit" class="primary" disabled={!dirty || busy}>save defaults</button>
    </div>
</form>

<style>
    .defaults {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 6px 8px;
    }

    label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-width: 0;
    }

    label span {
        font-size: 11px;
        color: var(--dim);
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    select:disabled {
        opacity: 0.5;
    }

    .hint {
        margin: 0;
        color: var(--dim);
        font-size: 11px;
    }

    .buttons {
        display: flex;
        justify-content: flex-end;
    }
</style>
