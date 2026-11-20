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

<details class="programs">
    <summary>Programs</summary>
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
                </form>
            </div>
        {/each}
        <div class="hook"><span class="name">hook</span> <span class="value" title={store.programs.hook}>{store.programs.hook}</span></div>
    {:else}
        <p class="dim">Loading...</p>
    {/if}
</details>

<style>
    .programs {
        border-top: 1px solid var(--line);
        padding: 6px 0 0;
        font-size: 12px;
    }

    summary {
        cursor: pointer;
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--dim);
    }

    .row {
        margin-top: 6px;
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

    .dim {
        color: var(--dim);
    }
</style>
