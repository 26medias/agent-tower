<script lang="ts">
    import { onDestroy, onMount } from "svelte";
    import DebugPaste from "$lib/components/DebugPaste.svelte";
    import EmployeeCards from "$lib/components/EmployeeCards.svelte";
    import EmployeeForm from "$lib/components/EmployeeForm.svelte";
    import FloorList from "$lib/components/FloorList.svelte";
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
    <FloorList />
    <main class="work">
        <div class="title">
            <h2>{store.floorView ? store.floorView.name : "No floor open"}</h2>
            {#if store.floorView}
                <span class="path">{store.floorView.path}</span>
            {/if}
        </div>
        <Toasts />
        <EmployeeCards onedit={(emp) => (form = emp)} onadd={() => (form = null)} />
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
        grid-template-columns: 240px minmax(0, 1fr);
        height: 100vh;
    }

    .work {
        display: grid;
        grid-template-rows: auto auto auto auto minmax(0, 1fr) auto;
        min-width: 0;
        min-height: 0;
    }

    .title {
        display: flex;
        align-items: baseline;
        gap: 10px;
        padding: 6px 10px 0;
    }

    .title h2 {
        color: var(--accent);
        font-size: 13px;
    }

    .path {
        color: var(--dim);
        font-size: 11px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
</style>
