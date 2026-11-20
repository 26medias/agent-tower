<script lang="ts">
    import { store } from "$lib/store.svelte";
</script>

<div class="toasts" aria-live="polite">
    {#each store.toasts as toast (toast.id)}
        <div class="toast" role="alert">
            <span class="text">{toast.text}</span>
            <button type="button" class="ghost" aria-label="Dismiss" onclick={() => store.dismissToast(toast.id)}>
                x
            </button>
        </div>
    {/each}
</div>

<style>
    .toasts {
        position: fixed;
        right: 12px;
        bottom: 12px;
        z-index: 50;
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-width: min(480px, 80vw);
    }

    .toast {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 6px 8px;
        background: #2a1620;
        border: 2px solid var(--danger);
        border-radius: 2px;
        box-shadow: 4px 4px 0 rgba(0, 0, 0, 0.5);
    }

    .text {
        flex: 1;
        overflow-wrap: anywhere;
        user-select: text;
    }
</style>
