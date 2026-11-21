<script lang="ts">
    // The first start: no floor is open. A centred card asks for the project folder. The same
    // actions are in the left column (the "Open folder" form), but the first thing that an operator
    // sees must be here, in the middle of the office area.
    import { store } from "$lib/store.svelte";

    let path = $state("");
    let name = $state("");
    let busy = $state(false);

    function basename(p: string): string {
        const parts = p.split(/[\\/]/).filter((x) => x !== "");
        return parts.length > 0 ? parts[parts.length - 1] : p;
    }

    $effect(() => {
        // Offer the folder name as the default name of the new floor.
        const pending = store.pendingCreate;
        if (pending) name = basename(pending.path);
    });

    async function open(): Promise<void> {
        if (busy) return;
        busy = true;
        if (await store.openFloor(path)) path = "";
        busy = false;
    }

    async function create(): Promise<void> {
        const pending = store.pendingCreate;
        if (!pending || name.trim() === "" || busy) return;
        busy = true;
        if (await store.createFloor(pending.path, name)) path = "";
        busy = false;
    }
</script>

<div class="empty" data-testid="empty-office">
    <div class="card">
        <h2>Open a project folder</h2>
        <p class="lead">
            A floor is a project folder with a team of agents. Type the full path of the folder. If the folder has no
            floor yet, the app offers to create one (it makes a <code>.swarm</code> folder).
        </p>
        <form
            onsubmit={(e) => {
                e.preventDefault();
                void open();
            }}
        >
            <label for="start-path">Folder path</label>
            <div class="line">
                <!-- svelte-ignore a11y_autofocus -->
                <input id="start-path" type="text" placeholder="/path/to/project" autocomplete="off" spellcheck="false" autofocus bind:value={path} />
                <button type="submit" class="primary" disabled={busy || path.trim() === ""}>Open</button>
            </div>
            {#if path.trim() === ""}
                <p class="reason">Type a path to open it.</p>
            {/if}
        </form>

        {#if store.pendingCreate}
            <form
                class="create"
                onsubmit={(e) => {
                    e.preventDefault();
                    void create();
                }}
            >
                <p>There is no floor in <code>{store.pendingCreate.path}</code>. Create one?</p>
                <label for="start-name">Floor name</label>
                <div class="line">
                    <input id="start-name" type="text" autocomplete="off" bind:value={name} />
                    <button type="submit" class="primary" disabled={busy || name.trim() === ""}>Create the floor</button>
                    <button type="button" onclick={() => store.cancelCreate()}>Cancel</button>
                </div>
            </form>
        {/if}
    </div>
</div>

<style>
    .empty {
        grid-column: 1 / -1;
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 0;
        padding: 12px;
        overflow: auto;
    }

    .card {
        width: min(520px, 100%);
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 18px 20px;
        background: var(--panel);
        border: 1px solid var(--line);
        border-radius: 10px;
    }

    h2 {
        color: var(--accent);
        font-size: 15px;
    }

    .lead {
        margin: 0;
        color: var(--dim);
    }

    form {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    label {
        color: var(--dim);
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    .line {
        display: flex;
        gap: 6px;
    }

    .line input {
        flex: 1;
    }

    .reason {
        margin: 0;
        color: var(--dim);
        font-size: 12px;
    }

    .create {
        padding-top: 8px;
        border-top: 1px solid var(--line);
    }

    .create p {
        margin: 0;
        overflow-wrap: anywhere;
    }

    code {
        color: var(--accent);
    }
</style>
