<script lang="ts">
    import { store } from "$lib/store.svelte";
    import ConfirmButton from "./ConfirmButton.svelte";
    import ProgramsPanel from "./ProgramsPanel.svelte";

    let pathInput = $state("");
    let createName = $state("");

    function basename(path: string): string {
        const parts = path.split(/[\\/]/).filter((p) => p !== "");
        return parts.length > 0 ? parts[parts.length - 1] : path;
    }

    $effect(() => {
        // Offer the folder name as the default name of a new floor.
        const pending = store.pendingCreate;
        if (pending) createName = basename(pending.path);
    });

    async function open(): Promise<void> {
        if (await store.openFloor(pathInput)) pathInput = "";
    }

    async function create(): Promise<void> {
        const pending = store.pendingCreate;
        if (!pending || createName.trim() === "") return;
        if (await store.createFloor(pending.path, createName)) pathInput = "";
    }
</script>

<aside class="floors">
    <header>
        <h1 class="logo">Agent<br />Tower</h1>
        {#if store.backend === "mock"}
            <span class="mock" title="No Tauri found: the page uses the in-memory mock backend">mock</span>
        {/if}
    </header>

    <h2>Floors</h2>
    <ul class="list" aria-label="Floors">
        {#each store.floors as floor (floor.path)}
            {@const questions = store.questionsIn(floor.path)}
            <li class:open={floor.path === store.openPath} class:missing={floor.missing} data-path={floor.path}>
                <button
                    type="button"
                    class="floor"
                    disabled={floor.missing}
                    title={floor.missing ? "This folder is missing" : floor.path}
                    onclick={() => store.openFloor(floor.path)}
                >
                    <span class="floor-name">{floor.name ?? basename(floor.path)}</span>
                    <span class="floor-path">{floor.missing ? "missing" : floor.path}</span>
                </button>
                {#if questions > 0}
                    <span class="badge" title="{questions} waiting for you">{questions}</span>
                {/if}
                <ConfirmButton
                    class="ghost"
                    label="x"
                    confirmLabel="forget?"
                    title="Forget this floor (no file is deleted)"
                    onconfirm={() => store.forgetFloor(floor.path)}
                />
            </li>
        {:else}
            <li class="empty">No floor yet. Open a folder.</li>
        {/each}
    </ul>

    <form
        class="open"
        onsubmit={(e) => {
            e.preventDefault();
            void open();
        }}
    >
        <label for="floor-path">Open folder</label>
        <div class="line">
            <input
                id="floor-path"
                type="text"
                placeholder="/path/to/project"
                autocomplete="off"
                spellcheck="false"
                bind:value={pathInput}
            />
            <button type="submit" class="primary">open</button>
        </div>
    </form>

    {#if store.pendingCreate}
        <form
            class="create"
            onsubmit={(e) => {
                e.preventDefault();
                void create();
            }}
        >
            <p>
                No floor in <code>{store.pendingCreate.path}</code>. Create one?
            </p>
            <label for="floor-name">Floor name</label>
            <input id="floor-name" type="text" autocomplete="off" bind:value={createName} />
            <div class="line">
                <button type="submit" class="primary" disabled={createName.trim() === ""}>create</button>
                <button type="button" onclick={() => store.cancelCreate()}>cancel</button>
            </div>
        </form>
    {/if}

    <div class="grow"></div>
    <ProgramsPanel />
</aside>

<style>
    .floors {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-height: 0;
        padding: 10px;
        background: var(--panel);
        border-right: 2px solid var(--line);
        overflow-y: auto;
    }

    header {
        display: flex;
        justify-content: space-between;
        align-items: flex-start;
    }

    .logo {
        color: var(--accent);
        font-size: 16px;
        line-height: 1.1;
        text-shadow: 2px 2px 0 #7a5a00;
    }

    .mock {
        padding: 0 5px;
        border: 2px solid var(--st-working);
        color: var(--st-working);
        font-size: 11px;
        text-transform: uppercase;
    }

    h2 {
        font-size: 11px;
        color: var(--dim);
    }

    .list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    li {
        display: flex;
        align-items: center;
        gap: 2px;
        border: 2px solid transparent;
        border-radius: 2px;
    }

    li.open {
        background: var(--panel-2);
        border-color: var(--accent);
    }

    li.missing .floor {
        color: var(--dim);
    }

    .floor {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        background: transparent;
        border: 0;
        padding: 3px 6px;
        text-align: left;
        text-transform: none;
    }

    .floor-name {
        font-weight: 700;
    }

    .floor-path {
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--dim);
        font-size: 11px;
    }

    .badge {
        min-width: 18px;
        padding: 0 4px;
        text-align: center;
        background: var(--st-question);
        color: var(--accent-ink);
        font-weight: 700;
        border-radius: 2px;
    }

    .empty {
        color: var(--dim);
        padding: 4px 6px;
    }

    form {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    label {
        font-size: 11px;
        color: var(--dim);
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    .line {
        display: flex;
        gap: 4px;
    }

    .line input {
        flex: 1;
    }

    .create {
        padding: 6px;
        border: 2px solid var(--accent);
        border-radius: 2px;
        background: var(--bg-deep);
    }

    .create p {
        margin: 0;
        overflow-wrap: anywhere;
    }

    code {
        color: var(--accent);
    }

    .grow {
        flex: 1;
    }
</style>
