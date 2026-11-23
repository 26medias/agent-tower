<script lang="ts">
    // The office: the info panel on the left (header, buttons, selected employee) and the room on
    // the right. The operator changes the height with the handle under the office, and the split
    // between the info panel and the room with the handle between them (see layout.svelte.ts).
    // The area of the room has two tabs: "Floor" (the room, the default) and "Hierarchy" (the organisation chart). The chosen
    // tab lives in the store, in memory. The room stays mounted under the other tab, so its zoom is kept.
    import { layout } from "$lib/layout.svelte";
    import type { Employee } from "$lib/types";
    import { store, type OfficeTab } from "$lib/store.svelte";
    import EmployeeBar from "./EmployeeBar.svelte";
    import EmptyOffice from "./EmptyOffice.svelte";
    import HierarchyChart from "./HierarchyChart.svelte";
    import OfficeHeader from "./OfficeHeader.svelte";
    import OfficeRoom from "./OfficeRoom.svelte";
    import ResizeHandle from "./ResizeHandle.svelte";
    import Roster from "./Roster.svelte";
    import TeamActivity from "./TeamActivity.svelte";

    let { onedit, onadd }: { onedit: (employee: Employee) => void; onadd: () => void } = $props();

    // The list "Team activity" shows under the roster only when the info panel has room for it. At
    // 1280 x 800 the panel is full, so the list is hidden. It does not change what `.top` needs.
    const ACTIVITY_MIN_PX = 160;
    let infoEl = $state<HTMLElement | null>(null);
    let topEl = $state<HTMLElement | null>(null);
    let spare = $state(0);
    const showActivity = $derived(spare >= ACTIVITY_MIN_PX);

    function measureSpare(): void {
        if (!infoEl || !topEl) return;
        // The padding of the panel (8 px up and down) and the gap before the list (8 px) are not free.
        spare = infoEl.clientHeight - 16 - topEl.offsetHeight - 8;
    }

    $effect(() => {
        if (!infoEl || !topEl) return;
        const observer = new ResizeObserver(measureSpare);
        observer.observe(infoEl);
        observer.observe(topEl);
        measureSpare();
        return () => observer.disconnect();
    });

    const OFFICE_TABS: { id: OfficeTab; label: string }[] = [
        { id: "floor", label: "Floor" },
        { id: "hierarchy", label: "Hierarchy" },
    ];

    /** Arrow keys, Home and End move between the two tabs, as in a tab list of the ARIA pattern. */
    function tabKey(e: KeyboardEvent): void {
        const at = OFFICE_TABS.findIndex((t) => t.id === store.officeTab);
        let to = at;
        if (e.key === "ArrowRight") to = (at + 1) % OFFICE_TABS.length;
        else if (e.key === "ArrowLeft") to = (at - 1 + OFFICE_TABS.length) % OFFICE_TABS.length;
        else if (e.key === "Home") to = 0;
        else if (e.key === "End") to = OFFICE_TABS.length - 1;
        else return;
        e.preventDefault();
        store.officeTab = OFFICE_TABS[to].id;
        document.getElementById(`office-tab-${OFFICE_TABS[to].id}`)?.focus();
    }

    // The default height of the office depends on the number of employees.
    $effect(() => {
        layout.employees = store.orderedEmployees.length;
    });
</script>

<section class="office" aria-label="Office" style:--office-h="{layout.officeH}px" style:--info-w="{layout.infoW}px">
    {#if !store.floorView && store.ready}
        <EmptyOffice />
    {:else}
        <div class="info" bind:this={infoEl} data-testid="info-panel">
            <div class="top" bind:this={topEl}>
                <OfficeHeader />
                <EmployeeBar {onedit} />
                <Roster {onadd} />
            </div>
            {#if showActivity}
                <TeamActivity />
            {/if}
        </div>
        <div class="room">
            <div class="view" id="office-panel-floor" role="tabpanel" aria-labelledby="office-tab-floor" hidden={store.officeTab !== "floor"}>
                <OfficeRoom />
            </div>
            {#if store.officeTab === "hierarchy"}
                <HierarchyChart />
            {/if}
            <!-- The tab bar floats over the top left corner, like the zoom buttons over the bottom right one. The room keeps its size. -->
            <div class="office-tabs" role="tablist" aria-label="Floor view" data-testid="office-tabs">
                {#each OFFICE_TABS as tab (tab.id)}
                    <button
                        type="button"
                        role="tab"
                        id="office-tab-{tab.id}"
                        class="ghost"
                        aria-selected={store.officeTab === tab.id}
                        aria-controls="office-panel-{tab.id}"
                        data-testid="office-tab-{tab.id}"
                        onclick={() => (store.officeTab = tab.id)}
                        onkeydown={tabKey}
                    >
                        {tab.label}
                    </button>
                {/each}
            </div>
        </div>
        <ResizeHandle
            orientation="vertical"
            label="Resize the info panel and the room"
            value={layout.infoW}
            min={layout.infoRangeNow.min}
            max={layout.infoRangeNow.max}
            place="left: var(--info-w)"
            onchange={(px) => layout.setInfo(px)}
            onreset={() => layout.resetInfo()}
        />
    {/if}
    {#if store.gone}
        <!-- The folder of the floor is gone: dim the office. An overlay does it: a CSS `filter` on SVG children is not drawn by WebKitGTK.
             The overlay takes no pointer event, so the "stop" buttons of live sessions (above it) and the resize handles still work. -->
        <div class="dim" data-testid="gone-dim" aria-hidden="true"></div>
    {/if}
    <ResizeHandle
        orientation="horizontal"
        label="Resize the office"
        value={layout.officeH}
        min={layout.officeRangeNow.min}
        max={layout.officeRangeNow.max}
        place="bottom: -5px"
        onchange={(px) => layout.setOffice(px)}
        onreset={() => layout.resetOffice()}
    />
</section>

<style>
    .office {
        position: relative;
        display: grid;
        grid-template-columns: var(--info-w) minmax(0, 1fr);
        grid-template-rows: minmax(0, 1fr);
        height: var(--office-h);
        min-width: 0;
        border-bottom: 1px solid var(--line);
        background: var(--bg);
    }

    .info {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-width: 0;
        min-height: 0;
        overflow-y: auto;
        padding: 8px 10px;
    }

    .top {
        display: flex;
        flex: none;
        flex-direction: column;
        gap: 8px;
        min-width: 0;
    }

    .dim {
        position: absolute;
        inset: 0;
        z-index: 2;
        background: rgba(11, 17, 28, 0.55);
        pointer-events: none;
    }

    /* The room fills the rest. The SVG inside scales to fit and stays centred. */
    .room {
        position: relative;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        border-left: 1px solid var(--line);
    }

    .view {
        position: absolute;
        inset: 0;
    }

    .office-tabs {
        position: absolute;
        z-index: 3;
        top: 6px;
        left: 8px;
        display: flex;
        gap: 2px;
        padding: 2px;
        border: 1px solid var(--line);
        border-radius: 8px;
        background: rgba(11, 17, 28, 0.8);
    }

    .office-tabs button {
        padding: 0 10px;
        font-size: 12px;
    }

    .office-tabs button[aria-selected="true"] {
        color: var(--accent);
        border-color: var(--accent);
    }
</style>
