<script lang="ts">
    // The attention bar. It shows above the office while one or more sessions of ANY known floor
    // ask a question (a permission question). It takes its own row, so it covers nothing. It is not
    // a toast: it does not go away by itself, only when no session asks any more.
    // "Open" opens the floor of the session when it is not the open one, makes its tab the active
    // tab, selects the employee and puts the keyboard focus in the terminal. With several questions,
    // each click opens the next one, the oldest first.
    import { store } from "$lib/store.svelte";

    const questions = $derived(store.questions);
    const next = $derived(store.nextQuestion);

    function where(q: { path: string; floorName: string }): string {
        return q.path === store.openPath ? "" : ` on ${q.floorName}`;
    }
</script>

<!-- The wrapper is always there, so that the page can measure it. It has no height when nothing asks. -->
<div class="wrap" aria-live="polite">
    {#if questions.length > 0 && next}
        <div class="attention" role="status" data-testid="attention-bar" data-count={questions.length}>
            <span class="mark" aria-hidden="true">?</span>
            {#if questions.length === 1}
                <span class="text"><strong>{next.label}</strong> asks a question{where(next)}</span>
            {:else}
                <span class="text">
                    <strong>{questions.length} agents</strong> ask a question.
                    <span class="next" data-testid="attention-next">Next: <strong>{next.label}</strong>{where(next)}</span>
                </span>
            {/if}
            <button type="button" class="open" data-testid="attention-open" onclick={() => void store.openQuestion(next.sessionId)}>Open</button>
        </div>
    {/if}
</div>

<style>
    .attention {
        display: flex;
        align-items: center;
        gap: 10px;
        min-height: 34px;
        padding: 4px 10px;
        background: var(--st-question);
        border-bottom: 2px solid #b3822e;
        color: #1a1400;
        font-size: 13px;
    }

    .mark {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        width: 22px;
        height: 22px;
        border-radius: 50%;
        background: #1a1400;
        color: var(--st-question);
        font: 700 14px var(--display);
    }

    .text {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .next {
        margin-left: 6px;
    }

    .open {
        flex: none;
        background: #1a1400;
        border-color: #1a1400;
        color: var(--st-question);
        font-weight: 700;
    }

    .open:hover:not(:disabled) {
        border-color: #fff;
    }

    @media (prefers-reduced-motion: no-preference) {
        .mark {
            animation: mark-pulse 2.4s ease-in-out infinite;
        }
    }

    @keyframes mark-pulse {
        0%,
        100% {
            transform: scale(1);
        }
        50% {
            transform: scale(1.12);
        }
    }
</style>
