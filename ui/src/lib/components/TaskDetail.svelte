<script lang="ts">
    // The detail panel of one task: fields, events, the report text, and the controls of the
    // operator (assign, set the status, add a note). It fills the board column.
    import { store } from "$lib/store.svelte";
    import {
        STATUS_CHOICES,
        canDiscard,
        canMerge,
        diffPlaceholder,
        diffText,
        eventText,
        formatTime,
        hasBranch,
        personName,
        shortCommit,
        statusLabel,
        taskLabel,
        worktreePath,
    } from "$lib/tasks";
    import type { TaskMerge, TaskStatus } from "$lib/types";
    import ConfirmButton from "./ConfirmButton.svelte";

    const current = $derived(
        store.taskDetail && store.taskDetail.path === store.openPath && store.taskDetail.detail.task.id === store.openTaskId
            ? store.taskDetail
            : null,
    );
    const task = $derived(current?.detail.task ?? null);
    const events = $derived(current?.detail.events ?? []);
    const names = $derived(Object.fromEntries(store.orderedEmployees.map((e) => [e.slug, e.name])));

    // The form fields follow the task until the operator changes them.
    let assignTo = $state("");
    let statusChoice = $state<TaskStatus>("todo");
    let note = $state("");
    let busy = $state(false);
    let seen = $state<string>("");
    // The git actions (docs/ipc-m3.md). `lastMerge` is the answer of the last merge of this task.
    let gitBusy = $state(false);
    let lastMerge = $state<{ id: number; result: TaskMerge } | null>(null);

    const diffOpen = $derived(
        task !== null && store.taskDiff !== null && store.taskDiff.path === store.openPath && store.taskDiff.id === task.id,
    );
    const diff = $derived(diffOpen ? (store.taskDiff?.diff ?? null) : null);
    const diffError = $derived(diffOpen ? (store.taskDiff?.error ?? null) : null);
    const merged = $derived(task !== null && lastMerge !== null && lastMerge.id === task.id ? lastMerge.result : null);

    async function toggleDiff(): Promise<void> {
        if (!task) return;
        if (diffOpen) store.hideDiff();
        else await store.showDiff(task.id);
    }

    async function merge(): Promise<void> {
        if (!task || gitBusy) return;
        gitBusy = true;
        const id = task.id;
        const result = await store.mergeTask(id);
        gitBusy = false;
        lastMerge = result ? { id, result } : null;
    }

    async function discard(): Promise<void> {
        if (!task || gitBusy) return;
        gitBusy = true;
        await store.discardTask(task.id);
        gitBusy = false;
        lastMerge = null;
    }

    $effect(() => {
        if (!task) return;
        const key = `${task.id}:${task.assignee}:${task.status}`;
        if (key === seen) return;
        seen = key;
        assignTo = task.assignee ?? "";
        statusChoice = task.status;
    });

    async function assign(): Promise<void> {
        if (!task || assignTo === "" || busy) return;
        busy = true;
        await store.assignTask(task.id, assignTo);
        busy = false;
    }

    async function apply(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (!task || busy) return;
        busy = true;
        const done = await store.updateTask(task.id, statusChoice, note);
        busy = false;
        if (done) note = "";
    }
</script>

