<script lang="ts">
    // A drag handle between two areas. `orientation` is the orientation of the separator line:
    // "vertical" sits between a left and a right area and changes a width (left and right arrow
    // keys); "horizontal" sits between a top and a bottom area and changes a height (up and down
    // arrow keys). A drag uses pointer capture, so it goes on over a terminal or an SVG.
    // `value` is the size in px. `onchange` gets the wanted size. The owner clamps it.
    // `reverse` is for a pane on the right (or at the bottom) of the handle: a drag to the left
    // makes the pane wider.
    import { KEY_STEP } from "$lib/sizes";

    let {
        orientation,
        value,
        min,
        max,
        label,
        place = "",
        reverse = false,
        onchange,
        onreset,
    }: {
        orientation: "vertical" | "horizontal";
        value: number;
        min: number;
        max: number;
        label: string;
        /** CSS for the position (left, bottom, ...). The handle is absolute inside its parent. */
        place?: string;
        reverse?: boolean;
        onchange: (px: number) => void;
        onreset: () => void;
    } = $props();

    let dragging = $state(false);
    let startPos = 0;
    let startValue = 0;

    const horizontal = $derived(orientation === "horizontal");

    function pos(e: PointerEvent): number {
        return horizontal ? e.clientY : e.clientX;
    }

    function down(e: PointerEvent): void {
        if (e.button !== 0) return;
        e.preventDefault();
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        dragging = true;
        startPos = pos(e);
        startValue = value;
    }

    function move(e: PointerEvent): void {
        if (!dragging) return;
        // The button was released where we could not see it.
        if (e.buttons === 0) {
            dragging = false;
            return;
        }
        const delta = pos(e) - startPos;
        onchange(startValue + (reverse ? -delta : delta));
    }

    function end(): void {
        dragging = false;
    }

    function key(e: KeyboardEvent): void {
        const back = horizontal ? "ArrowUp" : "ArrowLeft";
        const forward = horizontal ? "ArrowDown" : "ArrowRight";
        const sign = reverse ? -1 : 1;
        if (e.key === back) onchange(value - sign * KEY_STEP);
        else if (e.key === forward) onchange(value + sign * KEY_STEP);
        else return;
        e.preventDefault();
    }
</script>

<!-- A focusable separator is an interactive widget (WAI-ARIA window splitter). Svelte does not know this. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
    class="handle {orientation}"
    class:dragging
    role="separator"
    aria-orientation={orientation}
    aria-label={label}
    aria-valuenow={Math.round(value)}
    aria-valuemin={Math.round(min)}
    aria-valuemax={Math.round(max)}
    tabindex="0"
    title="{label}. Drag, or use the arrow keys. Double click resets."
    style={place}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={end}
    onpointercancel={end}
    onlostpointercapture={end}
    ondblclick={onreset}
    onkeydown={key}
></div>

<style>
    .handle {
        position: absolute;
        z-index: 20;
        touch-action: none;
        outline: none;
    }

    .handle.vertical {
        top: 0;
        bottom: 0;
        width: 6px;
        cursor: col-resize;
    }

    .handle.horizontal {
        left: 0;
        right: 0;
        height: 6px;
        cursor: row-resize;
    }

    /* The visible line: hidden until hover, focus or drag. */
    .handle::after {
        content: "";
        position: absolute;
        background: var(--accent);
        opacity: 0;
    }

    .handle.vertical::after {
        top: 0;
        bottom: 0;
        left: 2px;
        width: 2px;
    }

    .handle.horizontal::after {
        left: 0;
        right: 0;
        top: 2px;
        height: 2px;
    }

    .handle:hover::after,
    .handle:focus-visible::after,
    .handle.dragging::after {
        opacity: 0.85;
    }

    @media (prefers-reduced-motion: no-preference) {
        .handle::after {
            transition: opacity 0.12s;
        }
    }
</style>
