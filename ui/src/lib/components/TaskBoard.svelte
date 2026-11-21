<script lang="ts">
    // The task board: a column at the right of the work area. It lists the tasks of the open floor
    // in groups by status. A click on a card opens the detail panel (TaskDetail). The operator can
    // make a task here. The column is as wide as `layout.boardW`. A button collapses it to a strip
    // that shows the number of open tasks. The width has a drag handle, like the other panes.
    import { layout } from "$lib/layout.svelte";
    import { store } from "$lib/store.svelte";
    import { defaultOpenGroups, groupTasks, hasBranch, openCount, personName, statusLabel, taskLabel } from "$lib/tasks";
    import type { TaskStatus } from "$lib/types";
    import ResizeHandle from "./ResizeHandle.svelte";
    import TaskDetail from "./TaskDetail.svelte";

    const collapsed = $derived(layout.boardCollapsed);
    const tasks = $derived(store.tasks);
    const groups = $derived(groupTasks(tasks));
    const open = $derived(openCount(tasks));
    const names = $derived(Object.fromEntries(store.orderedEmployees.map((e) => [e.slug, e.name])));

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
            <span class="open-count" title="Tasks that are not done">{open} open</span>
            <span class="grow"></span>
            {#if store.openTaskId === null}
                <button
                    type="button"
                    class="new"
                    aria-expanded={creating}
                    disabled={store.openPath === null}
                    onclick={() => (creating = !creating)}
                >
                    {creating ? "Cancel" : "+ New"}
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
                        <button type="submit" class="primary" disabled={busy || title.trim() === ""}>Make the task</button>
                    </form>
                {/if}
                {#each groups as group (group.status)}
                    <section class="group" data-status={group.status}>
                        <button
                            type="button"
                            class="group-head ghost"
                            aria-expanded={groupOpen[group.status]}
                            onclick={() => (groupOpen = { ...groupOpen, [group.status]: !groupOpen[group.status] })}
                        >
                            <span class="chev">{groupOpen[group.status] ? "v" : ">"}</span>
                            <span class="dot" style:background="var(--tk-{group.status})"></span>
                            <span class="g-name">{statusLabel(group.status)}</span>
                            <span class="g-count">{group.tasks.length}</span>
                        </button>
                        {#if groupOpen[group.status]}
                            <ul class="cards">
                                {#each group.tasks as t (t.id)}
                                    <li>
                                        <button
                                            type="button"
                                            class="card"
                                            data-task={t.id}
                                            data-status={t.status}
                                            style:--tk="var(--tk-{t.status})"
                                            title="{taskLabel(t.id)}: {t.title}"
                                            onclick={() => store.openTask(t.id)}
                                        >
                                            <span class="c-top">
                                                <span class="c-id mono">{taskLabel(t.id)}</span>
                                                <span class="c-status">{statusLabel(t.status)}</span>
                                            </span>
                                            <span class="c-title">{t.title}</span>
                                            <span class="c-bottom">
                                                <span class="c-who">{personName(t.assignee, names)}</span>
                                                {#if hasBranch(t)}
                                                    <span class="c-branch mono" data-testid="branch-mark" title="Git branch {t.branch}">branch</span>
                                                {/if}
                                            </span>
                                        </button>
                                    </li>
                                {/each}
                            </ul>
                        {/if}
                    </section>
                {:else}
                    <p class="none">No tasks yet. Make one, or ask the CEO.</p>
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
        width: 10px;
        font-family: var(--mono);
        color: var(--dim);
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

    .c-status {
        color: var(--tk);
        font-weight: 700;
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