<div class="detail" aria-label="Task detail">
    <div class="bar">
        <button type="button" class="ghost" aria-label="Back to the list of tasks" onclick={() => store.closeTask()}>
            &lsaquo; Back
        </button>
    </div>
    {#if !task}
        <p class="none">Loading...</p>
    {:else}
        <div class="scroll">
            <div class="title-row">
                <span class="id mono">{taskLabel(task.id)}</span>
                <span class="pill" style:--tk="var(--tk-{task.status})" data-testid="detail-status">{statusLabel(task.status)}</span>
            </div>
            <h3 class="title">{task.title}</h3>
            <dl class="meta">
                <dt>Assigned to</dt>
                <dd data-testid="detail-assignee">{personName(task.assignee, names)}</dd>
                {#if task.assigner}
                    <dt>Given by</dt>
                    <dd>{personName(task.assigner, names)}</dd>
                {/if}
                <dt>Made by</dt>
                <dd>{personName(task.creator, names)}</dd>
                <dt>Made</dt>
                <dd>{formatTime(task.createdAt)}</dd>
                <dt>Changed</dt>
                <dd>{formatTime(task.updatedAt)}</dd>
            </dl>

            {#if hasBranch(task)}
                {@const wt = worktreePath(store.openPath ?? "", task)}
                <h4>Git branch</h4>
                <dl class="meta" data-testid="branch-info">
                    <dt>Branch</dt>
                    <dd class="mono" data-testid="detail-branch">{task.branch}</dd>
                    <dt>Started from</dt>
                    <dd class="mono">{task.baseBranch} at {shortCommit(task.baseCommit)}</dd>
                    <dt>Worktree</dt>
                    <dd class="mono" data-testid="detail-worktree">{wt ?? "removed"}</dd>
                </dl>
                <div class="git-actions">
                    <button type="button" data-testid="show-diff" aria-expanded={diffOpen} onclick={toggleDiff}>
                        {diffOpen ? "Hide diff" : "Show diff"}
                    </button>
                    {#if canMerge(task)}
                        <button
                            type="button"
                            class="primary"
                            data-testid="merge"
                            disabled={gitBusy}
                            title="Merge the branch into {task.baseBranch}. The app removes the worktree and sets the task to done."
                            onclick={merge}
                        >
                            Merge
                        </button>
                    {/if}
                    {#if canDiscard(task)}
                        <ConfirmButton
                            label="Discard"
                            confirmLabel="Discard? Click again"
                            class="discard"
                            title="Remove the worktree and the branch. The work on them is lost."
                            onconfirm={discard}
                        />
                    {/if}
                </div>
                {#if merged}
                    <p class="merge-note" class:blocked={!merged.merged} data-testid="merge-result" data-merged={merged.merged}>
                        {merged.message}
                    </p>
                {/if}
                {#if diffOpen}
                    {#if diffError}
                        <p class="error">{diffError}</p>
                    {:else if diff === null}
                        <p class="dim">Loading the diff...</p>
                    {:else}
                        {@const placeholder = diffPlaceholder(diff)}
                        {#if placeholder}
                            <p class="dim" data-testid="diff-placeholder">{placeholder}</p>
                        {:else}
                            <pre class="diff" aria-label="Diff text" data-testid="diff-box">{diffText(diff)}</pre>
                        {/if}
                    {/if}
                {/if}
            {/if}

            <h4>Description</h4>
            {#if task.description.trim() === ""}
                <p class="dim">No description.</p>
            {:else}
                <p class="text">{task.description}</p>
            {/if}

            <h4>Events</h4>
            <ol class="events" aria-label="Events">
                {#each events as e (e.id)}
                    {@const t = eventText(e, names)}
                    <li>
                        <span class="e-time mono">{formatTime(e.at)}</span>
                        <span class="e-line">{t.line}</span>
                        {#if t.note}<span class="e-note">{t.note}</span>{/if}
                    </li>
                {/each}
            </ol>

            <h4>Report</h4>
            {#if task.report === null}
                <p class="dim">No report yet.</p>
            {:else}
                <p class="dim mono small">{task.report}</p>
                {#if current?.reportError}
                    <p class="error">{current.reportError}</p>
                {:else if current?.report === undefined}
                    <p class="dim">Loading the report...</p>
                {:else if current?.report !== null}
                    <pre class="report" aria-label="Report text">{current.report}</pre>
                {/if}
            {/if}

            <h4>Operator</h4>
            <div class="controls">
                <label class="row">
                    <span>Assign to</span>
                    <select bind:value={assignTo} aria-label="Assign to">
                        <option value="" disabled>Choose an employee</option>
                        {#each store.orderedEmployees as emp (emp.slug)}
                            <option value={emp.slug} title={emp.role}>{emp.name}</option>
                        {/each}
                    </select>
                    <button type="button" disabled={busy || assignTo === "" || assignTo === task.assignee} onclick={assign}>
                        Assign
                    </button>
                </label>
                <form class="status-form" onsubmit={apply}>
                    <label class="row">
                        <span>Status</span>
                        <select bind:value={statusChoice} aria-label="Status">
                            {#each STATUS_CHOICES as s (s)}
                                <option value={s}>{statusLabel(s)}</option>
                            {/each}
                        </select>
                    </label>
                    <textarea rows="2" placeholder="Add a note (optional)" aria-label="Note" bind:value={note}></textarea>
                    <button type="submit" class="primary" disabled={busy || (statusChoice === task.status && note.trim() === "")}>
                        {statusChoice === task.status ? "Add the note" : "Set the status"}
                    </button>
                </form>
            </div>
        </div>
    {/if}
</div>

<style>
    .detail {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
    }

    .bar {
        padding: 4px 6px;
        border-bottom: 1px solid var(--line);
    }

    .scroll {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 8px 10px 14px;
    }

    .none {
        margin: 12px 10px;
        color: var(--dim);
    }

    .title-row {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .id {
        color: var(--dim);
    }

    .pill {
        padding: 1px 8px;
        border-radius: 10px;
        border: 1px solid var(--tk);
        color: var(--tk);
        font-size: 11px;
        font-weight: 700;
    }

    .title {
        margin: 4px 0 8px;
        font-family: var(--sans);
        text-transform: none;
        letter-spacing: 0;
        font-size: 14px;
        overflow-wrap: anywhere;
    }

    h4 {
        margin: 12px 0 4px;
        font-family: var(--display);
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--dim);
    }

    .meta {
        display: grid;
        grid-template-columns: auto 1fr;
        gap: 2px 10px;
        margin: 0;
        font-size: 12px;
    }

    .meta dt {
        color: var(--dim);
    }

    .meta dd {
        margin: 0;
        min-width: 0;
        overflow-wrap: anywhere;
    }

    .text {
        margin: 0;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }

    .dim {
        margin: 0;
        color: var(--dim);
    }

    .small {
        font-size: 11px;
        overflow-wrap: anywhere;
    }

    .events {
        display: flex;
        flex-direction: column;
        gap: 5px;
        margin: 0;
        padding: 0;
        list-style: none;
    }

    .events li {
        display: flex;
        flex-direction: column;
        font-size: 12px;
        padding-left: 8px;
        border-left: 2px solid var(--line);
    }

    .e-time {
        color: var(--dim);
        font-size: 11px;
    }

    .e-note {
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        color: var(--dim);
    }

    .report {
        max-height: 220px;
        margin: 4px 0 0;
        padding: 8px;
        overflow: auto;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        font-family: var(--mono);
        font-size: 12px;
        background: var(--bg-deep);
        border: 1px solid var(--line);
        border-radius: 6px;
    }

    .error {
        margin: 0;
        color: var(--danger);
        overflow-wrap: anywhere;
    }

    .controls {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .row {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .row span {
        flex: none;
        width: 62px;
        color: var(--dim);
    }

    .row select {
        flex: 1;
    }

    .git-actions {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-top: 6px;
    }

    .merge-note {
        margin: 6px 0 0;
        color: var(--ok);
        overflow-wrap: anywhere;
    }

    .merge-note.blocked {
        color: var(--danger);
    }

    /* Plain text, monospace, with a scroll in both directions. */
    .diff {
        max-height: 260px;
        margin: 6px 0 0;
        padding: 8px;
        overflow: auto;
        white-space: pre;
        font-family: var(--mono);
        font-size: 11px;
        background: var(--bg-deep);
        border: 1px solid var(--line);
        border-radius: 6px;
    }

    .status-form {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .status-form textarea {
        resize: vertical;
    }
</style>
