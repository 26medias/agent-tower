<script lang="ts">
    // The floor defaults of the settings panel: selects for CLI, model, effort and policy. Each
    // select has "(not set)" first. The model list is the list of the selected CLI. With no CLI
    // the model and effort selects are disabled and their values go to null.
    import { untrack } from "svelte";
    import { onDestroy } from "svelte";
    import { afterCliChange, afterModelChange, effortChoices, effortList, listFor, modelChoices } from "$lib/models";
    import { POLICY_IDS, POLICY_INFO } from "$lib/policies";
    import { store } from "$lib/store.svelte";
    import ActionError from "./ActionError.svelte";
    import type { CliKind, FloorDefaults, Policy } from "$lib/types";

    const initial = untrack(() => store.floorView?.defaults ?? null);

    let cli = $state<CliKind | "">(initial?.cli ?? "");
    let model = $state(initial?.model ?? "");
    let effort = $state(initial?.effort ?? "");
    let policy = $state<Policy | "">(initial?.policy ?? "");
    let busy = $state(false);
    // "Saved" shows for 2 s after a good save.
    let savedNote = $state(false);
    let savedTimer: ReturnType<typeof setTimeout> | undefined;
    onDestroy(() => clearTimeout(savedTimer));

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
        if (busy || !dirty || store.gone) return;
        busy = true;
        const ok = await store.setFloorDefaults(wanted);
        busy = false;
        if (ok) {
            savedNote = true;
            clearTimeout(savedTimer);
            savedTimer = setTimeout(() => (savedNote = false), 2000);
        }
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
            <select name="defaultCli" value={cli} disabled={store.gone} title={store.goneTitle()} onchange={(e) => changeCli(e.currentTarget.value)}>
                <option value="">(not set)</option>
                <option value="claude">claude</option>
                <option value="codex">codex</option>
            </select>
        </label>
        <label>
            <span>Policy</span>
            <select name="defaultPolicy" value={policy} disabled={store.gone} title={store.goneTitle()} onchange={(e) => (policy = e.currentTarget.value as Policy | "")}>
                <option value="">(not set)</option>
                {#each POLICY_IDS as id (id)}
                    <option value={id}>{POLICY_INFO[id].label}</option>
                {/each}
            </select>
        </label>
        <label>
            <span>Model</span>
            <select name="defaultModel" disabled={cli === "" || store.gone} title={store.goneTitle()} value={model} onchange={(e) => changeModel(e.currentTarget.value)}>
                <option value="">{cli === "" ? "Choose a CLI first" : "(not set)"}</option>
                {#each modelOptions as choice (choice.value)}
                    <option value={choice.value}>{choice.label}</option>
                {/each}
            </select>
        </label>
        <label>
            <span>Effort</span>
            <select name="defaultEffort" disabled={cli === "" || store.gone} title={store.goneTitle()} value={effort} onchange={(e) => (effort = e.currentTarget.value)}>
                <option value="">{cli === "" ? "Choose a CLI first" : "(not set)"}</option>
                {#each effortOptions as choice (choice.value)}
                    <option value={choice.value}>{choice.label}</option>
                {/each}
            </select>
        </label>
    </div>
    <p class="hint">
        An employee with no model or effort of its own gets these. With a CLI set, only employees of that CLI get them.
    </p>
    <ActionError area="defaults" />
    <div class="buttons">
        {#if savedNote}
            <span class="saved" role="status" data-testid="defaults-saved">Saved</span>
        {:else if !dirty}
            <span class="reason" data-testid="defaults-reason">No change to save.</span>
        {/if}
        <button type="submit" class="primary" disabled={!dirty || busy || store.gone} title={store.goneTitle()}>save defaults</button>
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
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
    }

    .saved {
        color: var(--ok);
        font-weight: 700;
    }

    .reason {
        color: var(--dim);
        font-size: 12px;
    }
</style>
