<script lang="ts">
    // The toasts are an overlay at the TOP right of the terminal area of the CENTRE column. The top edge of the stack is 6 px
    // under the top of the terminal area (just under the tab row). The right edge is 10 px inside the right edge of the centre
    // column. The stack grows DOWN. The newest toast is at the top, nearest to that anchor. At most 4 show. The line "N older
    // messages wait" is under the stack.
    // The stack never reaches into the last 8 rows of the terminal on view (the input of the CLI and its status lines). With no
    // terminal rows to measure, it stays above the bottom of the area less 130 px. `max-height` comes from that limit, and the
    // stack scrolls inside when it is taller.
    // It covers no control: not the top strip, not the office, not the tabs, and not the task board (the board is the
    // column at the right, and the stack stops at its edge). It never changes the layout and it never takes the focus.
    // The stack shows the newest toasts that fit WHOLE when they are folded, from 1 to 4 (see toast-fit.ts). The count comes from
    // the measured height of the stack and the measured folded height of each toast. The line "N older messages wait" counts the
    // others. A toast that the operator opens with "more" does not change the count: the stack scrolls inside, as before.
    // While a modal is open the stack stays where it is. It is under the backdrop of the modal (a lower `z-index`), it takes no
    // pointer event, and it is `inert` like the rest of the page (see modal.ts). An information toast waits: its timer
    // pauses while a modal is open (see the store). An information toast closes by itself (see TOAST_INFO_MS in the store).
    // An error toast stays until the operator closes it, except the toast of a blocked merge while the detail of its task
    // is open (see TOAST_TASK_MS in the store).
    import { onMount } from "svelte";
    import { layout } from "$lib/layout.svelte";
    import { TOASTS_VISIBLE, store } from "$lib/store.svelte";
    import { toastsThatFit } from "$lib/toast-fit";
    import ToastItem from "./ToastItem.svelte";

    /** The gap between the top of the terminal area and the stack. */
    const TOP_GAP_PX = 6;
    /** The rows of the terminal that the stack never covers: the input of the CLI and its status lines. */
    const KEEP_ROWS = 8;
    /** The gap between the stack and the first row that it must not cover. */
    const ROWS_GAP_PX = 4;
    /** With no terminal rows to measure: the stack ends this far above the bottom of the terminal area. */
    const FALLBACK_BOTTOM_PX = 130;

    /** The height of the line "N older messages wait", until the page has measured it. */
    const MORE_FALLBACK_PX = 24;
    /** The gap between two items of the stack, until the page has read it from the style. */
    const GAP_FALLBACK_PX = 6;

    let top = $state(TOP_GAP_PX + 40);
    let maxHeight = $state(240);
    let gap = $state(GAP_FALLBACK_PX);
    let moreHeight = $state(MORE_FALLBACK_PX);
    let stackEl: HTMLElement | undefined = $state();
    let moreEl: HTMLElement | undefined = $state();
    // The folded height of each toast, from its own measure (ToastItem). A toast that is open keeps its folded height.
    let folded = $state.raw<Record<number, number>>({});

    function onfold(id: number, height: number): void {
        if (folded[id] !== height) folded = { ...folded, [id]: height };
    }

    // The newest first: the store keeps the oldest first.
    const candidates = $derived(store.toasts.slice(-TOASTS_VISIBLE).reverse());
    // A toast with no measure yet takes the height of the tallest toast that has one. With none measured, it counts as not
    // fitting, so the newest toast shows alone, gets its measure, and then the count settles (before the next paint).
    const guess = $derived(Math.max(0, ...candidates.map((t) => folded[t.id] ?? 0)) || Infinity);
    const shown = $derived(
        toastsThatFit({
            available: maxHeight,
            heights: candidates.map((t) => folded[t.id] ?? guess),
            gap,
            moreHeight,
            total: store.toasts.length,
            max: TOASTS_VISIBLE,
        }),
    );
    const visible = $derived(candidates.slice(0, shown));
    const hidden = $derived(Math.max(0, store.toasts.length - visible.length));

    // The area moves when the debug bar shows, so the page measures it again when the area changes or the window resizes.
    function measure(): void {
        const area = document.querySelector('[data-testid="terminal-area"]');
        if (!(area instanceof HTMLElement)) return;
        const rect = area.getBoundingClientRect();
        const start = rect.top + TOP_GAP_PX;
        // The floor of the stack: the top of the 8th row from the bottom of the terminal on view, else the area less 130 px.
        let limit = rect.bottom - FALLBACK_BOTTOM_PX;
        const rows = document.querySelectorAll(".pane:not(.hidden) .xterm-rows > div");
        // A terminal can have more rows than fit in the area for a moment (before it is fitted). So the rows can only
        // make the limit higher on the screen, never lower than the limit of the area.
        if (rows.length >= KEEP_ROWS) limit = Math.min(limit, rows[rows.length - KEEP_ROWS].getBoundingClientRect().top - ROWS_GAP_PX);
        top = Math.round(start);
        maxHeight = Math.max(0, Math.floor(limit - start));
        if (stackEl) gap = parseFloat(getComputedStyle(stackEl).rowGap) || GAP_FALLBACK_PX;
    }

    // The height of the line for the older toasts, when it is on the page.
    $effect(() => {
        const el = moreEl;
        if (!el) return;
        const read = (): void => {
            moreHeight = Math.ceil(el.getBoundingClientRect().height) || MORE_FALLBACK_PX;
        };
        read();
        const observer = new ResizeObserver(read);
        observer.observe(el);
        return () => observer.disconnect();
    });

    // The rows of the terminal move when another tab comes in view, or when a toast comes (the terminal may have fitted
    // since). Measure again after the next frame.
    $effect(() => {
        void store.activeId;
        void visible.length;
        const frame = requestAnimationFrame(() => requestAnimationFrame(measure));
        return () => cancelAnimationFrame(frame);
    });

    onMount(() => {
        measure();
        const observer = new ResizeObserver(measure);
        const area = document.querySelector('[data-testid="terminal-area"]');
        if (area) observer.observe(area);
        window.addEventListener("resize", measure);
        return () => {
            observer.disconnect();
            window.removeEventListener("resize", measure);
        };
    });
</script>

<div
    class="toasts"
    aria-live="polite"
    data-testid="toasts"
    bind:this={stackEl}
    style:left="{layout.colW}px"
    style:right="{layout.boardW}px"
    style:top="{top}px"
    style:max-height="{maxHeight}px"
>
    {#each visible as toast (toast.id)}
        <ToastItem {toast} {onfold} />
    {/each}
    {#if hidden > 0}
        <div class="more" data-testid="toasts-more" bind:this={moreEl}>{hidden} older {hidden === 1 ? "message" : "messages"} wait. Close a message to see the next one.</div>
    {/if}
</div>

<style>
    .toasts {
        position: fixed;
        /* Under the backdrop of a modal (`z-index: 40` in the employee form, the settings panel and the task reader). */
        z-index: 30;
        /* `left`, `right`, `top` and `max-height` come from the page: the edges of the centre column, and the top of the
           terminal area. The stack is on the right of that box. */
        box-sizing: border-box;
        padding: 0 10px;
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        gap: 6px;
        overflow-y: auto;
        overscroll-behavior: contain;
        pointer-events: none;
    }

    .more {
        flex: none;
        pointer-events: auto;
        padding: 2px 8px;
        border-radius: 10px;
        background: var(--panel);
        border: 1px solid var(--line);
        color: var(--dim);
        font-size: 11px;
    }
</style>
