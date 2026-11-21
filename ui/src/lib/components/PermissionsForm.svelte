<script lang="ts">
    // The section "Commands with no question" of the settings panel (docs/ipc-m3.md): the rules of
    // `[permissions] allow` of the open floor. A list, a field to add a rule, and a remove button
    // for each rule. A bad rule is refused before the call, with the reason.
    import { onMount } from "svelte";
    import { addRule, removeRule } from "$lib/permissions";
    import { store } from "$lib/store.svelte";

    let draft = $state("");
    let error = $state<string | null>(null);
    let busy = $state(false);

    const rules = $derived(store.permissions);

    onMount(() => {
        void store.loadPermissions();
    });

    async function add(): Promise<void> {
        if (busy || rules === null) return;
        const next = addRule(rules, draft);
        if (next.error) {
            error = next.error;
            return;
        }
        busy = true;
        // The Rust side checks again. A refusal shows here, in the form, with the same reason.
        const refused = await store.savePermissions(next.list);
        busy = false;
        error = refused;
        if (refused === null) draft = "";
    }

    async function remove(rule: string): Promise<void> {
        if (busy || rules === null) return;
        busy = true;
        error = await store.savePermissions(removeRule(rules, rule));
        busy = false;
    }
</script>

<div class="permissions">
    <p class="hint">
        A rule lets a Claude employee run a command with no question. For example, <code>Bash(cargo test:*)</code> lets
        it run <code>cargo test</code> with any arguments. The rules apply to the Claude employees of this floor
        (not to a seat that asks always) and only to a session that starts after you save them. A session that runs
        keeps its old rules. A rule for every command, such as <code>Bash(*)</code>, is refused.
    </p>

    {#if rules === null}
        <p class="hint">Loading...</p>
    {:else}
        <ul class="rules" aria-label="Rules" data-testid="rule-list">
            {#each rules as rule (rule)}
                <li data-rule={rule}>
                    <code>{rule}</code>
                    <button type="button" class="ghost remove" aria-label="Remove {rule}" disabled={busy} onclick={() => void remove(rule)}>
                        remove
                    </button>
                </li>
            {:else}
                <li class="none">No rule. Each shell command asks the operator.</li>
            {/each}
        </ul>
        <form
            class="add"
            onsubmit={(e) => {
                e.preventDefault();
                void add();
            }}
        >
            <input
                type="text"
                name="rule"
                placeholder="Bash(cargo test:*)"
                aria-label="New rule"
                autocomplete="off"
                spellcheck="false"
                maxlength="200"
                bind:value={draft}
                oninput={() => (error = null)}
            />
            <button type="submit" class="primary" disabled={busy || draft.trim() === ""}>add</button>
        </form>
        {#if error}
            <p class="error" role="alert" data-testid="rule-error">{error}</p>
        {/if}
    {/if}
</div>

<style>
    .permissions {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .hint {
        margin: 0;
        color: var(--dim);
        font-size: 11px;
    }

    .rules {
        display: flex;
        flex-direction: column;
        gap: 3px;
        margin: 0;
        padding: 0;
        list-style: none;
    }

    .rules li {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 2px 6px;
        border: 1px solid var(--line);
        border-radius: 4px;
        background: var(--bg-deep);
    }

    .rules li.none {
        border-style: dashed;
        color: var(--dim);
    }

    code {
        font-family: var(--mono);
        overflow-wrap: anywhere;
    }

    .add {
        display: flex;
        gap: 4px;
    }

    .add input {
        flex: 1;
        font-family: var(--mono);
    }

    .error {
        margin: 0;
        color: var(--danger);
        overflow-wrap: anywhere;
    }
</style>
