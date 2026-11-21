<script lang="ts">
    import { onDestroy, onMount, tick } from "svelte";
    import DebugPaste from "$lib/components/DebugPaste.svelte";
    import BuildingColumn from "$lib/components/BuildingColumn.svelte";
    import EmployeeForm from "$lib/components/EmployeeForm.svelte";
    import Office from "$lib/components/Office.svelte";
    import ResizeHandle from "$lib/components/ResizeHandle.svelte";
    import SettingsPanel from "$lib/components/SettingsPanel.svelte";
    import Tabs from "$lib/components/Tabs.svelte";
    import TaskBoard from "$lib/components/TaskBoard.svelte";
    import TaskReader from "$lib/components/TaskReader.svelte";
    import TerminalArea from "$lib/components/TerminalArea.svelte";
    import Toasts from "$lib/components/Toasts.svelte";
    import TopStrip from "$lib/components/TopStrip.svelte";
    import { setWindowTitle } from "$lib/ipc";
    import { layout } from "$lib/layout.svelte";
    import { store } from "$lib/store.svelte";
    import type { Employee } from "$lib/types";

    // `undefined`: no form. `null`: the form for a new employee.
    let form = $state<Employee | null | undefined>(undefined);

    let work = $state<HTMLElement | null>(null);

    /**
     * Tell the layout the size of the window and of the area that the office and the terminal
     * share: the height of the work area minus the rows around them (the top strip, tabs, debug bar).
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

    // The debug bar adds or removes a row of the work area. Measure again after it.
    $effect(() => {
        void store.debugVisible;
        void tick().then(measure);
    });

    // The window title shows how many sessions ask a question: "(2) Agent Tower".
    $effect(() => {
        void setWindowTitle(store.windowTitle);
    });

    // Ctrl+Shift+D shows or hides the debug paste bar. The capture phase keeps the key from the
    // terminal: xterm would send it to the CLI as a control character.
    function onKeyCapture(e: KeyboardEvent): void {
        if (e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey && e.key.toLowerCase() === "d") {
            e.preventDefault();
            e.stopPropagation();
            store.setDebugVisible(!store.debugVisible);
        }
    }
</script>

<svelte:head>
    <title>{store.windowTitle}</title>
</svelte:head>

<svelte:window onresize={measure} onkeydowncapture={onKeyCapture} />

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
        <TopStrip />
        <Office onedit={(emp) => (form = emp)} onadd={() => (form = null)} />
        <Tabs />
        <TerminalArea />
        {#if store.debugVisible}
            <DebugPaste />
        {/if}
    </main>
    <TaskBoard />
</div>

<Toasts />

{#if store.reader !== null}
    <TaskReader />
{/if}

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
