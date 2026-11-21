<script lang="ts">
    // The left column: the building at the bottom, and a small toolbar under it with
    // "open folder", "create" and the list of floors (with "forget"). The "Settings" button in the
    // header opens the settings panel (it has the programs paths).
    import { tick } from "svelte";
    import { store } from "$lib/store.svelte";
    import { buildingScale } from "$lib/sizes";
    import Building from "./Building.svelte";
    import BrowseButton from "./BrowseButton.svelte";
    import ConfirmButton from "./ConfirmButton.svelte";

    let pathInput = $state("");
    let createName = $state("");
    let scroller: HTMLDivElement;
    // Size of the area of the building (without the padding). The building fits it.
    let areaW = $state(0);
    let areaH = $state(0);
    const scale = $derived(buildingScale(areaW - 16, areaH, store.floors.length));
    // On a tall window the column has room: the list of floors is open at the start.
    let floorsOpen = $state(typeof window !== "undefined" && window.innerHeight >= 1000);

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

    // The error of a bad path shows under the field that the path was typed in (the empty office has its own field).
    const showError = $derived(store.openError?.source === "column");

    async function open(): Promise<void> {
        if (await store.openFloor(pathInput, "column")) pathInput = "";
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
        <div class="tools">
            {#if store.backend === "mock"}
                <span class="mock" title="No Tauri found: the page uses the in-memory mock backend">mock</span>
            {/if}
            <button
                type="button"
                class="ghost settings"
                title="Settings"
                aria-label="Settings"
                onclick={() => (store.settingsOpen = true)}
            >
                <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                    <path
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linejoin="round"
                        d="M10.3 2.5h3.4l.5 2.6 1.9.8 2.2-1.5 2.4 2.4-1.5 2.2.8 1.9 2.6.5v3.4l-2.6.5-.8 1.9 1.5 2.2-2.4 2.4-2.2-1.5-1.9.8-.5 2.6h-3.4l-.5-2.6-1.9-.8-2.2 1.5-2.4-2.4 1.5-2.2-.8-1.9-2.6-.5v-3.4l2.6-.5.8-1.9-1.5-2.2 2.4-2.4 2.2 1.5 1.9-.8z"
                    />
                    <circle cx="12" cy="12" r="3.2" fill="none" stroke="currentColor" stroke-width="2" />
                </svg>
                <span>Settings</span>
            </button>
        </div>
    </header>

    <div class="scroll" bind:this={scroller} bind:clientWidth={areaW} bind:clientHeight={areaH}>
        <div class="tower">
            <Building {scale} />
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
                    aria-invalid={showError ? "true" : undefined}
                    aria-describedby={showError ? "floor-path-error" : undefined}
                    bind:value={pathInput}
                    oninput={() => store.clearOpenError()}
                />
                <BrowseButton
                    onpick={(picked) => {
                        pathInput = picked;
                        store.clearOpenError();
                    }}
                />
                <button type="submit" class="primary">open</button>
            </div>
            {#if showError && store.openError}
                <!-- The typed path stays in the field, so that the operator can correct it. No "create" is offered. -->
                <p class="error" id="floor-path-error" role="alert" data-testid="open-error">{store.openError.text}</p>
            {/if}
        </form>

        {#if store.pendingCreate && store.floorView}
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

        <details class="floors" bind:open={floorsOpen}>
            <summary>Floors ({store.floors.length})</summary>
            <ul class="list" aria-label="Floors">
                {#each store.floors as floor (floor.path)}
                    {@const live = store.liveIn(floor.path)}
                    {@const asks = store.questionsIn(floor.path)}
                    <li class:open={floor.path === store.openPath} class:missing={floor.missing} data-path={floor.path}>
                        <!-- A click on the row opens the floor. "forget" has its own click. -->
                        <button
                            type="button"
                            class="floor-text ghost"
                            title={floor.missing ? `${floor.path} (the folder is missing)` : `Open ${floor.name ?? basename(floor.path)}\n${floor.path}`}
                            disabled={floor.missing}
                            aria-current={floor.path === store.openPath}
                            data-testid="floor-open"
                            onclick={() => void store.openFloor(floor.path)}
                        >
                            <span class="floor-name">{floor.name ?? basename(floor.path)}</span>
                            <span class="floor-path">{floor.missing ? "missing" : floor.path}</span>
                            {#if !floor.missing && (live > 0 || asks > 0)}
                                <span class="floor-live" data-testid="floor-live">
                                    {live} live{#if asks > 0}, <span class="floor-asks">{asks} {asks === 1 ? "asks" : "ask"}</span>{/if}
                                </span>
                            {/if}
                        </button>
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
        flex-wrap: wrap;
        justify-content: space-between;
        align-items: center;
        gap: 4px 8px;
        padding: 10px 12px 4px;
    }

    .logo {
        color: var(--accent);
        font-size: 14px;
    }

    .tools {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    button.settings {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 1px 6px;
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
        scrollbar-gutter: stable;
        display: flex;
        flex-direction: column;
        padding: 0 8px;
    }

    .tower {
        /* At the bottom of the area, centred in the width. */
        margin: auto auto 0;
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

    .error {
        margin: 0;
        color: var(--danger);
        font-size: 12px;
        overflow-wrap: anywhere;
        user-select: text;
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
        align-items: flex-start;
        padding: 1px 4px;
        text-align: left;
        text-transform: none;
        color: var(--text);
    }

    .floor-text:disabled {
        opacity: 1;
        cursor: default;
    }

    .floor-live {
        color: var(--dim);
        font-size: 11px;
    }

    .floor-asks {
        color: var(--st-question);
        font-weight: 700;
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

    .none {
        color: var(--dim);
    }
</style>
