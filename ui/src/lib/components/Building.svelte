<script lang="ts">
    // The building: one storey for each floor, the first floor at the bottom. A click on a storey
    // opens the floor. The open floor is bright and the others are dim. A missing floor is darker.
    // The dim comes from a dark overlay polygon on the storey (fill and fill-opacity). A CSS
    // `filter` on SVG child elements is not drawn by WebKitGTK, the webview of Tauri on Linux.
    import { ART, T, buildingHeight, stickerTransform, storeyY } from "$lib/office-layout";
    import { store } from "$lib/store.svelte";

    const count = $derived(store.floors.length);

    // The part of the storey image that reacts to the pointer (the roof and the walls). The images
    // do not take pointer events: their boxes overlap, so that a high storey would cover a low one.
    const HIT = "770,124 1436,400 1436,712 770,958 100,682 100,400";

    function basename(path: string): string {
        const parts = path.split(/[\\/]/).filter((p) => p !== "");
        return parts.length > 0 ? parts[parts.length - 1] : path;
    }

    function nameOf(floor: { name: string | null; path: string }): string {
        return floor.name ?? basename(floor.path);
    }

    function open(floor: { path: string; missing: boolean }): void {
        if (!floor.missing) void store.openFloor(floor.path);
    }
</script>

{#if count === 0}
    <p class="empty">No floor yet. Open a folder.</p>
{:else}
    <svg
        class="building"
        viewBox="0 0 {ART.w} {buildingHeight(count)}"
        role="group"
        aria-label="Building, one storey for each floor"
        data-storeys={count}
    >
        {#each store.floors as floor, i (floor.path)}
            {@const y = storeyY(i, count)}
            {@const logo = store.logos[floor.path]}
            {@const questions = store.questionsIn(floor.path)}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <g
                class="storey"
                class:on={floor.path === store.openPath}
                class:missing={floor.missing}
                role="button"
                tabindex={floor.missing ? -1 : 0}
                aria-label="Floor {i + 1}, {nameOf(floor)}{floor.missing ? ', folder missing' : ''}"
                aria-pressed={floor.path === store.openPath}
                aria-disabled={floor.missing}
                data-path={floor.path}
                data-index={i}
                onclick={() => open(floor)}
                onkeydown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        open(floor);
                    }
                }}
            >
                <title>{floor.missing ? `${nameOf(floor)} (the folder is missing)` : `${nameOf(floor)}\n${floor.path}`}</title>
                <image href="/art/floor-pixel.webp" x="0" {y} width={ART.w} height={ART.h} />
                <g transform={stickerTransform(y)}>
                    {#if logo}
                        <image class="logo" href={logo} x={-T.sw / 2} y={-T.sw / 2} width={T.sw} height={T.sw} />
                    {:else}
                        <rect class="plain" x={-T.sw / 2} y={-T.sw / 2} width={T.sw} height={T.sw} rx="34" />
                        <rect class="plain-in" x={-T.sw / 2 + 12} y={-T.sw / 2 + 12} width={T.sw - 24} height={T.sw - 24} rx="26" />
                        <text class="letter" x="0" y="26" text-anchor="middle">{[...nameOf(floor).trim()][0]?.toUpperCase() ?? "?"}</text>
                    {/if}
                </g>
                <polygon class="shade" points={HIT} transform="translate(0 {y})" />
                {#if questions > 0}
                    <g class="badge" transform="translate(1250 {y + 430})">
                        <circle r="34" />
                        <text y="14" text-anchor="middle">{questions}</text>
                    </g>
                {/if}
                <polygon class="hit" points={HIT} transform="translate(0 {y})" />
            </g>
        {/each}
    </svg>
{/if}

<style>
    .building {
        display: block;
        width: 100%;
        height: auto;
    }

    .empty {
        margin: 0;
        padding: 8px;
        color: var(--dim);
    }

    .storey {
        cursor: pointer;
        outline: none;
    }

    .storey image {
        pointer-events: none;
    }

    .hit {
        fill: transparent;
        pointer-events: all;
    }

    /* The overlay of a storey. Dim for a floor that is not open, lighter on hover or focus, none
       for the open floor, strong for a missing floor. The order of the rules matters. */
    .shade {
        fill: #0b111c;
        fill-opacity: 0.5;
        pointer-events: none;
    }

    .storey:hover .shade {
        fill-opacity: 0.25;
    }

    .storey:focus-visible .shade {
        fill-opacity: 0.1;
    }

    .storey.on .shade {
        fill-opacity: 0;
    }

    .storey.missing {
        cursor: default;
    }

    .storey.missing .shade {
        fill: #07090d;
        fill-opacity: 0.75;
    }

    .plain {
        fill: #ffffff;
    }

    .plain-in {
        fill: #33487a;
    }

    .letter {
        font: 700 96px var(--display);
        fill: #e6ecf8;
        pointer-events: none;
    }

    .badge circle {
        fill: var(--st-question);
        stroke: #0b111c;
        stroke-width: 6;
    }

    .badge text {
        font: 700 40px var(--display);
        fill: #1a1400;
        pointer-events: none;
    }

    @media (prefers-reduced-motion: no-preference) {
        .shade {
            transition: fill-opacity 0.15s;
        }
    }
</style>
