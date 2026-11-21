<script lang="ts">
    // The room of the open floor: one desk and one character for each employee, with a name tag.
    // Draw order: the room, then each employee from the back to the front (the character first,
    // then the desk, so that the desk covers the legs), then all name tags in a top layer.
    import {
        ART,
        STATUS_COLOR,
        STATUS_GLYPH,
        TAG_FONT,
        TAG_ROLE_FONT,
        TAG_ROLE_MAX_EMPLOYEES,
        layoutOffice,
        readableTagScale,
        roleColor,
        spriteFor,
        tagBoxWidth,
        tagLabel,
        tagRole,
        tagY,
    } from "$lib/office-layout";
    import { store } from "$lib/store.svelte";
    import type { Status } from "$lib/types";

    const employees = $derived(store.orderedEmployees);
    const layout = $derived(layoutOffice(employees.length));
    const showRole = $derived(employees.length <= TAG_ROLE_MAX_EMPLOYEES);
    const selectedSlug = $derived(store.selectedEmployee?.slug ?? null);

    // Screen pixels for one SVG unit. The tag text never gets smaller than 10 px on the screen.
    let boxW = $state(0);
    let boxH = $state(0);
    const viewScale = $derived(boxW > 0 && boxH > 0 ? Math.min(boxW / ART.w, boxH / ART.h) : 0);
    const tagScaleNow = $derived(readableTagScale(layout.k, viewScale, 10));

    function statusOf(slug: string): Status {
        return store.sessionOfEmployee(slug)?.status ?? "stopped";
    }

    function press(e: KeyboardEvent, slug: string): void {
        if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            store.selectEmployee(slug);
        }
    }
</script>

<div class="room" bind:clientWidth={boxW} bind:clientHeight={boxH}>
    <svg
        viewBox="0 0 {ART.w} {ART.h}"
        role="group"
        aria-label="Office of {store.floorView?.name ?? 'no floor'} with {employees.length} employees"
        data-count={employees.length}
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
            {#each layout.placements as p (employees[p.index].slug)}
                {@const emp = employees[p.index]}
                {@const status = statusOf(emp.slug)}
                {@const color = STATUS_COLOR[status]}
                {@const label = tagLabel(emp.name)}
                {@const role = showRole ? tagRole(emp.role) : ""}
                {@const w = tagBoxWidth(label, role)}
                {@const gx = -(58 + [...label].length * 17) / 2 + 24}
                <g
                    class="tag"
                    data-slug={emp.slug}
                    aria-hidden="true"
                    transform="translate({p.fx} {tagY(p, tagScaleNow)}) scale({tagScaleNow})"
                    onclick={() => store.selectEmployee(emp.slug)}
                >
                    <g class="bubble {status}">
                        <polygon points="-8,18 8,18 0,30" fill="#0b111c" />
                        <rect x={-w / 2} y={showRole ? -46 : -20} width={w} height={showRole ? 66 : 40} rx="10" fill="#0b111c" stroke={color} stroke-width="4" />
                        {#if showRole}
                            <text class="tag-role" x="0" y="-20" text-anchor="middle" font-size={TAG_ROLE_FONT} fill={roleColor(emp.role)}>{role}</text>
                        {/if}
                        {#if status === "working" || status === "starting"}
                            {#each [-9, 0, 9] as dx, i (dx)}
                                <circle class="d d{i + 1}" cx={gx + dx} cy="0" r="3.5" fill={color} />
                            {/each}
                        {:else}
                            <text x={gx} y="9" text-anchor="middle" font-size={TAG_FONT} fill={color}>{STATUS_GLYPH[status]}</text>
                        {/if}
                        <text class="tag-name" x={gx + 20} y="9" font-size={TAG_FONT} fill={emp.slug === selectedSlug ? "#5ee0c0" : "#e6ecf8"}>{label}</text>
                    </g>
                </g>
            {/each}
        </g>
    </svg>
</div>

<style>
    .room {
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

    @media (prefers-reduced-motion: no-preference) {
        .bubble.question {
            animation: hop 0.9s ease-in-out infinite;
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
