<script lang="ts">
    import { store } from "$lib/store.svelte";
    import { SETUP_SLUG } from "$lib/types";
    import StatusDot from "./StatusDot.svelte";

    function nameOf(slug: string): string {
        if (slug === SETUP_SLUG) return "Setup";
        return store.floorView?.employees.find((e) => e.slug === slug)?.name ?? slug;
    }
</script>

<div class="tabs" role="tablist" aria-label="Sessions">
    {#each store.tabs as tab (tab.sessionId)}
        <div class="tab" class:active={tab.sessionId === store.activeId} class:stopped={tab.status === "stopped"} data-session={tab.sessionId}>
            <button
                type="button"
                class="main"
                role="tab"
                aria-selected={tab.sessionId === store.activeId}
                onclick={() => store.selectTab(tab.sessionId)}
            >
                <StatusDot status={tab.status} />
                <span class="label">{nameOf(tab.slug)}</span>
            </button>
            {#if tab.messageWaiting}
                <button
                    type="button"
                    class="mark waiting"
                    title="A message waits. Click to clear your draft flag so it can be delivered."
                    aria-label="Message waiting. Clear the draft flag."
                    onclick={() => store.clearDraft(tab.sessionId)}
                >
                    msg
                </button>
            {:else if tab.draft}
                <button
                    type="button"
                    class="mark draft"
                    title="The app thinks that you have unsent text here. It holds messages while this flag is set. Click to clear the flag, for example when you deleted the text."
                    aria-label="Draft flag set. Clear the draft flag."
                    onclick={() => store.clearDraft(tab.sessionId)}
                >
                    draft
                </button>
            {/if}
            {#if tab.status === "stopped"}
                <button
                    type="button"
                    class="close ghost"
                    title="Close this tab"
                    aria-label="Close tab"
                    onclick={() => store.closeTab(tab.sessionId)}
                >
                    x
                </button>
            {/if}
        </div>
    {:else}
        <span class="none">No live session</span>
    {/each}
</div>

<style>
    .tabs {
        display: flex;
        align-items: flex-end;
        gap: 2px;
        padding: 4px 8px 0;
        min-height: 30px;
        overflow-x: auto;
        background: var(--bg);
        border-bottom: 1px solid var(--line);
    }

    .tab {
        display: flex;
        align-items: center;
        gap: 2px;
        padding-right: 4px;
        background: var(--panel);
        border: 1px solid var(--line);
        border-bottom: 0;
        border-radius: 6px 6px 0 0;
        margin-bottom: -2px;
    }

    .tab.active {
        background: var(--bg-deep);
        border-color: var(--accent);
    }

    .tab.stopped .label {
        color: var(--dim);
    }

    .main {
        display: flex;
        align-items: center;
        gap: 6px;
        background: transparent;
        border: 0;
        padding: 3px 6px;
        text-transform: none;
    }

    .mark {
        padding: 0 4px;
        font-size: 11px;
        border-radius: 6px;
        text-transform: uppercase;
    }

    .waiting {
        background: var(--st-question);
        border-color: var(--st-question);
        color: var(--accent-ink);
        font-weight: 700;
    }

    .draft {
        background: transparent;
        border: 1px dashed var(--dim);
        color: var(--dim);
    }

    .draft:hover {
        color: var(--text);
        border-color: var(--accent);
    }

    .close {
        padding: 0 4px;
    }

    .none {
        color: var(--dim);
        padding: 4px 0 6px;
    }
</style>
