<script lang="ts">
    import { store } from "$lib/store.svelte";
    import type { Employee } from "$lib/types";
    import ConfirmButton from "./ConfirmButton.svelte";
    import StatusDot from "./StatusDot.svelte";

    let { onedit, onadd }: { onedit: (employee: Employee) => void; onadd: () => void } = $props();
</script>

<div class="wrap">
<section class="cards" aria-label="Employees">
    {#if store.floorView}
        {#each store.floorView.employees as emp (emp.slug)}
            {@const session = store.sessionOfEmployee(emp.slug)}
            {@const live = store.isLive(session)}
            <article class="card" class:live class:active={session && session.sessionId === store.activeId} data-slug={emp.slug}>
                <button
                    type="button"
                    class="main"
                    disabled={!live}
                    title={live ? "Show the terminal of this employee" : "No live session"}
                    onclick={() => session && store.selectTab(session.sessionId)}
                >
                    <span class="top">
                        <StatusDot status={session?.status ?? "stopped"} />
                        <span class="name">{emp.name}</span>
                    </span>
                    <span class="role">{emp.role}</span>
                    <span class="meta">
                        <span class="cli {emp.cli}">{emp.cli}</span>
                        <span class="state">{session?.status ?? "no session"}</span>
                    </span>
                </button>
                <div class="actions">
                    {#if live && session}
                        <button type="button" class="danger" onclick={() => store.stopSession(session.sessionId)}>stop</button>
                    {:else}
                        <button type="button" class="primary" onclick={() => store.startSession(emp.slug)}>start</button>
                        {#if session?.canResume}
                            <button type="button" onclick={() => store.resumeSession(emp.slug)}>resume</button>
                        {/if}
                    {/if}
                    <span class="spacer"></span>
                    <button type="button" class="ghost" title="Edit {emp.name}" aria-label="Edit {emp.name}" onclick={() => onedit(emp)}>
                        edit
                    </button>
                    <ConfirmButton
                        class="ghost"
                        label="del"
                        confirmLabel="sure?"
                        title="Remove {emp.name}"
                        onconfirm={() => store.removeEmployee(emp.slug)}
                    />
                </div>
            </article>
        {/each}
        <button type="button" class="add" onclick={onadd}>+ employee</button>
    {:else}
        <p class="hint">Open a floor on the left to see its employees.</p>
    {/if}
</section>
{#if store.floorView && store.floorView.employeeErrors.length > 0}
    <ul class="errors" aria-label="Employee files that did not load">
        {#each store.floorView.employeeErrors as err (err)}
            <li>{err}</li>
        {/each}
    </ul>
{/if}
</div>

<style>
    .wrap {
        border-bottom: 2px solid var(--line);
        background: var(--bg);
        min-width: 0;
    }

    .cards {
        display: flex;
        gap: 8px;
        padding: 8px;
        overflow-x: auto;
    }

    .card {
        flex: none;
        width: 214px;
        display: flex;
        flex-direction: column;
        background: var(--panel);
        border: 2px solid var(--line);
        border-radius: 2px;
        box-shadow: 3px 3px 0 rgba(0, 0, 0, 0.45);
    }

    .card.live {
        border-color: #4a4470;
    }

    .card.active {
        border-color: var(--accent);
    }

    .main {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 2px;
        padding: 5px 7px;
        background: transparent;
        border: 0;
        text-align: left;
        text-transform: none;
    }

    .main:disabled {
        opacity: 1;
        cursor: default;
    }

    .top {
        display: flex;
        align-items: center;
        gap: 6px;
        max-width: 100%;
    }

    .name {
        font-weight: 700;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .role {
        color: var(--dim);
        font-size: 12px;
    }

    .meta {
        display: flex;
        gap: 6px;
        font-size: 11px;
    }

    .cli {
        padding: 0 4px;
        border: 1px solid currentColor;
        text-transform: uppercase;
    }

    .cli.claude {
        color: #ff9a62;
    }

    .cli.codex {
        color: #7dbcff;
    }

    .state {
        color: var(--dim);
    }

    .actions {
        display: flex;
        gap: 4px;
        padding: 0 6px 6px;
        align-items: center;
    }

    .actions button {
        padding: 0 6px;
        font-size: 12px;
    }

    .spacer {
        flex: 1;
    }

    .add {
        flex: none;
        align-self: stretch;
        width: 84px;
        border-style: dashed;
        color: var(--dim);
    }

    .hint {
        margin: 0;
        color: var(--dim);
    }

    .errors {
        margin: 0;
        padding: 4px 8px 4px 24px;
        background: #2a1620;
        color: var(--danger);
        font-size: 12px;
    }
</style>
