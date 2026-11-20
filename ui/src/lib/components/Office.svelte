<script lang="ts">
    // The office: the info panel on the left (header, buttons, selected employee) and the room on
    // the right. The height is bounded, so that the terminal below keeps at least half of the window.
    import type { Employee } from "$lib/types";
    import EmployeeBar from "./EmployeeBar.svelte";
    import OfficeHeader from "./OfficeHeader.svelte";
    import OfficeRoom from "./OfficeRoom.svelte";

    let { onedit, onadd }: { onedit: (employee: Employee) => void; onadd: () => void } = $props();

    let big = $state(false);
</script>

<section class="office" class:big aria-label="Office">
    <div class="info">
        <OfficeHeader />
        <EmployeeBar {onedit} {onadd} />
    </div>
    <div class="room">
        <OfficeRoom />
        <button
            type="button"
            class="size"
            aria-pressed={big}
            title="Make the office bigger or smaller. A bigger office leaves less room for the terminal."
            onclick={() => (big = !big)}
        >
            {big ? "smaller office" : "bigger office"}
        </button>
    </div>
</section>

<style>
    .office {
        /* At most 42% of the window: the terminal keeps at least half of the height. */
        --office-h: clamp(220px, 42vh, 640px);
        display: grid;
        grid-template-columns: minmax(300px, 1fr) auto;
        height: var(--office-h);
        min-width: 0;
        border-bottom: 1px solid var(--line);
        background: var(--bg);
    }

    .office.big {
        --office-h: clamp(220px, 66vh, 760px);
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

    .size {
        position: absolute;
        top: 6px;
        right: 6px;
        padding: 1px 7px;
        background: rgba(11, 17, 28, 0.8);
        color: var(--dim);
        font-size: 11px;
    }

    .room {
        position: relative;
        height: 100%;
        aspect-ratio: 3 / 2;
        /* The info panel keeps 320px. The left column is as wide as in +page.svelte. */
        max-width: calc(100vw - clamp(260px, 25vw, 360px) - 320px);
        min-width: 0;
        border-left: 1px solid var(--line);
    }
</style>
