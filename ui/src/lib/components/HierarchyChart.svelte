<script lang="ts">
    // The organisation chart of the open floor: the tab "Hierarchy" of the floor view.
    // The tree comes from the `manager` field of each employee (see `buildHierarchy`): the top employees are in the
    // first row, the direct reports of an employee are below it, and lines join them.
    // A node is a card with the name, the job title and the session status (the same colours as the roster). A card
    // turns amber, with a word, when the employee asks a question or has an open request for the operator
    // (docs/ipc-final.md). A click does what a click on the employee in the office does: it selects the employee, and
    // it opens the tab when the employee has a live session.
    // A row of many reports that have no reports of their own would be wider than any window. From 5 of them, they are drawn
    // as one block of cards in a grid under their manager (see `splitReports`). The other reports stay in the row.
    // The chart scrolls inside its area in both directions. It never changes the size of the page. It opens with the first
    // top employee in the middle of the view, so a wide chart does not open on its left edge.
    import { buildHierarchy, groupColumns, splitReports, type HierarchyNode } from "$lib/hierarchy";
    import { roleColor } from "$lib/office-layout";
    import { store } from "$lib/store.svelte";
    import StatusDot from "./StatusDot.svelte";

    const roots = $derived(buildHierarchy(store.floorView?.employees ?? []));
    const selectedSlug = $derived(store.selectedEmployee?.slug ?? null);

    let chartEl = $state<HTMLElement | null>(null);

    // Scroll the first top employee to the middle of the view, when the chart opens and when another floor opens.
    $effect(() => {
        void store.openPath;
        const el = chartEl;
        if (!el) return;
        const frame = requestAnimationFrame(() => {
            const first = el.querySelector(".tree > li > .node");
            if (!first) return;
            const a = first.getBoundingClientRect();
            const b = el.getBoundingClientRect();
            el.scrollLeft += a.left + a.width / 2 - (b.left + b.width / 2);
            el.scrollTop = 0;
        });
        return () => cancelAnimationFrame(frame);
    });
</script>

