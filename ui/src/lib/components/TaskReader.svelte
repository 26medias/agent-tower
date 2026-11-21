<script lang="ts">
    // A large overlay to read the report or the diff of the open task. The task column is only
    // 260 px wide: a long report or a patch is hard to read there.
    import { modal } from "$lib/modal";
    import { store } from "$lib/store.svelte";
    import { diffPlaceholder, diffText, taskLabel } from "$lib/tasks";

    const kind = $derived(store.reader);
    const detail = $derived(
        store.taskDetail && store.taskDetail.path === store.openPath && store.taskDetail.detail.task.id === store.openTaskId
            ? store.taskDetail
            : null,
    );
    const task = $derived(detail?.detail.task ?? null);

    const diffState = $derived(
        store.taskDiff && store.taskDiff.path === store.openPath && task && store.taskDiff.id === task.id ? store.taskDiff : null,
    );
    const diff = $derived(diffState?.diff ?? null);
    const placeholder = $derived(diff ? diffPlaceholder(diff) : null);
    const lines = $derived(diff && !placeholder ? diffText(diff).split("\n") : []);

    function lineClass(line: string): string {
        if (line.startsWith("@@")) return "hunk";
        if (line.startsWith("+++") || line.startsWith("---") || line.startsWith("diff ") || line.startsWith("index ")) return "meta";
        if (line.startsWith("+")) return "add";
        if (line.startsWith("-")) return "del";
        return "";
    }

    const title = $derived(
        task ? `${kind === "diff" ? "Diff" : "Report"} of ${taskLabel(task.id)}: ${task.title}` : kind === "diff" ? "Diff" : "Report",
    );
</script>

<div class="backdrop" role="presentation" use:modal={{ onclose: () => store.closeReader() }} onclick={(e) => e.target === e.currentTarget && store.closeReader()}>
    <div class="panel" role="dialog" aria-modal="true" aria-label={title} data-testid="reader" data-kind={kind}>
        <header>
            <h2>{title}</h2>
            {#if kind === "diff" && diff?.branch}
                <span class="branch mono">{diff.branch}{diff.baseBranch ? ` -> ${diff.baseBranch}` : ""}</span>
            {/if}
            <span class="grow"></span>
            <button type="button" class="close" data-autofocus onclick={() => store.closeReader()}>Close</button>
        </header>

        <div class="body">
            {#if kind === "report"}
                {#if detail?.reportError}
                    <p class="error">{detail.reportError}</p>
                {:else if !detail || detail.report === undefined}
                    <p class="dim">Loading the report...</p>
                {:else if detail.report === null}
                    <p class="dim">No report yet.</p>
                {:else}
                    <pre class="report" aria-label="Report text" data-testid="report-text">{detail.report}</pre>
                {/if}
            {:else if kind === "diff"}
                {#if diffState?.error}
                    <p class="error">{diffState.error}</p>
                {:else if diff === null}
                    <p class="dim">Loading the diff...</p>
                {:else if placeholder}
                    <p class="dim" data-testid="diff-placeholder">{placeholder}</p>
                {:else}
                    <pre class="diff" aria-label="Diff text" data-testid="diff-box">{#each lines as line, i (i)}<span class="l {lineClass(line)}">{line}{"\n"}</span>{/each}</pre>
                {/if}
            {/if}
        </div>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        z-index: 40;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(6, 10, 18, 0.75);
    }

    .panel {
        display: flex;
        flex-direction: column;
        width: min(1100px, 94vw);
        min-height: min(50vh, 480px);
        max-height: min(86vh, 1000px);
        background: var(--panel);
        border: 1px solid var(--accent);
        border-radius: 8px;
        box-shadow: 0 8px 28px rgba(0, 0, 0, 0.6);
    }

    header {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 10px 14px;
        border-bottom: 1px solid var(--line);
    }

    h2 {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--accent);
        font-size: 14px;
        text-transform: none;
        letter-spacing: 0.02em;
    }

    .branch {
        color: var(--dim);
        font-size: 12px;
    }

    .grow {
        flex: 1;
    }

    .body {
        flex: 1;
        min-height: 0;
        overflow: auto;
        padding: 12px 16px 16px;
        background: var(--bg-deep);
        border-radius: 0 0 8px 8px;
    }

    pre {
        margin: 0;
        font-family: var(--mono);
        font-size: 13px;
        line-height: 1.5;
    }

    .report {
        max-width: 100ch;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }

    .diff {
        white-space: pre;
        width: max-content;
        min-width: 100%;
    }

    .l {
        display: inline;
    }

    .add {
        color: #7cf0a6;
        background: rgba(74, 222, 128, 0.1);
    }

    .del {
        color: #ff8fa0;
        background: rgba(255, 107, 129, 0.1);
    }

    .hunk {
        color: #7dbcff;
    }

    .meta {
        color: var(--dim);
    }

    .dim {
        margin: 0;
        color: var(--dim);
    }

    .error {
        margin: 0;
        color: var(--danger);
        overflow-wrap: anywhere;
    }
</style>
