<script lang="ts">
    import { onDestroy, onMount } from "svelte";
    import DebugPaste from "$lib/components/DebugPaste.svelte";
    import BuildingColumn from "$lib/components/BuildingColumn.svelte";
    import EmployeeForm from "$lib/components/EmployeeForm.svelte";
    import Office from "$lib/components/Office.svelte";
    import ResizeHandle from "$lib/components/ResizeHandle.svelte";
    import SettingsPanel from "$lib/components/SettingsPanel.svelte";
    import Tabs from "$lib/components/Tabs.svelte";
    import TaskBoard from "$lib/components/TaskBoard.svelte";
    import TerminalArea from "$lib/components/TerminalArea.svelte";
    import Toasts from "$lib/components/Toasts.svelte";
    import { layout } from "$lib/layout.svelte";
    import { store } from "$lib/store.svelte";
    import type { Employee } from "$lib/types";

    // `undefined`: no form. `null`: the form for a new employee.
    let form = $state<Employee | null | undefined>(undefined);

    let work = $state<HTMLElement | null>(null);

    /**
     * Tell the layout the size of the window and of the area that the office and the terminal
     * share: the height of the work area minus the rows around them (toasts, tabs, debug bar).
     */
    function measure(): void {
        if (!work) return;
        let others = 0;
        for (const child of work.children) {
            if (child.matches(".office, .area")) continue;
            const position = getComputedStyle(child).position;
            if (position === "absolute" || position === "fixed") continue;
            others += (child as HTMLElement).offsetHeight;
        }
        layout.measure(window.innerWidth, window.innerHeight, work.clientWidth, work.clientHeight - others);
    }

    onMount(() => {
        void store.init();
        measure();
        const observer = new ResizeObserver(measure);
        if (work) {
            observer.observe(work);
            for (const child of work.children) observer.observe(child);
        }
        return () => observer.disconnect();
    });

    onDestroy(() => store.dispose());
</script>

<svelte:head>
    <title>Agent Tower</title>
</svelte:head>

<svelte:window onresize={measure} />

<div class="app" style:--col-w="{layout.colW}px" style:--board-w="{layout.boardW}px">
    <BuildingColumn />
    <ResizeHandle
        orientation="vertical"
        label="Resize the building column"
        value={layout.colW}
        min={layout.colRange.min}
        max={layout.colRange.max}
        place="left: calc(var(--col-w) - 1px)"
        onchange={(px) => layout.setColumn(px)}
        onreset={() => layout.resetColumn()}
    />
    <main class="work" bind:this={work}>
        <Toasts />
        <Office onedit={(emp) => (form = emp)} onadd={() => (form = null)} />
        <Tabs />
        <TerminalArea />
        <DebugPaste />
    </main>
    <TaskBoard />
</div>

{#if store.settingsOpen}
    <SettingsPanel onclose={() => (store.settingsOpen = false)} />
{/if}

{#if form !== undefined}
    {#key form?.slug ?? "new"}
        <EmployeeForm employee={form} onclose={() => (form = undefined)} />
    {/key}
{/if}

<style>
    .app {
        position: relative;
        display: grid;
        grid-template-columns: var(--col-w) minmax(0, 1fr) var(--board-w);
        height: 100vh;
    }

    .work {
        display: grid;
        grid-template-rows: auto auto auto minmax(0, 1fr) auto;
        min-width: 0;
        min-height: 0;
    }
</style>
