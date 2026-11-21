<script lang="ts">
    // The room of the open floor: one desk and one character for each employee, with a name tag.
    // Draw order: the room, then each employee from the back to the front (the character first,
    // then the desk, so that the desk covers the legs), then all name tags in a top layer.
    // The wheel (or the buttons) zooms the room. A drag on an empty place moves a zoomed room.
    import {
        ART,
        STATUS_COLOR,
        STATUS_GLYPH,
        TAG_FONT,
        TAG_ROLE_FONT,
        TAG_ROLE_MAX_EMPLOYEES,
        CHIP_GAP,
        chipBoxWidth,
        chipWidth,
        layoutOffice,
        nameWidth,
        readableTagScale,
        roleChip,
        roleColor,
        rolePopWidth,
        spriteFor,
        tagBoxWidth,
        tagLabel,
        tagRole,
        tagWidth,
        tagY,
    } from "$lib/office-layout";
    import { fullView, panView, zoomView, ZOOM_MAX } from "$lib/room-view";
    import { store } from "$lib/store.svelte";
    import type { Status } from "$lib/types";

    const employees = $derived(store.orderedEmployees);
    const layout = $derived(layoutOffice(employees.length));
    const showRole = $derived(employees.length <= TAG_ROLE_MAX_EMPLOYEES);
    const selectedSlug = $derived(store.selectedEmployee?.slug ?? null);
    // The tag of the selected employee is the last one: it is on top of the others.
    const tagOrder = $derived(
        [...layout.placements].sort(
            (a, b) => Number(employees[a.index].slug === selectedSlug) - Number(employees[b.index].slug === selectedSlug),
        ),
    );

    // The part of the picture that shows. Zoom 1 shows all of it.
    let view = $state(fullView());
    const zoomed = $derived(view.w < ART.w - 0.5);

    // Screen pixels for one SVG unit. The tag text never gets smaller than 10 px on the screen.
    let boxW = $state(0);
    let boxH = $state(0);
    let svg: SVGSVGElement;
    const viewScale = $derived(boxW > 0 && boxH > 0 ? Math.min(boxW / view.w, boxH / view.h) : 0);
    // The tag grows with the zoom up to its natural size (see `readableTagScale`).
    const tagScaleNow = $derived(readableTagScale(layout.k, viewScale, 10, ART.w / view.w));

    function statusOf(slug: string): Status {
        return store.sessionOfEmployee(slug)?.status ?? "stopped";
    }

    function press(e: KeyboardEvent, slug: string): void {
        if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            store.selectEmployee(slug);
        }
    }

    // ---- zoom and move ----

    /** A point of the screen as a point of the picture (SVG units). */
    function toUnits(clientX: number, clientY: number): { x: number; y: number } {
        const m = svg.getScreenCTM();
        if (!m) return { x: view.x + view.w / 2, y: view.y + view.h / 2 };
        const p = new DOMPoint(clientX, clientY).matrixTransform(m.inverse());
        return { x: p.x, y: p.y };
    }

    function zoomAt(factor: number, at?: { x: number; y: number }): void {
        view = zoomView(view, factor, at ?? { x: view.x + view.w / 2, y: view.y + view.h / 2 });
    }

    // A wheel handler must not be passive: it stops the page from scrolling. Svelte makes `onwheel` passive.
    function wheel(node: SVGSVGElement) {
        const onwheel = (e: WheelEvent): void => {
            e.preventDefault();
            zoomAt(Math.exp(-e.deltaY * 0.0015), toUnits(e.clientX, e.clientY));
        };
        node.addEventListener("wheel", onwheel, { passive: false });
        return { destroy: () => node.removeEventListener("wheel", onwheel) };
    }

    let drag: { id: number; x: number; y: number } | null = null;

    function down(e: PointerEvent): void {
        if (!zoomed || e.button !== 0) return;
        if ((e.target as Element).closest(".emp, .tag")) return;
        drag = { id: e.pointerId, x: e.clientX, y: e.clientY };
        svg.setPointerCapture(e.pointerId);
    }

    function move(e: PointerEvent): void {
        if (!drag || drag.id !== e.pointerId) return;
        const m = svg.getScreenCTM();
        if (!m) return;
        view = panView(view, (e.clientX - drag.x) / m.a, (e.clientY - drag.y) / m.d);
        drag = { id: e.pointerId, x: e.clientX, y: e.clientY };
    }

    function up(e: PointerEvent): void {
        if (drag && drag.id === e.pointerId) drag = null;
    }

    // A new floor starts with the whole room.
    $effect(() => {
        void store.openPath;
        view = fullView();
    });
</script>

