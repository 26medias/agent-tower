<script lang="ts">
    // One model list in the settings panel: the models with label and ID, a field to add a custom
    // model, and a "remove" button on each custom model.
    import { isValidModelName, modelText } from "$lib/models";
    import { store } from "$lib/store.svelte";
    import ActionError from "./ActionError.svelte";
    import type { ModelKind, ModelOption } from "$lib/types";

    let {
        kind,
        title,
        models,
        note = "",
    }: { kind: ModelKind; title: string; models: ModelOption[]; note?: string } = $props();

    let draft = $state("");
    let busy = $state(false);

    const customIds = $derived(models.filter((m) => m.custom).map((m) => m.id));
    const trimmed = $derived(draft.trim());
    const problem = $derived(
        trimmed === ""
            ? ""
            : !isValidModelName(trimmed)
              ? "A name has 1 to 100 characters from A-Z a-z 0-9 . _ : / [ ] - and does not start with a dash."
              : models.some((m) => m.id === trimmed)
                ? "This model is in the list already."
                : "",
    );

    async function add(): Promise<void> {
        if (trimmed === "" || problem !== "" || busy) return;
        busy = true;
        const ok = await store.setCustomModels(kind, [...customIds, trimmed]);
        busy = false;
        if (ok) draft = "";
    }

    async function remove(id: string): Promise<void> {
        if (busy) return;
        busy = true;
        await store.setCustomModels(
            kind,
            customIds.filter((c) => c !== id),
        );
        busy = false;
    }
</script>

<div class="list" data-kind={kind}>
    <div class="head">
        <h4>{title}</h4>
        {#if note}<span class="note">{note}</span>{/if}
    </div>
    <ul aria-label="{title} models">
        {#each models as model (model.id)}
            <li data-model={model.id} class:custom={model.custom}>
                <span class="text" title={modelText(model)}>{modelText(model)}</span>
                {#if model.custom}
                    <span class="tag">custom</span>
                    <button type="button" class="ghost remove" disabled={busy} onclick={() => void remove(model.id)}>remove</button>
                {/if}
            </li>
        {:else}
            <li class="none">No model.</li>
        {/each}
    </ul>
    <form
        onsubmit={(e) => {
            e.preventDefault();
            void add();
        }}
    >
        <input
            type="text"
            placeholder="add a model name"
            aria-label="Add a custom {title} model"
            autocomplete="off"
            spellcheck="false"
            bind:value={draft}
        />
        <button type="submit" class="add" disabled={trimmed === "" || problem !== "" || busy}>add</button>
    </form>
    {#if problem}<p class="problem">{problem}</p>{/if}
    <ActionError area="models:{kind}" />
</div>

<style>
    .list {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-width: 0;
    }

    .head {
        display: flex;
        align-items: baseline;
        gap: 8px;
        flex-wrap: wrap;
    }

    h4 {
        margin: 0;
        font-size: 12px;
        color: var(--text);
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    .note {
        color: var(--dim);
        font-size: 11px;
    }

    ul {
        list-style: none;
        margin: 0;
        padding: 0;
        max-height: 150px;
        overflow-y: auto;
        border: 1px solid var(--line);
        border-radius: 6px;
        background: var(--bg-deep);
    }

    li {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 2px 8px;
        font-size: 12px;
    }

    li + li {
        border-top: 1px solid var(--line);
    }

    .text {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-family: var(--mono);
    }

    .tag {
        color: var(--accent);
        font-size: 11px;
    }

    .none {
        color: var(--dim);
    }

    button.remove {
        padding: 0 6px;
        font-size: 11px;
    }

    form {
        display: flex;
        gap: 4px;
    }

    input {
        flex: 1;
        font-size: 12px;
    }

    .problem {
        margin: 0;
        color: var(--danger);
        font-size: 11px;
    }
</style>
