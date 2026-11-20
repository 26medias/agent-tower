<script lang="ts">
    import { store } from "$lib/store.svelte";
    import TerminalPane from "./TerminalPane.svelte";
</script>

<div class="area">
    <div class="stack">
        {#each store.allTabSessions as s (s.sessionId)}
            <TerminalPane
                sessionId={s.sessionId}
                live={s.status !== "stopped"}
                active={s.sessionId === store.activeId && s.path === store.openPath}
            />
        {/each}
        {#if !store.activeId}
            <div class="empty">
                <p>No session open.</p>
                <p class="dim">Click a character, then press "start".</p>
            </div>
        {/if}
    </div>
</div>

<style>
    .area {
        min-height: 0;
        padding: 6px;
        background: var(--bg-deep);
    }

    .stack {
        position: relative;
        height: 100%;
    }

    .empty {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        color: var(--dim);
    }

    .empty p {
        margin: 2px;
    }

    .dim {
        font-size: 12px;
    }
</style>
