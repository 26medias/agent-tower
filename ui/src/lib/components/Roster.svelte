<script lang="ts">
    // The list of all employees of the open floor: status, name and job title. A click selects the
    // employee, the same as a click on the character. The text is readable at each office size.
    import { roleColor } from "$lib/office-layout";
    import { store } from "$lib/store.svelte";
    import StatusDot from "./StatusDot.svelte";

    const employees = $derived(store.orderedEmployees);
    const selectedSlug = $derived(store.selectedEmployee?.slug ?? null);
</script>

{#if employees.length > 0}
    <ul class="roster" aria-label="Employees">
        {#each employees as emp (emp.slug)}
            {@const status = store.sessionOfEmployee(emp.slug)?.status ?? "stopped"}
            <li>
                <button
                    type="button"
                    class="row"
                    class:selected={emp.slug === selectedSlug}
                    class:asking={status === "question"}
                    data-status={status}
                    aria-pressed={emp.slug === selectedSlug}
                    data-slug={emp.slug}
                    title="{emp.name}, {emp.role}, {status}"
                    onclick={() => store.selectEmployee(emp.slug)}
                >
                    <StatusDot {status} />
                    <span class="r-name">{emp.name}</span>
                    <span class="r-role" style:color={status === "question" ? "inherit" : roleColor(emp.role)}>{emp.role}</span>
                    {#if status === "question"}<span class="r-asks" data-testid="roster-asks">asks</span>{/if}
                </button>
            </li>
        {/each}
    </ul>
{/if}

<style>
    .roster {
        display: grid;
        /* Two columns in the info panel at 1280 px, more in a wide panel. The role is cut with an ellipsis. */
        grid-template-columns: repeat(auto-fill, minmax(min(100%, 128px), 1fr));
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
    .row.asking {
        background: var(--st-question);
        border-color: #b3822e;
        color: #1a1400;
    }

    .row.asking .r-name {
        color: #1a1400;
    }

    .row.asking.selected {
        border-color: var(--accent);
        box-shadow: 0 0 0 1px var(--accent);
    }

    .row.asking :global(.dot) {
        background: #1a1400;
        border-color: #1a1400;
        animation: none;
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

    .r-role {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
</style>
