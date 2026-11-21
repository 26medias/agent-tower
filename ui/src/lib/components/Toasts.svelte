<script lang="ts">
    // The toasts are an overlay at the top right, over the task board. They never change the layout
    // and they never take the focus. The newest toast is on top. At most 4 show. An information
    // toast closes by itself (see TOAST_INFO_MS in the store). An error toast stays until the
    // operator closes it.
    import { TOASTS_VISIBLE, store } from "$lib/store.svelte";

    const visible = $derived(store.toasts.slice(-TOASTS_VISIBLE).reverse());
    const hidden = $derived(Math.max(0, store.toasts.length - TOASTS_VISIBLE));
</script>

<!-- `data-keep-live`: a modal does not make the toasts inert. An error can need a click while a modal is open. -->
<div class="toasts" data-keep-live aria-live="polite" data-testid="toasts">
    {#each visible as toast (toast.id)}
        <div class="toast" class:info={toast.kind === "info"} role={toast.kind === "info" ? "status" : "alert"} data-kind={toast.kind ?? "error"}>
            <span class="text">{toast.text}</span>
            {#if toast.action}
                <button type="button" class="act" onclick={() => { toast.action?.run(); store.dismissToast(toast.id); }}>{toast.action.label}</button>
            {/if}
            <button type="button" class="ghost" aria-label="Dismiss" onclick={() => store.dismissToast(toast.id)}>x</button>
        </div>
    {/each}
    {#if hidden > 0}
        <div class="more" data-testid="toasts-more">{hidden} older {hidden === 1 ? "message" : "messages"} wait. Close a message to see the next one.</div>
    {/if}
</div>

<style>
    .toasts {
        position: fixed;
        z-index: 60;
        /* Under the header of the task board, so that "New" and "collapse" stay in reach. */
        top: 48px;
        right: 10px;
        display: flex;
        flex-direction: column;
        gap: 6px;
        width: min(380px, calc(100vw - 20px));
        pointer-events: none;
    }

    .toast,
    .more {
        pointer-events: auto;
    }

    .toast {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 6px 8px 6px 10px;
        background: #2a1a2c;
        border: 1px solid var(--danger);
        border-left-width: 4px;
        border-radius: 6px;
        box-shadow: 0 6px 20px rgba(0, 0, 0, 0.5);
    }

    /* A notice, not an error: no red border. */
    .toast.info {
        background: var(--panel);
        border-color: var(--line);
        border-left-color: var(--accent);
    }

    .text {
        flex: 1;
        overflow-wrap: anywhere;
        user-select: text;
    }

    .act {
        flex: none;
        border-color: var(--danger);
    }

    .more {
        align-self: flex-end;
        padding: 2px 8px;
        border-radius: 10px;
        background: var(--panel);
        border: 1px solid var(--line);
        color: var(--dim);
        font-size: 11px;
    }

    @media (prefers-reduced-motion: no-preference) {
        .toast {
            animation: toast-in 0.15s ease-out;
        }
    }

    @keyframes toast-in {
        from {
            opacity: 0;
            transform: translateY(-6px);
        }
    }
</style>
