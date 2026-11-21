<script lang="ts">
    import { store } from "$lib/store.svelte";
</script>

<div class="toasts" aria-live="polite">
    {#each store.toasts as toast (toast.id)}
        <div class="toast" class:info={toast.kind === "info"} role={toast.kind === "info" ? "status" : "alert"} data-kind={toast.kind ?? "error"}>
            <span class="text">{toast.text}</span>
            <button type="button" class="ghost" aria-label="Dismiss" onclick={() => store.dismissToast(toast.id)}>
                x
            </button>
        </div>
    {/each}
</div>

<style>
    /* In the page flow, between the title and the cards: a toast never covers a control. */
    .toasts {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 0 8px;
    }

    .toasts:not(:empty) {
        padding-top: 4px;
        padding-bottom: 2px;
    }

    .toast {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 4px 8px;
        background: #2a1a2c;
        border: 1px solid var(--danger);
        border-radius: 6px;
    }

    /* A notice, not an error: no red border. */
    .toast.info {
        background: var(--panel);
        border-color: var(--line);
    }

    .text {
        flex: 1;
        overflow-wrap: anywhere;
        user-select: text;
    }
</style>
