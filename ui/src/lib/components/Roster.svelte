<script lang="ts">
    // The list of all employees of the open floor: status, name and job title. A click selects the
    // employee, the same as a click on the character. The text is readable at each office size.
    // The columns are at least 240 px wide when the panel has the room, so a job title is not cut.
    // A row that asks a question keeps the whole name and hides the job title: the amber "ASKS" says what matters.
    // An employee with an open request for the operator (docs/ipc-final.md) gets the same amber row with the word "HELP" (a short word: the columns are narrow).
    // "+ employee" is the last item of the list (it is not an action on the selected employee).
    import { roleColor } from "$lib/office-layout";
    import { store } from "$lib/store.svelte";
    import StatusDot from "./StatusDot.svelte";

    let { onadd }: { onadd: () => void } = $props();

    const employees = $derived(store.orderedEmployees);
    const selectedSlug = $derived(store.selectedEmployee?.slug ?? null);
</script>

{#if store.floorView}
    <ul class="roster" aria-label="Employees">
        {#each employees as emp (emp.slug)}
            {@const status = store.sessionOfEmployee(emp.slug)?.status ?? "stopped"}
            {@const request = store.requestOf(emp.slug)}
            <li>
                <button
                    type="button"
                    class="row"
                    class:selected={emp.slug === selectedSlug}
                    class:asking={status === "question"}
                    class:needs={request !== undefined}
                    data-status={status}
                    aria-pressed={emp.slug === selectedSlug}
                    data-slug={emp.slug}
                    title="{emp.name}, {emp.role}, {status}{request ? `, needs you: ${request.text}` : ''}"
                    onclick={() => store.selectEmployee(emp.slug)}
                >
                    <StatusDot {status} />
                    <span class="r-name">{emp.name}</span>
                    {#if status === "question"}
                        <span class="r-asks" data-testid="roster-asks">asks</span>
                    {:else if request}
                        <span class="r-asks" data-testid="roster-needs" title="Needs you: {request.text}">help</span>
                    {:else}
                        <span class="r-role" style:color={roleColor(emp.role)}>{emp.role}</span>
                    {/if}
                </button>
            </li>
        {/each}
        <li class="add-item">
            <button type="button" class="add" data-testid="add-employee" disabled={store.gone} title={store.goneTitle("Add an employee to this floor")} onclick={onadd}>+ employee</button>
        </li>
    </ul>
{/if}

<style>
    .roster {
        display: grid;
        /* The columns are wide, so a job title is not cut: at least 240 px when the panel is wide enough for two of them
           (at 486 px and more). A narrower panel keeps two compact columns (half the width), and a very narrow one
           (under about 230 px) keeps one. Fewer and wider columns, not many narrow ones. */
        grid-template-columns: repeat(auto-fill, minmax(max(110px, min(240px, calc(50% - 3px))), 1fr));
        gap: 3px 6px;
        margin: 0;
        padding: 0;
        list-style: none;
    }

    .row {
        display: flex;
        align-items: center;
        gap: 6px;
        width: 100%;
        min-width: 0;
        padding: 3px 6px;
        background: transparent;
        border: 1px solid transparent;
        border-radius: 4px;
        text-align: left;
        font-size: 12px;
    }

    .row:hover {
        border-color: var(--line);
    }

    .row.selected {
        border-color: var(--accent);
    }

    /* A session that waits for an answer: an amber row, not only a dot. */
    .row.asking,
    .row.needs {
        background: var(--st-question);
        border-color: #b3822e;
        color: #1a1400;
    }

    .row.asking .r-name,
    .row.needs .r-name {
        color: #1a1400;
    }

    .row.asking.selected,
    .row.needs.selected {
        border-color: var(--accent);
        box-shadow: 0 0 0 1px var(--accent);
    }

    .row.asking :global(.dot),
    .row.needs :global(.dot) {
        background: #1a1400;
        border-color: #1a1400;
        animation: none;
    }

    /* A row that asks: the whole name. It shrinks (with an ellipsis) only when the row is really too narrow. */
    .row.asking .r-name,
    .row.needs .r-name {
        flex: 0 1 auto;
        max-width: none;
        min-width: 0;
    }

    .r-asks {
        flex: none;
        margin-left: auto;
        padding: 0 6px;
        border-radius: 8px;
        background: #1a1400;
        color: var(--st-question);
        font-size: 11px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.06em;
    }

    .r-name {
        flex: none;
        max-width: 55%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--text);
        font-weight: 700;
    }

    .add-item {
        display: flex;
        align-items: center;
    }

    .add {
        padding: 1px 8px;
        border-style: dashed;
        color: var(--dim);
        font-size: 12px;
    }

    .r-role {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
</style>
