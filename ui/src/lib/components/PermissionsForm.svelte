<script lang="ts">
    // The section "Commands with no question" of the settings panel (docs/ipc-m3.md): the rules of
    // the open floor. The app stores them in its own config, outside the project folder. A list, a
    // field to add a rule, and a remove button for each rule. A bad rule is refused before the call,
    // with the reason. A rule that runs the code of the project is saved with a warning from the
    // server: the warning shows on that rule. An old `[permissions]` table in the project is only
    // proposed (an agent can have written it): the operator reads it and clicks "Use these rules".
    import { onMount } from "svelte";
    import { EXAMPLE_RULES, addRule, mergeRules, removeRule } from "$lib/permissions";
    import { store } from "$lib/store.svelte";

    let draft = $state("");
    let error = $state<string | null>(null);
    let busy = $state(false);
    let more = $state(false);

    const rules = $derived(store.permissions);
    // The examples that are not in the list yet. A click adds one.
    const examples = $derived(EXAMPLE_RULES.filter((r) => !(rules ?? []).includes(r)));
    const proposed = $derived(store.proposedRules);

    /** The warning of the server for a rule, without the rule in front of it. `null` when it has none. */
    function warningOf(rule: string): string | null {
        const w = store.permissionWarnings.find((x) => x.startsWith(`${rule}: `));
        return w ? w.slice(rule.length + 2) : null;
    }

    onMount(() => {
        // A gone floor cannot be read: the call would only give a `FLOOR_GONE:` toast.
        if (!store.gone) void store.loadPermissions();
    });

    async function add(raw: string = draft): Promise<void> {
        if (busy || rules === null || store.gone) return;
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

    /** Use the rules of the old table: the list that the operator sees now, plus the proposed rules. The server checks them. */
    async function useProposed(): Promise<void> {
        if (busy || rules === null || store.gone || proposed.length === 0) return;
        busy = true;
        error = await store.savePermissions(mergeRules(rules, proposed));
        busy = false;
    }

    async function remove(rule: string): Promise<void> {
        if (busy || rules === null || store.gone) return;
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
            session that starts after you save them. A session that runs keeps its old rules. The app refuses a rule that is too wide
            or that runs other programs: a rule for every command, a shell, an interpreter, <code>find</code>, <code>sed</code>,
            <code>rm</code>, <code>curl</code>, <code>git</code> with a subcommand that changes things, and more. The server decides,
            and its reason shows here.
        </p>
    {/if}
    <p class="hint" data-testid="rules-storage">
        The app stores these rules in its own config folder, outside the project folder. So an agent of the floor cannot change them
        through the project.
    </p>
    <p class="warning" data-testid="rules-warning" role="note">
        Each rule lets every agent of this floor run that command with no question. Build tools such as cargo, npm and make run the
        code of the project.
    </p>
    <p class="hint" data-testid="rules-hook-note">
        A rule must match the command as the CLI sees it. If a hook on this machine rewrites commands (for example
        <code>cargo test</code> becomes <code>rtk cargo test</code>), write the rule for the rewritten command.
    </p>

    {#if rules === null && store.gone}
        <p class="hint" data-testid="rules-gone">The folder of this floor is gone. The app cannot read or change the rules.</p>
    {:else if rules === null}
        <p class="hint">Loading...</p>
    {:else}
        {#if proposed.length > 0}
            <div class="proposal" data-testid="rules-proposal">
                <p>
                    This project has a list of rules from an older version. An agent can have written it. Read each rule before you use
                    it.
                </p>
                <ul aria-label="Rules that the project proposes">
                    {#each proposed as rule (rule)}
                        <li data-proposed={rule}><code>{rule}</code></li>
                    {/each}
                </ul>
                <button type="button" class="primary" data-testid="rules-use" disabled={busy || store.gone} title={store.goneTitle()} onclick={() => void useProposed()}>
                    Use these rules
                </button>
            </div>
        {/if}
        <ul class="rules" aria-label="Rules" data-testid="rule-list">
            {#each rules as rule (rule)}
                {@const warning = warningOf(rule)}
                <li data-rule={rule} class:warned={warning !== null}>
                    <code>{rule}</code>
                    {#if warning !== null}
                        <span class="mark" role="img" aria-label="Warning" title={warning}>!</span>
                    {/if}
                    <button type="button" class="ghost remove" aria-label="Remove {rule}" disabled={busy || store.gone} title={store.goneTitle()} onclick={() => void remove(rule)}>
                        remove
                    </button>
                    {#if warning !== null}
                        <span class="warn-text" data-testid="rule-warning">{warning}</span>
                    {/if}
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
                disabled={store.gone}
                title={store.goneTitle()}
                oninput={() => (error = null)}
            />
            <button type="submit" class="primary" disabled={busy || store.gone || draft.trim() === ""} title={store.goneTitle()}>add</button>
        </form>
        {#if error}
            <p class="error" role="alert" data-testid="rule-error">{error}</p>
        {/if}
        {#if examples.length > 0}
            <div class="examples" aria-label="Example rules">
                <span class="hint">Click to add:</span>
                {#each examples as rule (rule)}
                    <button type="button" class="chip" data-example={rule} disabled={busy || store.gone} title={store.goneTitle()} onclick={() => void add(rule)}>{rule}</button>
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

    .rules li {
        flex-wrap: wrap;
    }

    .rules li code {
        flex: 1;
        min-width: 0;
    }

    /* A rule with a warning: an amber mark, and the text of the server under the rule. */
    .rules li.warned {
        border-color: #b3822e;
    }

    .mark {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        width: 16px;
        height: 16px;
        border-radius: 50%;
        background: var(--st-question);
        color: #1a1400;
        font: 700 11px var(--display);
    }

    .warn-text {
        flex-basis: 100%;
        color: var(--st-question);
        font-size: 11px;
    }

    .warning {
        margin: 0;
        padding: 4px 8px;
        border: 1px solid #b3822e;
        border-left-width: 4px;
        border-radius: 4px;
        color: var(--st-question);
        font-size: 11px;
    }

    .proposal {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 4px;
        padding: 6px 8px;
        border: 1px solid var(--accent);
        border-radius: 6px;
        background: var(--bg-deep);
    }

    .proposal p {
        margin: 0;
    }

    .proposal ul {
        display: flex;
        flex-direction: column;
        gap: 2px;
        margin: 0;
        padding: 0;
        list-style: none;
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
