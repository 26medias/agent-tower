<script lang="ts">
    // The section "Commands with no question" of the settings panel (docs/ipc-m3.md): the rules of
    // `[permissions] allow` of the open floor. A list, a field to add a rule, and a remove button
    // for each rule. A bad rule is refused before the call, with the reason.
    import { onMount } from "svelte";
    import { EXAMPLE_RULES, addRule, removeRule } from "$lib/permissions";
    import { store } from "$lib/store.svelte";

    let draft = $state("");
    let error = $state<string | null>(null);
    let busy = $state(false);
    let more = $state(false);

    const rules = $derived(store.permissions);
    // The examples that are not in the list yet. A click adds one.
    const examples = $derived(EXAMPLE_RULES.filter((r) => !(rules ?? []).includes(r)));

    onMount(() => {
        void store.loadPermissions();
    });

    async function add(raw: string = draft): Promise<void> {
        if (busy || rules === null) return;
        const next = addRule(rules, raw);
        if (next.error) {
            error = next.error;
            return;
        }
        busy = true;
        // The Rust side checks again and is the judge. A refusal shows here, in the form, with the
        // text of the server.
        const refused = await store.savePermissions(next.list);
        busy = false;
        error = refused;
        if (refused === null && raw === draft) draft = "";
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
        A rule lets a Claude employee run a command with no question. For example, <code>Bash(cargo test:*)</code> lets it run
        <code>cargo test</code> with any arguments.
        <button type="button" class="link" aria-expanded={more} data-testid="rules-more" onclick={() => (more = !more)}>
            {more ? "Less" : "More..."}
        </button>
    </p>
    {#if more}
        <p class="hint" data-testid="rules-more-text">
            The rules apply to the Claude employees of this floor (not to an employee that asks before each action) and only to a
            session that starts after you save them. A session that runs keeps its old rules. The app refuses a rule that is too wide:
            a rule for every command, a shell, an interpreter, <code>rm</code>, <code>curl</code>, <code>git push</code> and more.
            The server decides, and its reason shows here.
        </p>
    {/if}
    <p class="hint" data-testid="rules-hook-note">
        A rule must match the command as the CLI sees it. If a hook on this machine rewrites commands (for example
        <code>cargo test</code> becomes <code>rtk cargo test</code>), write the rule for the rewritten command.
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
        {#if examples.length > 0}
            <div class="examples" aria-label="Example rules">
                <span class="hint">Click to add:</span>
                {#each examples as rule (rule)}
                    <button type="button" class="chip" data-example={rule} disabled={busy} onclick={() => void add(rule)}>{rule}</button>
                {/each}
            </div>
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

    .link {
        padding: 0;
        border: 0;
        background: transparent;
        color: var(--accent);
        font-size: 11px;
        text-decoration: underline;
    }

    .examples {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 4px 6px;
    }

    .chip {
        padding: 1px 8px;
        border-radius: 10px;
        font: 11px var(--mono);
    }
</style>
