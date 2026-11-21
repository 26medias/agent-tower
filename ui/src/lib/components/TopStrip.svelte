<script lang="ts">
    // The top strip of the work area. It is ONE row of a fixed height (`--strip-h`) that is always
    // there, so that nothing below it moves when a question or a gone folder shows:
    //   - nothing needs attention: the name and the path of the open floor,
    //   - one or more sessions of ANY known floor ask a question: the strip turns amber. It says who
    //     asks and has an "Open" button. It does not go away by itself, only when no session asks,
    //   - the folder of the open floor is gone (`FloorView.gone`, docs/ipc-final.md): the strip turns
    //     red. It names the folder and has the buttons "Forget this floor" and "Check again". The live
    //     sessions keep running: their terminals stay usable. Every control that writes is disabled,
    //   - both: the strip is red, and the question is an amber box in it.
    // "Open" opens the floor of the session when it is not the open one, makes its tab the active
    // tab, selects the employee and puts the keyboard focus in the terminal. With several questions,
    // each click opens the next one, the oldest first.
    import { store } from "$lib/store.svelte";
    import ConfirmButton from "./ConfirmButton.svelte";

    const view = $derived(store.floorView);
    const questions = $derived(store.questions);
    const next = $derived(store.nextQuestion);
    const isGone = $derived(view?.gone === true);
    const asking = $derived(questions.length > 0 && next !== null);
    const mode = $derived(isGone ? (asking ? "both" : "gone") : asking ? "question" : "idle");

    let checking = $state(false);

    async function check(): Promise<void> {
        if (checking) return;
        checking = true;
        await store.checkFloorAgain();
        checking = false;
    }

    function where(q: { path: string; floorName: string }): string {
        return q.path === store.openPath ? "" : ` on ${q.floorName}`;
    }
</script>

<div class="strip" data-testid="top-strip" data-mode={mode}>
    {#if view && !isGone}
        <h2 class="name" title={view.name}>{view.name}</h2>
        <span class="path" title={view.path}>{view.path}</span>
    {:else if !view}
        <span class="none">No floor is open</span>
    {/if}

    {#if view && isGone}
        <div class="gone" role="alert" data-testid="gone-banner" data-path={view.path}>
            <span class="mark" aria-hidden="true">!</span>
            <!-- Three parts. When the strip is narrow, the last sentence goes first, then the path. The title has it all. -->
            <span class="text gone-text" title="The folder of this floor is gone: {view.path}. The app cannot read or change this floor.">
                <span class="lead">The folder is gone:</span>
                <span class="where"><strong class="gone-path" data-testid="gone-path">{view.path}</strong>.</span>
                <span class="tail">The app cannot read or change this floor.</span>
            </span>
            <div class="buttons">
                <ConfirmButton
                    class="forget"
                    label="Forget this floor"
                    confirmLabel="Forget it? Click again"
                    title="Remove this floor from the list. No file is deleted."
                    onconfirm={() => void store.forgetFloor(view.path)}
                />
                <button type="button" class="check" data-testid="gone-check" disabled={checking} onclick={() => void check()}>
                    {checking ? "Checking..." : "Check again"}
                </button>
            </div>
        </div>
    {/if}

    <!-- The slot is always there, so that a screen reader says the new question. -->
    <div class="slot" aria-live="polite">
        {#if asking && next}
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
</div>

<style>
    .strip {
        display: flex;
        align-items: center;
        gap: 10px;
        flex: none;
        height: var(--strip-h);
        min-width: 0;
        padding: 0 10px;
        overflow: hidden;
        background: var(--bg-deep);
        border-bottom: 1px solid var(--line);
        font-size: 13px;
    }

    .name {
        flex: none;
        max-width: 40%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-family: var(--display);
        font-size: 15px;
        color: var(--text);
        text-transform: none;
        letter-spacing: 0.02em;
    }

    .path,
    .none {
        flex: 1 1 0;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--dim);
        font-size: 12px;
    }

    .slot {
        display: flex;
        flex: 0 1 auto;
        min-width: 0;
    }

    .mark {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        width: 22px;
        height: 22px;
        border-radius: 50%;
        font: 700 14px var(--display);
    }

    .text {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gone-text {
        display: flex;
        gap: 0.3em;
        overflow: hidden;
        text-overflow: clip;
    }

    .gone-text > span {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gone-text .lead {
        flex: none;
    }

    .gone-text .where {
        flex: 0 1 auto;
    }

    /* The last sentence takes what is left after the path. With no room left it has no width. */
    .gone-text .tail {
        flex: 1 1 0;
    }

    /* ---- a question: the whole strip is amber ---- */

    .strip[data-mode="question"] {
        background: var(--st-question);
        border-bottom: 2px solid #b3822e;
        color: #1a1400;
    }

    .strip[data-mode="question"] .name,
    .strip[data-mode="question"] .path {
        color: #1a1400;
    }

    .strip[data-mode="question"] .path {
        opacity: 0.7;
    }

    .attention {
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
    }

    .attention .mark {
        background: #1a1400;
        color: var(--st-question);
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

    /* ---- a folder that is gone: the whole strip is red ---- */

    .strip[data-mode="gone"],
    .strip[data-mode="both"] {
        background: #3a1822;
        border-bottom: 2px solid var(--danger);
        color: var(--text);
    }

    .gone {
        display: flex;
        align-items: center;
        gap: 10px;
        flex: 1 1 0;
        min-width: 0;
    }

    .gone .mark {
        background: var(--danger);
        color: #2a0a12;
    }

    .gone-path {
        font-family: var(--mono);
        font-weight: 600;
    }

    .buttons {
        display: flex;
        flex: none;
        gap: 6px;
    }

    /* Both: the question is an amber box in the red strip. */
    .strip[data-mode="both"] .attention {
        padding: 2px 4px 2px 8px;
        border-radius: 6px;
        background: var(--st-question);
        color: #1a1400;
    }

    .strip[data-mode="both"] .slot {
        max-width: 45%;
    }

    /* The red strip already has a mark. The amber box needs no second one. */
    .strip[data-mode="both"] .attention .mark {
        display: none;
    }

    @media (prefers-reduced-motion: no-preference) {
        .attention .mark {
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
