<script lang="ts">
    // The task board: a column at the right of the work area. It lists the tasks of the open floor
    // in groups by status. A click on a card opens the detail panel (TaskDetail). The operator can
    // make a task here. The column is as wide as `layout.boardW`. A button collapses it to a strip
    // that shows the number of open tasks. The width has a drag handle, like the other panes.
    import { layout } from "$lib/layout.svelte";
    import { store } from "$lib/store.svelte";
    import {
        NO_FILTER,
        UNASSIGNED,
        defaultOpenGroups,
        filterTasks,
        groupTasks,
        hasBranch,
        isFiltering,
        openCount,
        personName,
        splitDone,
        statusLabel,
        taskLabel,
        type TaskFilter,
    } from "$lib/tasks";
    import type { TaskStatus } from "$lib/types";
    import ResizeHandle from "./ResizeHandle.svelte";
    import TaskDetail from "./TaskDetail.svelte";

    const collapsed = $derived(layout.boardCollapsed);
    const tasks = $derived(store.tasks);
    const open = $derived(openCount(tasks));
    const names = $derived(Object.fromEntries(store.orderedEmployees.map((e) => [e.slug, e.name])));

    // The filter: by assignee and by a text. The groups, and the 3 newest done tasks, come from the tasks that pass it.
    let filter = $state<TaskFilter>({ ...NO_FILTER });
    const filtering = $derived(isFiltering(filter));
    const shownTasks = $derived(filterTasks(tasks, filter, names));
    const groups = $derived(groupTasks(shownTasks));

    // Compact rows: one line for each task. The operator's choice is kept.
    const COMPACT_KEY = "agent-tower:board-compact";
    let compact = $state(readCompact());

    function readCompact(): boolean {
        try {
            return localStorage.getItem(COMPACT_KEY) === "1";
        } catch {
            return false;
        }
    }

    function setCompact(on: boolean): void {
        compact = on;
        try {
            localStorage.setItem(COMPACT_KEY, on ? "1" : "0");
        } catch {
            // Storage can be blocked. The choice works for this run only.
        }
    }

    // A narrow board (under 240 px) keeps the header on one line: the words go, the numbers stay.
    const narrow = $derived(layout.boardOpenW < 240);

    /** The cards of a group. The done group always shows its 3 newest tasks, and the older ones when it is open. */
    function cardsOf(status: TaskStatus, list: typeof tasks): typeof tasks {
        if (status !== "done") return list;
        const { recent, older } = splitDone(list);
        return groupOpen.done ? [...recent, ...older] : recent;
    }

    let groupOpen = $state<Record<TaskStatus, boolean>>(defaultOpenGroups());
    let creating = $state(false);
    let title = $state("");
    let description = $state("");
    let busy = $state(false);

    async function create(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (busy || title.trim() === "") return;
        busy = true;
        const made = await store.createTask(title, description);
        busy = false;
        if (made) {
            title = "";
            description = "";
            creating = false;
            store.openTask(made.id);
        }
    }
</script>

