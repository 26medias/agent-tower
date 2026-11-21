<script lang="ts">
    // The list "Team activity" of the open floor, newest first, at most 30 lines. It shows what the UI
    // already knows: a status change of a session, "X started Y", a change of a task, a question, and a message
    // that the app delivered ("Maya → Eli: ..."). Each employee has one current status line: a new status line
    // replaces the old one (see `appendActivity`). Each line has its time. A click on a line goes to the tab of
    // the employee (for a message: the receiver), or to the task.
    // It is calm: small text, no colour except the amber of a question.
    import { activityTime, lineText, visibleActivity, type ActivityEntry } from "$lib/activity";
    import { store } from "$lib/store.svelte";

    const lines = $derived(visibleActivity(store.activity, store.openPath));

    // The name of the employee is read when the line is drawn: a floor can bring its employees after its first event.
    const text = (line: ActivityEntry): string => lineText(line, (path, slug) => store.sessionLabel({ path, slug }));

    function go(line: ActivityEntry): void {
        if (line.kind === "message" && line.slug !== undefined) {
            // The receiver. Its session is live, because the message was delivered to it.
            store.selectEmployee(line.slug);
        } else if (line.taskId !== undefined) {
            store.openTask(line.taskId);
        } else if (line.sessionId !== undefined) {
            void store.openSession(line.sessionId);
        } else if (line.slug !== undefined && store.orderedEmployees.some((e) => e.slug === line.slug)) {
            store.selectEmployee(line.slug);
        }
    }
</script>

<section class="activity" aria-label="Team activity" data-testid="team-activity">
    <h3>Team activity</h3>
    {#if lines.length === 0}
        <p class="empty">Nothing yet. Changes of the team show here.</p>
    {:else}
        <ol>
            {#each lines as line (line.id)}
                <li>
                    <button type="button" class="line" class:question={line.kind === "question"} data-kind={line.kind} title={text(line)} onclick={() => go(line)}>
                        <time datetime={new Date(line.at).toISOString()}>{activityTime(line.at)}</time>
                        <span class="text">{text(line)}</span>
                    </button>
                </li>
            {/each}
        </ol>
    {/if}
</section>

<style>
    .activity {
        display: flex;
        flex-direction: column;
        flex: 1 1 0;
        min-height: 0;
        gap: 4px;
        padding-top: 6px;
        border-top: 1px solid var(--line);
    }

    h3 {
        flex: none;
        color: var(--dim);
        font-size: 11px;
        letter-spacing: 0.1em;
    }

    .empty {
        margin: 0;
        color: var(--dim);
        font-size: 11px;
    }

    ol {
        flex: 1;
        min-height: 0;
        margin: 0;
        padding: 0;
        overflow-y: auto;
        list-style: none;
    }

    .line {
        display: flex;
        gap: 8px;
        width: 100%;
        padding: 1px 4px;
        border: 0;
        border-radius: 3px;
        background: transparent;
        color: var(--dim);
        font-size: 12px;
        text-align: left;
    }

    .line:hover:not(:disabled) {
        background: var(--panel);
        color: var(--text);
    }

    time {
        flex: none;
        font-family: var(--mono);
        opacity: 0.75;
    }

    .text {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* The one colour: a question. */
    .line.question {
        color: var(--st-question);
    }
</style>
