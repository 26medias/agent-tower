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
            <li>
                <button
                    type="button"
                    class="row"
                    class:selected={emp.slug === selectedSlug}
                    aria-pressed={emp.slug === selectedSlug}
                    data-slug={emp.slug}
                    title="{emp.name}, {emp.role}"
                    onclick={() => store.selectEmployee(emp.slug)}
                >
                    <StatusDot status={store.sessionOfEmployee(emp.slug)?.status ?? "stopped"} />
                    <span class="r-name">{emp.name}</span>
                    <span class="r-role" style:color={roleColor(emp.role)}>{emp.role}</span>
                </button>
            </li>
        {/each}
    </ul>
{/if}

<style>
    .roster {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
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