<aside class="board" class:collapsed aria-label="Task board" data-collapsed={collapsed}>
    {#if collapsed}
        <button
            type="button"
            class="strip"
            title="Open the task board ({open} open)"
            aria-label="Open the task board. {open} open tasks."
            onclick={() => layout.setBoardCollapsed(false)}
        >
            <span class="count" data-testid="board-count">{open}</span>
            <span class="vlabel">Tasks</span>
        </button>
    {:else}
        <ResizeHandle
            orientation="vertical"
            label="Resize the task board"
            value={layout.boardOpenW}
            min={layout.boardRangeNow.min}
            max={layout.boardRangeNow.max}
            place="left: -3px"
            reverse
            onchange={(px) => layout.setBoard(px)}
            onreset={() => layout.resetBoard()}
        />
        <header class="head">
            <h2>Tasks</h2>
            <span class="open-count" title="{open} tasks are not done" data-testid="open-count">{narrow ? open : `${open} open`}</span>
            <span class="grow"></span>
            {#if store.openTaskId === null}
                <button
                    type="button"
                    class="new"
                    aria-expanded={creating}
                    disabled={store.openPath === null}
                    aria-label={narrow ? (creating ? "Cancel the new task" : "Make a new task") : undefined}
                    title={creating ? "Cancel" : "Make a new task"}
                    onclick={() => (creating = !creating)}
                >
                    {creating ? "Cancel" : narrow ? "+" : "+ New"}
                </button>
            {/if}
            <button
                type="button"
                class="ghost collapse"
                title="Collapse the task board"
                aria-label="Collapse the task board"
                onclick={() => layout.setBoardCollapsed(true)}
            >
                &raquo;
            </button>
        </header>

        {#if store.openPath === null}
            <p class="none">No floor is open.</p>
        {:else if store.openTaskId !== null}
            <TaskDetail />
        {:else}
            <div class="body">
                {#if creating}
                    <form class="new-form" onsubmit={create} aria-label="New task">
                        <input
                            type="text"
                            placeholder="Title"
                            aria-label="Title of the new task"
                            maxlength="200"
                            bind:value={title}
                        />
                        <textarea
                            rows="3"
                            placeholder="What must be done?"
                            aria-label="Description of the new task"
                            bind:value={description}
                        ></textarea>
                        <div class="new-actions">
                            {#if title.trim() === ""}
                                <span class="reason" data-testid="new-task-reason">Give the task a title.</span>
                            {/if}
                            <button type="submit" class="primary" disabled={busy || title.trim() === ""}>Make the task</button>
                        </div>
                    </form>
                {/if}
                {#if tasks.length > 0}
                    <div class="filter" role="search" aria-label="Filter the tasks" data-testid="board-filter">
                        <select aria-label="Show the tasks of" data-testid="filter-assignee" bind:value={filter.assignee}>
                            <option value="">Everyone</option>
                            <option value={UNASSIGNED}>Unassigned</option>
                            {#each store.orderedEmployees as emp (emp.slug)}
                                <option value={emp.slug}>{emp.name}</option>
                            {/each}
                        </select>
                        <input
                            type="search"
                            placeholder="Search tasks"
                            aria-label="Search the tasks"
                            data-testid="filter-query"
                            autocomplete="off"
                            bind:value={filter.query}
                        />
                        <label class="compact" title="One line for each task">
                            <input type="checkbox" data-testid="filter-compact" checked={compact} onchange={(e) => setCompact(e.currentTarget.checked)} />
                            <span>Compact</span>
                        </label>
                        {#if filtering}
                            <p class="filter-note" data-testid="filter-note">
                                {shownTasks.length} of {tasks.length} tasks.
                                <button type="button" class="link" onclick={() => (filter = { ...NO_FILTER })}>Clear the filter</button>
                            </p>
                        {/if}
                    </div>
                {/if}
                {#each groups as group (group.status)}
                    {@const isDone = group.status === "done"}
                    {@const older = isDone ? splitDone(group.tasks).older.length : 0}
                    <section class="group" data-status={group.status}>
                        <button
                            type="button"
                            class="group-head ghost"
                            aria-expanded={isDone && older === 0 ? undefined : groupOpen[group.status]}
                            onclick={() => (groupOpen = { ...groupOpen, [group.status]: !groupOpen[group.status] })}
                        >
                            <svg class="chev" class:down={isDone ? older === 0 || groupOpen.done : groupOpen[group.status]} viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
                                <path d="M3 1.5 L7 5 L3 8.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
                            </svg>
                            <span class="dot" style:background="var(--tk-{group.status})"></span>
                            <span class="g-name">{statusLabel(group.status)}</span>
                            <span class="g-count">{group.tasks.length}</span>
                        </button>
                        {#if isDone || groupOpen[group.status]}
                            <ul class="cards" class:compact>
                                {#each cardsOf(group.status, group.tasks) as t (t.id)}
                                    <li>
                                        <button
                                            type="button"
                                            class="card"
                                            class:compact
                                            data-task={t.id}
                                            data-status={t.status}
                                            style:--tk="var(--tk-{t.status})"
                                            title="{taskLabel(t.id)}: {t.title}"
                                            onclick={() => store.openTask(t.id)}
                                        >
                                            {#if compact}
                                                <span class="c-id mono">{taskLabel(t.id)}</span>
                                                <span class="c-title one">{t.title}</span>
                                                <span class="c-who">{personName(t.assignee, names)}</span>
                                            {:else}
                                                <span class="c-top">
                                                    <span class="c-id mono">{taskLabel(t.id)}</span>
                                                </span>
                                                <span class="c-title">{t.title}</span>
                                                <span class="c-bottom">
                                                    <span class="c-who">{personName(t.assignee, names)}</span>
                                                    {#if hasBranch(t)}
                                                        <span class="c-branch mono" data-testid="branch-mark" title="Git branch {t.branch}">branch</span>
                                                    {/if}
                                                </span>
                                            {/if}
                                        </button>
                                    </li>
                                {/each}
                            </ul>
                            {#if isDone && older > 0}
                                <button
                                    type="button"
                                    class="more ghost"
                                    data-testid="done-older"
                                    onclick={() => (groupOpen = { ...groupOpen, done: !groupOpen.done })}
                                >
                                    {groupOpen.done ? "Fold the older done tasks" : `Show ${older} older done ${older === 1 ? "task" : "tasks"}`}
                                </button>
                            {/if}
                        {/if}
                    </section>
                {:else}
                    {#if filtering}
                        <p class="none">No task matches the filter.</p>
                    {:else}
                        <p class="none">No tasks yet. Make one, or ask the CEO.</p>
                    {/if}
                {/each}
            </div>
        {/if}
    {/if}
</aside>

<style>
    .board {
        position: relative;
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        border-left: 1px solid var(--line);
        background: var(--bg);
    }

    .strip {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 10px;
        width: 100%;
        height: 100%;
        padding: 10px 0;
        border: none;
        border-radius: 0;
        background: transparent;
        color: var(--dim);
    }

    .strip:hover {
        color: var(--text);
    }

    .count {
        min-width: 22px;
        padding: 1px 6px;
        border-radius: 10px;
        background: var(--panel-2);
        border: 1px solid var(--line);
        color: var(--text);
        font-weight: 700;
        text-align: center;
    }

    .vlabel {
        writing-mode: vertical-rl;
        font-family: var(--display);
        text-transform: uppercase;
        letter-spacing: 0.12em;
        font-size: 12px;
    }

    .head {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 8px 8px 6px 10px;
        border-bottom: 1px solid var(--line);
        white-space: nowrap;
    }

    .head > * {
        flex: none;
    }

    .head > .grow {
        flex: 1 1 0;
        min-width: 0;
    }

    .head h2 {
        font-size: 13px;
    }

    .open-count {
        color: var(--dim);
        font-size: 12px;
    }

    .grow {
        flex: 1;
    }

    .body {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 8px;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .none {
        margin: 12px 10px;
        color: var(--dim);
    }

    .new-form {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 8px;
        border: 1px solid var(--line);
        border-radius: 8px;
        background: var(--panel);
    }

    .new-form textarea {
        resize: vertical;
    }

    .group-head {
        display: flex;
        align-items: center;
        gap: 6px;
        width: 100%;
        padding: 3px 4px;
        text-align: left;
    }

    .chev {
        flex: none;
        color: var(--dim);
        transition: transform 0.12s;
    }

    .chev.down {
        transform: rotate(90deg);
    }

    .new-actions {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
    }

    .reason {
        margin-right: auto;
        color: var(--dim);
        font-size: 12px;
    }

    .filter {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 4px 6px;
    }

    .filter select {
        flex: 1 1 90px;
    }

    .filter input[type="search"] {
        flex: 2 1 110px;
    }

    .compact {
        display: flex;
        align-items: center;
        gap: 4px;
        color: var(--dim);
        font-size: 11px;
    }

    .filter-note {
        flex: 1 1 100%;
        margin: 0;
        color: var(--dim);
        font-size: 12px;
    }

    .link {
        padding: 0;
        border: 0;
        background: transparent;
        color: var(--accent);
        text-decoration: underline;
    }

    .more {
        width: 100%;
        margin-top: 4px;
        font-size: 12px;
    }

    .dot {
        width: 8px;
        height: 8px;
        border-radius: 50%;
    }

    .g-name {
        font-weight: 700;
        color: var(--text);
    }

    .g-count {
        margin-left: auto;
        color: var(--dim);
    }

    .cards {
        display: flex;
        flex-direction: column;
        gap: 5px;
        margin: 3px 0 0;
        padding: 0;
        list-style: none;
    }

    .card {
        display: flex;
        flex-direction: column;
        gap: 2px;
        width: 100%;
        padding: 6px 8px 6px 10px;
        text-align: left;
        background: var(--panel);
        border-left: 4px solid var(--tk);
    }

    .c-top {
        display: flex;
        justify-content: space-between;
        gap: 6px;
        font-size: 11px;
    }

    .c-id {
        color: var(--dim);
    }

    /* Compact rows: id, title and name on one line. */
    .card.compact {
        flex-direction: row;
        align-items: center;
        gap: 6px;
        padding: 2px 6px 2px 8px;
    }

    .card.compact .c-title.one {
        flex: 1;
        min-width: 0;
        display: block;
        white-space: nowrap;
        text-overflow: ellipsis;
    }

    .card.compact .c-who {
        flex: none;
        max-width: 70px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .cards.compact {
        gap: 2px;
    }

    .c-title {
        overflow: hidden;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow-wrap: anywhere;
    }

    .c-bottom {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 6px;
    }

    .c-who {
        color: var(--dim);
        font-size: 12px;
    }

    /* A small mark: the task has its own git branch and worktree. */
    .c-branch {
        flex: none;
        padding: 0 5px;
        border: 1px solid var(--line);
        border-radius: 8px;
        color: var(--dim);
        font-size: 10px;
        line-height: 14px;
    }
</style>
