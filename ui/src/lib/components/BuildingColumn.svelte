<script lang="ts">
    // The left column: the building at the bottom, and a small toolbar under it with
    // "open folder", "create", the list of floors (with "forget") and the programs panel.
    import { tick } from "svelte";
    import { store } from "$lib/store.svelte";
    import Building from "./Building.svelte";
    import ConfirmButton from "./ConfirmButton.svelte";
    import ProgramsPanel from "./ProgramsPanel.svelte";

    let pathInput = $state("");
    let createName = $state("");
    let scroller: HTMLDivElement;

    function basename(path: string): string {
        const parts = path.split(/[\\/]/).filter((p) => p !== "");
        return parts.length > 0 ? parts[parts.length - 1] : path;
    }

    $effect(() => {
        // Offer the folder name as the default name of a new floor.
        const pending = store.pendingCreate;
        if (pending) createName = basename(pending.path);
    });

    $effect(() => {
        // A tall building scrolls. Start at the bottom, where the first floor is.
        void store.floors.length;
        void tick().then(() => {
            if (scroller) scroller.scrollTop = scroller.scrollHeight;
        });
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

<aside class="column">
    <header>
        <h1 class="logo">Agent Tower</h1>
        {#if store.backend === "mock"}
            <span class="mock" title="No Tauri found: the page uses the in-memory mock backend">mock</span>
        {/if}
    </header>

    <div class="scroll" bind:this={scroller}>
        <div class="tower">
            <Building />
        </div>
    </div>

    <div class="toolbar">
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

        <details class="floors">
            <summary>Floors ({store.floors.length})</summary>
            <ul class="list" aria-label="Floors">
                {#each store.floors as floor (floor.path)}
                    <li class:open={floor.path === store.openPath} class:missing={floor.missing} data-path={floor.path}>
                        <span class="floor-text" title={floor.path}>
                            <span class="floor-name">{floor.name ?? basename(floor.path)}</span>
                            <span class="floor-path">{floor.missing ? "missing" : floor.path}</span>
                        </span>
                        <ConfirmButton
                            class="ghost"
                            label="forget"
                            confirmLabel="sure?"
                            title="Forget this floor (no file is deleted)"
                            onconfirm={() => store.forgetFloor(floor.path)}
                        />
                    </li>
                {:else}
                    <li class="none">No floor yet.</li>
                {/each}
            </ul>
        </details>

        <ProgramsPanel />
    </div>
</aside>

<style>
    .column {
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
        background: var(--panel);
        border-right: 1px solid var(--line);
    }

    header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 10px 12px 4px;
    }

    .logo {
        color: var(--accent);
        font-size: 14px;
    }

    .mock {
        padding: 0 6px;
        border: 1px solid var(--st-working);
        border-radius: 4px;
        color: var(--st-working);
        font-size: 11px;
        text-transform: uppercase;
    }

    /* The tower sits at the bottom of this area. With more floors than fit, the area scrolls. */
    .scroll {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        padding: 0 8px;
    }

    .tower {
        margin-top: auto;
    }

    .toolbar {
        display: flex;
        flex-direction: column;
        gap: 8px;
        max-height: 55%;
        overflow-y: auto;
        padding: 8px 12px 10px;
        border-top: 1px solid var(--line);
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
        border: 1px solid var(--accent);
        border-radius: 6px;
        background: var(--bg-deep);
    }

    .create p {
        margin: 0;
        overflow-wrap: anywhere;
    }

    code {
        color: var(--accent);
    }

    .floors {
        font-size: 12px;
    }

    summary {
        cursor: pointer;
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--dim);
    }

    .list {
        list-style: none;
        margin: 4px 0 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .list li {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 1px 4px;
        border: 1px solid transparent;
        border-radius: 6px;
    }

    .list li.open {
        border-color: var(--accent);
    }

    .list li.missing .floor-name {
        color: var(--dim);
    }

    .floor-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
    }

    .floor-name {
        font-weight: 700;
    }

    .floor-path {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--dim);
        font-size: 11px;
    }

    .none {
        color: var(--dim);
    }
</style>
