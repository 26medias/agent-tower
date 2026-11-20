<script lang="ts">
    import { onDestroy, onMount } from "svelte";
    import DebugPaste from "$lib/components/DebugPaste.svelte";
    import BuildingColumn from "$lib/components/BuildingColumn.svelte";
    import EmployeeForm from "$lib/components/EmployeeForm.svelte";
    import Office from "$lib/components/Office.svelte";
    import Tabs from "$lib/components/Tabs.svelte";
    import TerminalArea from "$lib/components/TerminalArea.svelte";
    import Toasts from "$lib/components/Toasts.svelte";
    import { store } from "$lib/store.svelte";
    import type { Employee } from "$lib/types";

    // `undefined`: no form. `null`: the form for a new employee.
    let form = $state<Employee | null | undefined>(undefined);

    onMount(() => {
        void store.init();
    });

    onDestroy(() => store.dispose());
</script>

<svelte:head>
    <title>Agent Tower</title>
</svelte:head>

<div class="app">
    <BuildingColumn />
    <main class="work">
        <Toasts />
        <Office onedit={(emp) => (form = emp)} onadd={() => (form = null)} />
        <Tabs />
        <TerminalArea />
        <DebugPaste />
    </main>
</div>

{#if form !== undefined}
    {#key form?.slug ?? "new"}
        <EmployeeForm employee={form} onclose={() => (form = undefined)} />
    {/key}
{/if}

<style>
    .app {
        display: grid;
        grid-template-columns: clamp(260px, 25vw, 360px) minmax(0, 1fr);
        height: 100vh;
    }

    .work {
        display: grid;
        grid-template-rows: auto auto auto minmax(0, 1fr) auto;
        min-width: 0;
        min-height: 0;
    }
</style>
