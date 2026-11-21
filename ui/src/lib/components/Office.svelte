<script lang="ts">
    // The office: the info panel on the left (header, buttons, selected employee) and the room on
    // the right. The operator changes the height with the handle under the office, and the split
    // between the info panel and the room with the handle between them (see layout.svelte.ts).
    import { layout } from "$lib/layout.svelte";
    import type { Employee } from "$lib/types";
    import EmployeeBar from "./EmployeeBar.svelte";
    import OfficeHeader from "./OfficeHeader.svelte";
    import OfficeRoom from "./OfficeRoom.svelte";
    import ResizeHandle from "./ResizeHandle.svelte";
    import Roster from "./Roster.svelte";

    let { onedit, onadd }: { onedit: (employee: Employee) => void; onadd: () => void } = $props();
</script>

<section class="office" aria-label="Office" style:--office-h="{layout.officeH}px" style:--info-w="{layout.infoW}px">
    <div class="info">
        <OfficeHeader />
        <EmployeeBar {onedit} {onadd} />
        <Roster />
    </div>
    <div class="room">
        <OfficeRoom />
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

    /* The room fills the rest. The SVG inside scales to fit and stays centred. */
    .room {
        position: relative;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        border-left: 1px solid var(--line);
    }
</style>
