<script lang="ts">
    // The toasts are an overlay at the bottom right of the CENTRE column. They sit over the terminal area, above its
    // last two lines (where the input of the CLI is). They cover no control: not the top strip, not the office, not
    // the tabs, and not the task board (the board is the column at the right, and the toasts stop at its edge).
    // While a modal is open, everything behind it is inert, so the stack moves to the bottom right corner of the window:
    // it covers no control of the page then, and it is the farthest from the form of the modal (which is in the middle).
    // They never change the layout and they never take the focus. The newest toast is the lowest, nearest to the
    // corner. At most 4 show. An information toast closes by itself (see TOAST_INFO_MS in the store). An error toast
    // stays until the operator closes it, except the toast of a blocked merge while the detail of its task is open
    // (see TOAST_TASK_MS in the store).
    import { onMount } from "svelte";
    import { layout } from "$lib/layout.svelte";
    import { modalOpen, onModalChange } from "$lib/modal";
    import { TOASTS_VISIBLE, store } from "$lib/store.svelte";

    /** The padding of the terminal area, the height of one terminal line (a little more than the real 15 px), and the gap. */
    const AREA_PAD_PX = 6;
    const LINE_PX = 16;
    const GAP_PX = 8;
    /** The distance from the window bottom while a modal is open. */
    const MODAL_BOTTOM_PX = 12;

    const visible = $derived(store.toasts.slice(-TOASTS_VISIBLE));
    const hidden = $derived(Math.max(0, store.toasts.length - TOASTS_VISIBLE));

    // The stack stands on the terminal area, above its last two lines. The area moves when the debug bar shows, so
    // the page measures it again when the area changes or the window resizes.
    let bottom = $state(AREA_PAD_PX + 2 * LINE_PX + GAP_PX);
    let maxHeight = $state(240);
    let underModal = $state(false);

    function measure(): void {
        const area = document.querySelector('[data-testid="terminal-area"]');
        if (!(area instanceof HTMLElement)) return;
        const rect = area.getBoundingClientRect();
        // The floor of the stack: the bottom of the terminal area, less the padding, two lines and a gap.
        let wanted = window.innerHeight - rect.bottom + AREA_PAD_PX + 2 * LINE_PX + GAP_PX;
        // The real terminal: the terminal rows stop above the bottom of the area (a part of a line is left over). When
        // the terminal on view has rows, the stack stands above the second row from the bottom.
        const rows = document.querySelectorAll(".pane:not(.hidden) .xterm-rows > div");
        if (rows.length >= 2) wanted = Math.max(wanted, window.innerHeight - rows[rows.length - 2].getBoundingClientRect().top + GAP_PX);
        bottom = Math.max(0, Math.round(wanted));
        // The stack never grows over the tabs: its top stays in the terminal area.
        maxHeight = Math.max(60, Math.floor(window.innerHeight - bottom - rect.top - 4));
    }

    // The rows of the terminal move when another tab comes in view, or when a toast comes (the page is the same, the
    // terminal may have fitted since). Measure again after the next frame.
    $effect(() => {
        void store.activeId;
        void visible.length;
        const frame = requestAnimationFrame(() => requestAnimationFrame(measure));
        return () => cancelAnimationFrame(frame);
    });

    onMount(() => {
        underModal = modalOpen();
        const stopModal = onModalChange((open) => (underModal = open));
        measure();
        const observer = new ResizeObserver(measure);
        const area = document.querySelector('[data-testid="terminal-area"]');
        if (area) observer.observe(area);
        window.addEventListener("resize", measure);
        return () => {
            stopModal();
            observer.disconnect();
            window.removeEventListener("resize", measure);
        };
    });
</script>

<!-- `data-keep-live`: a modal does not make the toasts inert. An error can need a click while a modal is open. -->
<div
    class="toasts"
    data-keep-live
    aria-live="polite"
    data-testid="toasts"
    class:modal={underModal}
    style:left="{underModal ? 0 : layout.colW}px"
    style:right="{underModal ? 0 : layout.boardW}px"
    style:bottom="{underModal ? MODAL_BOTTOM_PX : bottom}px"
    style:max-height="{underModal ? 360 : maxHeight}px"
>
    {#if hidden > 0}
        <div class="more" data-testid="toasts-more">{hidden} older {hidden === 1 ? "message" : "messages"} wait. Close a message to see the next one.</div>
    {/if}
    {#each visible as toast (toast.id)}
        <div class="toast" class:info={toast.kind === "info"} role={toast.kind === "info" ? "status" : "alert"} data-kind={toast.kind ?? "error"}>
            <span class="text">{toast.text}</span>
            {#if toast.action}
                <button type="button" class="act" onclick={() => { toast.action?.run(); store.dismissToast(toast.id); }}>{toast.action.label}</button>
            {/if}
            <button type="button" class="ghost" aria-label="Dismiss" onclick={() => store.dismissToast(toast.id)}>x</button>
        </div>
    {/each}
</div>

<style>
    .toasts {
        position: fixed;
        z-index: 60;
        /* `left`, `right` and `bottom` come from the page: the edges of the centre column, and the top of the last two
           terminal lines. The stack is on the right of that box. */
        box-sizing: border-box;
        padding: 0 10px;
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        justify-content: flex-end;
        gap: 6px;
        overflow-y: auto;
        pointer-events: none;
    }

    .toast {
        width: min(380px, 100%);
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

    .toast.info .act {
        border-color: var(--accent);
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
            transform: translateY(6px);
        }
    }
</style>
