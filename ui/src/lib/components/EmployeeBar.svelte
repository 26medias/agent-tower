<script lang="ts">
    // The bar for the selected employee: name, role, CLI, model, and the buttons of the old cards.
    import { store } from "$lib/store.svelte";
    import type { Employee } from "$lib/types";
    import ConfirmButton from "./ConfirmButton.svelte";
    import StatusDot from "./StatusDot.svelte";

    let { onedit, onadd }: { onedit: (employee: Employee) => void; onadd: () => void } = $props();

    const emp = $derived(store.selectedEmployee);
    const session = $derived(emp ? store.sessionOfEmployee(emp.slug) : undefined);
    const live = $derived(store.isLive(session));
    const busy = $derived(emp ? store.isBusy(emp.slug) : false);
</script>

<div class="bar" aria-label="Selected employee" data-slug={emp?.slug ?? ""}>
    {#if emp}
        <div class="who">
            <StatusDot status={session?.status ?? "stopped"} />
            <span class="name">{emp.name}</span>
            <span class="role">{emp.role}</span>
        </div>
        <div class="meta">
            <span class="cli {emp.cli}">{emp.cli}</span>
            <span class="model">{emp.model ?? "default model"}</span>
            <span class="state">{session?.status ?? "no session"}</span>
        </div>
        <div class="actions">
            {#if live && session}
                <button type="button" class="danger stop" onclick={() => store.stopSession(session.sessionId)}>stop</button>
            {:else}
                <button type="button" class="primary start" disabled={busy} onclick={() => store.startSession(emp.slug)}>start</button>
                {#if session?.canResume}
                    <button type="button" class="resume" disabled={busy} onclick={() => store.resumeSession(emp.slug)}>resume</button>
                {/if}
            {/if}
            <button type="button" class="edit" title="Edit {emp.name}" aria-label="Edit {emp.name}" onclick={() => onedit(emp)}>edit</button>
            <ConfirmButton
                class="remove"
                label="remove"
                confirmLabel="sure?"
                title="Remove {emp.name}"
                onconfirm={() => store.removeEmployee(emp.slug)}
            />
        </div>
    {:else}
        <p class="hint">{store.floorView ? "Click a character to select an employee." : "Open a floor to see its employees."}</p>
    {/if}
    {#if store.floorView}
        <button type="button" class="add" onclick={onadd}>+ employee</button>
    {/if}
</div>

<style>
    .bar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px 10px;
        padding: 6px 8px;
        background: var(--bg-deep);
        border: 1px solid var(--line);
        border-radius: 8px;
    }

    .who {
        display: flex;
        align-items: center;
        gap: 6px;
        min-width: 0;
    }

    .name {
        font-weight: 700;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .role,
    .state,
    .model {
        color: var(--dim);
        font-size: 12px;
    }

    .meta {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .cli {
        padding: 0 4px;
        border: 1px solid currentColor;
        border-radius: 4px;
        font-size: 11px;
        text-transform: uppercase;
    }

    .cli.claude {
        color: #ff9a62;
    }

    .cli.codex {
        color: #7dbcff;
    }

    .actions {
        display: flex;
        gap: 4px;
        align-items: center;
    }

    .add {
        margin-left: auto;
        border-style: dashed;
        color: var(--dim);
    }

    .hint {
        margin: 0;
        color: var(--dim);
    }
</style>