{#snippet card(node: HierarchyNode)}
    {@const emp = node.employee}
    {@const status = store.sessionOfEmployee(emp.slug)?.status ?? "stopped"}
    {@const request = store.requestOf(emp.slug)}
    {@const asking = status === "question"}
    <button
        type="button"
        class="node"
        class:selected={emp.slug === selectedSlug}
        class:attn={asking || request !== undefined}
        data-testid="h-node"
        data-slug={emp.slug}
        data-status={status}
        data-level={node.level}
        data-needs={request !== undefined ? "true" : "false"}
        aria-pressed={emp.slug === selectedSlug}
        title="{emp.name}, {emp.role}, {status}{request ? `, needs you: ${request.text}` : ''}"
        onclick={() => store.selectEmployee(emp.slug)}
    >
        <span class="n-head">
            <StatusDot {status} />
            <span class="n-name">{emp.name}</span>
        </span>
        <span class="n-role" style:color={roleColor(emp.role)}>{emp.role}</span>
        <span class="n-foot">
            <span class="n-status" style:--st="var(--st-{status})">{status}</span>
            {#if asking}
                <span class="n-chip" data-testid="h-asks">asks</span>
            {/if}
            {#if request}
                <span class="n-chip" data-testid="h-needs">needs you</span>
            {/if}
        </span>
    </button>
{/snippet}

{#snippet branch(node: HierarchyNode)}
    {@const split = splitReports(node.children)}
    <li data-level={node.level}>
        {@render card(node)}
        {#if node.children.length > 0}
            <ul>
                {#each split.nodes as child (child.employee.slug)}
                    {@render branch(child)}
                {/each}
                {#if split.group.length > 0}
                    <li class="group-item" data-level={node.level + 1}>
                        <div class="group" role="group" aria-label="Reports of {node.employee.name} with no reports" data-testid="h-group" style:--cols={groupColumns(split.group.length)}>
                            {#each split.group as leaf (leaf.employee.slug)}
                                {@render card(leaf)}
                            {/each}
                        </div>
                    </li>
                {/if}
            </ul>
        {/if}
    </li>
{/snippet}

<div
    class="chart"
    id="office-panel-hierarchy"
    role="tabpanel"
    aria-labelledby="office-tab-hierarchy"
    data-testid="hierarchy"
    bind:this={chartEl}
    data-count={store.floorView?.employees.length ?? 0}
>
    {#if roots.length === 0}
        <p class="empty">No employees yet. Add one with "+ employee".</p>
    {:else}
        <ul class="tree" aria-label="Organisation chart">
            {#each roots as root (root.employee.slug)}
                {@render branch(root)}
            {/each}
        </ul>
    {/if}
</div>

<style>
    /* The chart fills the area of the floor view and scrolls inside it. The top padding keeps the cards clear of the tab bar. */
    .chart {
        --link: #4b5f8c;
        --gap: 18px;
        position: absolute;
        inset: 0;
        overflow: auto;
        overscroll-behavior: contain;
        padding: 40px 16px 16px;
        background: #0b111c;
    }

    .empty {
        margin: 0;
        padding: 8px 0;
        color: var(--dim);
        text-align: center;
    }

    /* Centred when it is narrower than the area. A wider chart starts at the left edge and scrolls. */
    .tree {
        display: flex;
        align-items: flex-start;
        gap: 28px;
        width: max-content;
        margin: 0 auto;
    }

    .tree,
    .tree ul {
        padding: 0;
        list-style: none;
    }

    .tree li {
        display: flex;
        flex-direction: column;
        align-items: center;
        position: relative;
    }

    /* The row of the reports of one employee, and the line that goes down from the employee to the row. */
    .tree li > ul {
        display: flex;
        justify-content: center;
        position: relative;
        margin: 0;
        padding-top: var(--gap);
    }

    .tree li > ul::before {
        content: "";
        position: absolute;
        top: 0;
        left: 50%;
        height: var(--gap);
        border-left: 2px solid var(--link);
        transform: translateX(-1px);
    }

    .tree li > ul > li {
        padding: var(--gap) 7px 0;
    }

    /* The two halves of the bar over a report, and the line that goes down to it. */
    .tree li > ul > li::before,
    .tree li > ul > li::after {
        content: "";
        position: absolute;
        top: 0;
        right: 50%;
        width: 50%;
        height: var(--gap);
        border-top: 2px solid var(--link);
    }

    .tree li > ul > li::after {
        right: auto;
        left: 50%;
        border-left: 2px solid var(--link);
    }

    /* One report: the vertical line of the row reaches it. No bar. */
    .tree li > ul > li:only-child {
        padding-top: 0;
    }

    .tree li > ul > li:only-child::before,
    .tree li > ul > li:only-child::after {
        display: none;
    }

    .tree li > ul > li:first-child::before,
    .tree li > ul > li:last-child::after {
        border: 0 none;
    }

    .tree li > ul > li:last-child::before {
        border-right: 2px solid var(--link);
        border-radius: 0 6px 0 0;
    }

    .tree li > ul > li:first-child::after {
        border-radius: 6px 0 0 0;
    }

    /* ---- a block of reports with no reports of their own ---- */

    .group {
        display: grid;
        grid-template-columns: repeat(var(--cols), 160px);
        gap: 8px;
        padding: 9px;
        border: 2px solid var(--link);
        border-radius: 10px;
    }

    /* ---- a node ---- */

    .node {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: 2px;
        width: 160px;
        padding: 6px 9px;
        border: 1px solid var(--line);
        border-radius: 8px;
        background: var(--panel);
        color: var(--text);
        text-align: left;
        font-size: 12px;
    }

    .node:hover {
        border-color: var(--accent);
    }

    .node.selected {
        border-color: var(--accent);
        box-shadow: 0 0 0 1px var(--accent);
    }

    .n-head,
    .n-foot {
        display: flex;
        align-items: center;
        gap: 6px;
        min-width: 0;
    }

    .n-name {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-weight: 700;
        font-size: 13px;
    }

    .n-role {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .n-status {
        color: var(--st);
    }

    .n-chip {
        margin-left: auto;
        padding: 0 6px;
        border-radius: 8px;
        background: #1a1400;
        color: var(--st-question);
        font-size: 11px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        white-space: nowrap;
    }

    .n-chip + .n-chip {
        margin-left: 0;
    }

    /* A question or an open request: an amber card, like the amber row of the roster. The dark ink keeps it readable. */
    .node.attn {
        border-color: #b3822e;
        background: var(--st-question);
        color: #1a1400;
    }

    .node.attn .n-role,
    .node.attn .n-status {
        color: #1a1400 !important;
    }

    .node.attn.selected {
        border-color: var(--accent);
        box-shadow: 0 0 0 1px var(--accent);
    }

    .node.attn :global(.dot) {
        background: #1a1400;
        border-color: #1a1400;
        animation: none;
    }
</style>
