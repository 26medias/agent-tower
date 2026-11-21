<script lang="ts">
    // The bar for the selected employee: name, role, CLI, model, and the buttons of the old cards.
    // The selected employee and the active tab are the same: a click on a tab selects its employee.
    // For the tab of the setup agent, the bar shows the setup session and its own "Stop the setup".
    import { modelDisplay } from "$lib/models";
    import { store } from "$lib/store.svelte";
    import type { Employee } from "$lib/types";
    import ConfirmButton from "./ConfirmButton.svelte";
    import Spinner from "./Spinner.svelte";
    import StatusDot from "./StatusDot.svelte";

    let { onedit, onadd }: { onedit: (employee: Employee) => void; onadd: () => void } = $props();

    const emp = $derived(store.selectedEmployee);
    const session = $derived(emp ? store.sessionOfEmployee(emp.slug) : undefined);
    const live = $derived(store.isLive(session));
    // Which button was clicked, while its command runs. It shows a busy state until the command returns.
    const busyKind = $derived(emp ? store.busyKind(emp.slug) : null);
    const busy = $derived(busyKind !== null);
    const setup = $derived(store.selectedSetup);
    const setupLive = $derived(store.isLive(setup ?? undefined));
    let stopping = $state(false);

    // The model that the session will get: its own, the floor default, or the CLI default.
    const modelText = $derived(emp ? modelDisplay(emp, store.floorView?.defaults) : "");

    async function stopSetup(): Promise<void> {
        if (!setup || stopping) return;
        stopping = true;
        await store.stopSession(setup.sessionId);
        stopping = false;
    }
</script>

<div class="bar" aria-label="Selected employee" data-slug={emp?.slug ?? (setup ? "setup" : "")}>
    {#if setup && !emp}
        <div class="who">
            <StatusDot status={setup.status} />
            <span class="name">Setup agent</span>
            <span class="role">reads the project and proposes a team</span>
        </div>
        <div class="meta">
            <span class="state">{setup.status}</span>
        </div>
        <div class="actions">
            {#if setupLive}
                <button type="button" class="danger stop-session" data-testid="stop-setup-bar" disabled={stopping} onclick={stopSetup}>Stop the setup</button>
            {:else}
                <span class="state">The setup is stopped.</span>
            {/if}
        </div>
    {:else if emp}
        <div class="who">
            <StatusDot status={session?.status ?? "stopped"} />
            <span class="name">{emp.name}</span>
            <span class="role">{emp.role}</span>
        </div>
        <div class="meta">
            <span class="cli {emp.cli}">{emp.cli}</span>
            <span class="model" data-testid="bar-model">{modelText}</span>
            <span class="state">{session?.status ?? "no session"}</span>
        </div>
        <div class="actions">
            {#if live && session}
                {#if session.status === "working"}
                    <!-- A working session loses its turn when it stops: ask first. -->
                    <ConfirmButton
                        class="danger stop"
                        label="stop"
                        confirmLabel="{emp.name} is working. Stop?"
                        title="Stop {emp.name}. The session is working now."
                        onconfirm={() => store.stopSession(session.sessionId)}
                    />
                {:else}
                    <button type="button" class="danger stop" onclick={() => store.stopSession(session.sessionId)}>stop</button>
                {/if}
            {:else}
                <button
                    type="button"
                    class="primary start"
                    class:busy={busyKind === "start"}
                    disabled={busy}
                    aria-busy={busyKind === "start"}
                    onclick={() => store.startSession(emp.slug)}
                >
                    {#if busyKind === "start"}<Spinner size={11} /> starting...{:else}start{/if}
                </button>
                {#if session?.canResume}
                    <button
                        type="button"
                        class="resume"
                        class:busy={busyKind === "resume"}
                        disabled={busy}
                        aria-busy={busyKind === "resume"}
                        onclick={() => store.resumeSession(emp.slug)}
                    >
                        {#if busyKind === "resume"}<Spinner size={11} /> resuming...{:else}resume{/if}
                    </button>
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
        <p class="hint">{store.floorView ? "Click a character or a tab to select an employee." : "Open a floor to see its employees."}</p>
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

    .actions button.busy {
        display: inline-flex;
        align-items: center;
        gap: 5px;
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