<div class="room" bind:clientWidth={boxW} bind:clientHeight={boxH}>
    <svg
        bind:this={svg}
        use:wheel
        viewBox="{view.x} {view.y} {view.w} {view.h}"
        preserveAspectRatio="xMidYMid meet"
        role="group"
        aria-label="Office of {store.floorView?.name ?? 'no floor'} with {employees.length} employees"
        data-count={employees.length}
        data-zoom={(ART.w / view.w).toFixed(2)}
        class:zoomed
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
    >
        <image href="/art/room-pixel.webp" x="0" y="0" width={ART.w} height={ART.h} />
        {#each layout.placements as p (employees[p.index].slug)}
            {@const emp = employees[p.index]}
            {@const status = statusOf(emp.slug)}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <g
                class="emp {status}"
                class:selected={emp.slug === selectedSlug}
                role="button"
                tabindex="0"
                aria-label="{emp.name}, {emp.role}, {status}"
                aria-pressed={emp.slug === selectedSlug}
                data-slug={emp.slug}
                data-status={status}
                data-index={p.index}
                data-fx={p.fx}
                data-fy={p.fy}
                onclick={() => store.selectEmployee(emp.slug)}
                onkeydown={(e) => press(e, emp.slug)}
            >
                <ellipse cx={p.fx} cy={p.fy - 3} rx={p.ah * 0.2} ry={p.ah * 0.065} fill="#000" opacity="0.35" />
                <image
                    class="sprite"
                    href="/art/agent-{spriteFor(emp.role, emp.slug)}.webp"
                    x={p.fx - p.ah / 4}
                    y={p.fy - p.ah}
                    width={p.ah / 2}
                    height={p.ah}
                    preserveAspectRatio="xMidYMax meet"
                />
                <image href="/art/desk-back.webp" x={p.x0} y={p.y0} width={p.size} height={p.size} />
                <ellipse class="ring" cx={p.fx} cy={p.fy - 3} rx={p.ah * 0.3} ry={p.ah * 0.1} />
                <rect class="hit" x={p.fx - p.ah * 0.28} y={p.fy - p.ah} width={p.ah * 0.56} height={p.ah + 6} />
            </g>
        {/each}
        <g class="tags">
            {#each tagOrder as p (employees[p.index].slug)}
                {@const emp = employees[p.index]}
                {@const status = statusOf(emp.slug)}
                {@const asking = status === "question"}
                {@const color = STATUS_COLOR[status]}
                {@const label = tagLabel(emp.name)}
                {@const role = showRole ? tagRole(emp.role) : ""}
                {@const chip = showRole ? null : roleChip(emp.role)}
                {@const w = tagBoxWidth(label, role, chip)}
                {@const content = tagWidth(label) + chipWidth(chip)}
                {@const gx = -content / 2 + 24}
                {@const chipX = gx + 20 + nameWidth(label) + CHIP_GAP}
                {@const chipW = chipBoxWidth(chip)}
                {@const ink = asking ? "#1a1400" : "#e6ecf8"}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <g
                    class="tag"
                    class:selected={emp.slug === selectedSlug}
                    data-slug={emp.slug}
                    data-status={status}
                    transform="translate({p.fx} {tagY(p, tagScaleNow)}) scale({tagScaleNow})"
                    onclick={() => store.selectEmployee(emp.slug)}
                >
                    <title>{emp.name}, {emp.role}, {status}</title>
                    <g class="bubble {status}" aria-hidden="true">
                        <polygon points="-8,18 8,18 0,30" fill={asking ? color : "#0b111c"} />
                        <rect
                            class="box"
                            x={-w / 2}
                            y={showRole ? -46 : -20}
                            width={w}
                            height={showRole ? 66 : 40}
                            rx="10"
                            fill={asking ? color : "#0b111c"}
                            stroke={asking ? "#7a5410" : color}
                            stroke-width={asking ? 5 : 4}
                        />
                        {#if showRole}
                            <text class="tag-role" x="0" y="-20" text-anchor="middle" font-size={TAG_ROLE_FONT} fill={asking ? "#3b2a05" : roleColor(emp.role)}>{role}</text>
                        {/if}
                        {#if status === "working" || status === "starting"}
                            {#each [-9, 0, 9] as dx, i (dx)}
                                <circle class="d d{i + 1}" cx={gx + dx} cy="0" r="3.5" fill={color} />
                            {/each}
                        {:else}
                            <text x={gx} y="9" text-anchor="middle" font-size={TAG_FONT} fill={asking ? "#1a1400" : color}>{STATUS_GLYPH[status]}</text>
                        {/if}
                        <text class="tag-name" x={gx + 20} y="9" font-size={TAG_FONT} fill={asking ? ink : emp.slug === selectedSlug ? "#5ee0c0" : ink}>{label}</text>
                        {#if chip}
                            <g class="chip" data-chip={chip.text}>
                                <rect x={chipX} y="-15" width={chipW} height="30" rx="8" fill={asking ? "#1a1400" : chip.color} />
                                <text x={chipX + chipW / 2} y="7" text-anchor="middle" font-size={TAG_ROLE_FONT} fill={asking ? chip.color : "#0b111c"}>{chip.text}</text>
                            </g>
                        {/if}
                        {#if !showRole && !(chip && chip.text === tagRole(emp.role))}
                            <!-- The role of a crowded room shows only for the selected employee, or when the pointer is on the tag.
                                 The CEO has no box: the chip already says CEO. -->
                            {@const pw = rolePopWidth(tagRole(emp.role))}
                            <g class="role-pop" class:on={emp.slug === selectedSlug}>
                                <rect x={-pw / 2} y="-56" width={pw} height="30" rx="8" fill="#0b111c" stroke={roleColor(emp.role)} stroke-width="3" />
                                <text x="0" y="-35" text-anchor="middle" font-size={TAG_ROLE_FONT} fill={roleColor(emp.role)}>{tagRole(emp.role)}</text>
                            </g>
                        {/if}
                    </g>
                </g>
            {/each}
        </g>
    </svg>
    <div class="zoom" role="group" aria-label="Zoom the room">
        <button type="button" class="ghost" title="Zoom in (or use the wheel)" aria-label="Zoom in" disabled={ART.w / view.w >= ZOOM_MAX - 0.01} onclick={() => zoomAt(1.4)}>+</button>
        <button type="button" class="ghost" title="Zoom out" aria-label="Zoom out" disabled={!zoomed} onclick={() => zoomAt(1 / 1.4)}>-</button>
        <button type="button" class="ghost" title="Show the whole room" aria-label="Show the whole room" disabled={!zoomed} onclick={() => (view = fullView())}>fit</button>
    </div>
</div>

<style>
    .room {
        position: relative;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: #0b111c;
    }

    svg {
        display: block;
        width: 100%;
        height: 100%;
    }

    svg.zoomed {
        cursor: grab;
    }

    .zoom {
        position: absolute;
        right: 6px;
        bottom: 6px;
        display: flex;
        gap: 2px;
        padding: 2px;
        border: 1px solid var(--line);
        border-radius: 8px;
        background: rgba(11, 17, 28, 0.8);
    }

    .zoom button {
        min-width: 26px;
        padding: 0 6px;
        font-size: 13px;
    }

    .emp {
        cursor: pointer;
        outline: none;
    }

    .emp image {
        pointer-events: none;
    }

    .hit {
        fill: transparent;
        pointer-events: all;
    }

    .ring {
        fill: none;
        stroke: var(--accent);
        stroke-width: 4;
        opacity: 0;
    }

    .emp.selected .ring,
    .emp:focus-visible .ring {
        opacity: 1;
    }

    /* A session that asks a question: an amber ring on the floor, always shown. */
    .emp.question .ring {
        stroke: var(--st-question);
        stroke-width: 6;
        opacity: 1;
    }

    /* A plain opacity. A CSS `filter` on an SVG child is not drawn by WebKitGTK. */
    .emp.stopped .sprite {
        opacity: 0.45;
    }

    .tag {
        cursor: pointer;
    }

    .tag-name {
        font-family: var(--display);
        font-weight: 700;
    }

    .tag text {
        font-family: var(--display);
        font-weight: 700;
        user-select: none;
    }

    .role-pop {
        opacity: 0;
        pointer-events: none;
    }

    .tag:hover .role-pop,
    .role-pop.on {
        opacity: 1;
    }

    @media (prefers-reduced-motion: no-preference) {
        .bubble.question {
            animation: hop 0.9s ease-in-out infinite;
        }
        .emp.question .ring {
            animation: ring-pulse 1.6s ease-in-out infinite;
        }
        .bubble.working circle.d,
        .bubble.starting circle.d {
            animation: blink 1.2s infinite;
        }
        .bubble circle.d2 {
            animation-delay: 0.2s;
        }
        .bubble circle.d3 {
            animation-delay: 0.4s;
        }
        .emp.working .sprite {
            animation: bob 0.6s steps(2) infinite;
        }
    }

    @keyframes ring-pulse {
        0%,
        100% {
            opacity: 1;
        }
        50% {
            opacity: 0.35;
        }
    }

    @keyframes hop {
        0%,
        100% {
            transform: translateY(0);
        }
        50% {
            transform: translateY(-12px);
        }
    }

    @keyframes blink {
        0%,
        60%,
        100% {
            opacity: 0.25;
        }
        30% {
            opacity: 1;
        }
    }

    @keyframes bob {
        0% {
            transform: translateY(0);
        }
        100% {
            transform: translateY(-4px);
        }
    }
</style>
