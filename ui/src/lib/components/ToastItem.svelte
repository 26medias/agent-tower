<script lang="ts">
    // One toast. The text shows at most 3 lines. When the text is cut, a "more" button shows all of it, and "less" cuts it
    // again. The full text is always in the `title`. The toast never takes the focus, except a button that the operator clicks.
    import { store, type Toast } from "$lib/store.svelte";

    let { toast }: { toast: Toast } = $props();

    let open = $state(false);
    // True when the text is longer than 3 lines at the width of the toast. It keeps its value while the text is open.
    let cut = $state(false);
    let textEl: HTMLElement | undefined = $state();

    $effect(() => {
        const el = textEl;
        if (!el) return;
        void toast.text;
        const measure = (): void => {
            if (!open) cut = el.scrollHeight > el.clientHeight + 1;
        };
        measure();
        const observer = new ResizeObserver(measure);
        observer.observe(el);
        return () => observer.disconnect();
    });
</script>

<div class="toast" class:info={toast.kind === "info"} role={toast.kind === "info" ? "status" : "alert"} data-kind={toast.kind ?? "error"}>
    <div class="body">
        <span class="text" class:open title={toast.text} bind:this={textEl}>{toast.text}</span>
        {#if cut || open}
            <button type="button" class="more-btn" aria-expanded={open} data-testid="toast-more" onclick={() => (open = !open)}>{open ? "less" : "more"}</button>
        {/if}
    </div>
    {#if toast.action}
        <button type="button" class="act" onclick={() => { toast.action?.run(); store.dismissToast(toast.id); }}>{toast.action.label}</button>
    {/if}
    <button type="button" class="ghost" aria-label="Dismiss" onclick={() => store.dismissToast(toast.id)}>x</button>
</div>

<style>
    .toast {
        width: min(380px, 100%);
        flex: none;
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 6px 8px 6px 10px;
        background: #2a1a2c;
        border: 1px solid var(--danger);
        border-left-width: 4px;
        border-radius: 6px;
        box-shadow: 0 6px 20px rgba(0, 0, 0, 0.5);
        pointer-events: auto;
    }

    /* A notice, not an error: no red border. */
    .toast.info {
        background: var(--panel);
        border-color: var(--line);
        border-left-color: var(--accent);
    }

    .body {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 2px;
    }

    /* At most 3 lines. `-webkit-line-clamp` works in WebKit (WebKitGTK too) and in Chromium. The `max-height` of 3 lines is the
       fallback for an engine that ignores it: the text is then cut with no dots, and the "more" button still shows. */
    .text {
        display: -webkit-box;
        -webkit-box-orient: vertical;
        -webkit-line-clamp: 3;
        line-clamp: 3;
        line-height: 1.4;
        max-height: 4.2em;
        overflow: hidden;
        overflow-wrap: anywhere;
        user-select: text;
        align-self: stretch;
    }

    .text.open {
        display: block;
        -webkit-line-clamp: unset;
        line-clamp: unset;
        max-height: none;
    }

    .more-btn {
        padding: 0;
        border: 0;
        background: transparent;
        color: var(--accent);
        font-size: 11px;
        text-decoration: underline;
    }

    .act {
        flex: none;
        border-color: var(--danger);
    }

    .toast.info .act {
        border-color: var(--accent);
    }

    @media (prefers-reduced-motion: no-preference) {
        .toast {
            animation: toast-in 0.15s ease-out;
        }
    }

    @keyframes toast-in {
        from {
            opacity: 0;
        }
    }
</style>
