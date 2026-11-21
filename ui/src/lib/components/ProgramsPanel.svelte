<script lang="ts">
    import { store } from "$lib/store.svelte";
    import type { ProgramName } from "$lib/types";

    const names: ProgramName[] = ["claude", "codex", "node"];
    let drafts = $state<Record<ProgramName, string>>({ claude: "", codex: "", node: "" });

    async function set(name: ProgramName): Promise<void> {
        if (drafts[name].trim() === "") return;
        if (await store.setProgram(name, drafts[name])) drafts[name] = "";
    }
</script>

<div class="programs">
    {#if store.programs}
        {#each names as name (name)}
            <div class="row" data-program={name}>
                <div class="head">
                    <span class="name">{name}</span>
                    <span class="value" class:missing={!store.programs[name]} title={store.programs[name] ?? ""}>
                        {store.programs[name] ?? "not found"}
                    </span>
                </div>
                <form
                    onsubmit={(e) => {
                        e.preventDefault();
                        void set(name);
                    }}
                >
                    <input
                        type="text"
                        placeholder="/path/to/{name}"
                        aria-label="Path of {name}"
                        bind:value={drafts[name]}
                    />
                    <button type="submit" disabled={drafts[name].trim() === ""}>set</button>
                    <button
                        type="button"
                        class="test"
                        title="Run {name} --version with the environment of a session"
                        disabled={store.programTesting[name]}
                        onclick={() => void store.testProgram(name)}
                    >
                        {store.programTesting[name] ? "testing..." : "test"}
                    </button>
                    {#if store.programs.overrides[name]}
                        <button
                            type="button"
                            class="reset"
                            title="Remove the path that you set. The app searches for {name} again."
                            onclick={() => void store.resetProgram(name)}
                        >
                            reset
                        </button>
                    {/if}
                </form>
                {#if store.programs.overrides[name]}
                    <p class="note" data-testid="override-{name}" title={store.programs.overrides[name]}>
                        your path: {store.programs.overrides[name]}
                    </p>
                {/if}
                {#if store.programTests[name]}
                    {@const result = store.programTests[name]}
                    <p
                        class="result"
                        class:ok={result.ok}
                        class:bad={!result.ok}
                        data-testid="test-{name}"
                        data-ok={result.ok}
                        title={result.path ?? ""}
                    >
                        <span class="mark" aria-hidden="true">{result.ok ? "\u2713" : "\u2717"}</span>
                        <span class="sr">{result.ok ? "works:" : "fails:"}</span>
                        {result.output}
                    </p>
                {/if}
            </div>
        {/each}
        <div class="hook"><span class="name">hook</span> <span class="value" title={store.programs.hook}>{store.programs.hook}</span></div>
    {:else}
        <p class="dim">Loading...</p>
    {/if}
</div>

<style>
    .programs {
        font-size: 12px;
    }

    .row {
        margin-top: 6px;
    }

    .row:first-child {
        margin-top: 0;
    }

    .head,
    .hook {
        display: flex;
        gap: 6px;
        align-items: baseline;
    }

    .hook {
        margin-top: 6px;
    }

    .name {
        color: var(--accent);
        min-width: 44px;
    }

    .value {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--text);
    }

    .value.missing {
        color: var(--danger);
    }

    form {
        display: flex;
        gap: 4px;
        margin-top: 2px;
    }

    input {
        flex: 1;
        font-size: 12px;
    }

    .note {
        margin-top: 2px;
        color: var(--dim);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .result {
        margin-top: 2px;
        display: flex;
        gap: 6px;
        align-items: baseline;
        overflow-wrap: anywhere;
    }

    .result.ok {
        color: var(--ok);
    }

    .result.bad {
        color: var(--danger);
    }

    .mark {
        font-weight: 700;
    }

    .sr {
        position: absolute;
        width: 1px;
        height: 1px;
        overflow: hidden;
        clip-path: inset(50%);
    }

    .dim {
        color: var(--dim);
    }
</style>
