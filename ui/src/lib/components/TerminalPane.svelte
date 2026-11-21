<script lang="ts">
    // One terminal for one session. It stays mounted while its tab is hidden.
    // While the session has the status "starting", an overlay shows that the app works: in the
    // centre while the terminal has no output, and as a small strip at the top right from the first
    // output byte on (the CLI can show a trust dialog during the start, and the overlay must not
    // hide it). The overlay takes no pointer events, so the terminal stays usable.
    import { untrack } from "svelte";
    import { modalOpen } from "$lib/modal";
    import { store } from "$lib/store.svelte";
    import {
        SLOW_HINT_TEXT,
        STUCK_TEXT,
        elapsedSeconds,
        formatElapsed,
        startingTitle,
        startingView,
        stripText,
    } from "$lib/starting";
    import { SessionTerminal } from "$lib/terminal";
    import type { Status } from "$lib/types";
    import Spinner from "./Spinner.svelte";

    let {
        sessionId,
        live,
        active,
        status,
        cli,
        name,
    }: {
        sessionId: string;
        live: boolean;
        active: boolean;
        status: Status;
        cli: string | null;
        name: string;
    } = $props();

    // A prop getter re-runs its effect each time the parent object changes, even when the value
    // is the same. A $derived compares values, so the effects below run only on a real change.
    // Without this, each status change would rebuild the terminal and steal the focus.
    const id = untrack(() => sessionId);
    const isLive = $derived(live);
    const isActive = $derived(active);
    const isStarting = $derived(status === "starting");

    let host: HTMLDivElement;
    let controller = $state.raw<SessionTerminal | null>(null);
    let hasOutput = $state(false);

    // The clock of the overlay. It runs only while the status is "starting".
    let since = $state(Date.now());
    let now = $state(Date.now());
    $effect(() => {
        if (!isStarting) return;
        since = now = Date.now();
        const timer = setInterval(() => (now = Date.now()), 1000);
        return () => clearInterval(timer);
    });
    const elapsed = $derived(elapsedSeconds(since, now));
    const view = $derived(startingView(status, hasOutput, elapsed));

    $effect(() => {
        const c = new SessionTerminal(id, host, {
            onError: (text) => store.toast(text),
            onSize: (rows, cols) => store.reportSize(rows, cols),
            onOutput: () => (hasOutput = true),
            onInput: () => store.noteInput(id),
        });
        controller = c;
        return () => {
            c.dispose();
            controller = null;
        };
    });

    $effect(() => {
        controller?.setLive(isLive);
    });

    $effect(() => {
        const c = controller;
        if (isActive && c) {
            void c.show().then(() => {
                // Do not take the focus from a field that the operator uses.
                const a = document.activeElement;
                // A modal that is open keeps the focus: the terminal must not take it.
                if (isActive && !modalOpen() && (!a || a === document.body || a.closest(".xterm"))) c.focus();
            });
        }
    });
</script>

<div class="pane" class:hidden={!isActive} bind:this={host} data-session={id}></div>

{#if status === "stopped"}
    <!-- The CLI prints its own "resume with" line. This line says what to do in the app. -->
    <div class="stopped-note" class:hidden={!isActive} data-role="stopped-note" role="status">
        Stopped. Use "resume" or "start" in the bar of the employee to continue.
    </div>
{/if}

{#if view.mode === "centre"}
    <div class="starting centre" class:hidden={!isActive} data-starting="centre" data-session-overlay={id}>
        <Spinner size={30} />
        <p class="title" role="status">{startingTitle(cli, name)}</p>
        <p class="time" data-role="elapsed">{formatElapsed(elapsed)}</p>
        {#if view.slowHint}
            <p class="note" data-role="slow-hint">{SLOW_HINT_TEXT}</p>
        {/if}
        {#if view.stuckLine}
            <p class="note stuck" data-role="stuck">{STUCK_TEXT}</p>
        {/if}
    </div>
{:else if view.mode === "strip"}
    <div class="starting strip" class:hidden={!isActive} data-starting="strip" data-session-overlay={id}>
        <div class="line">
            <Spinner size={11} />
            <span data-role="elapsed">{stripText(elapsed)}</span>
        </div>
        {#if view.stuckLine}
            <div class="note stuck" data-role="stuck">{STUCK_TEXT}</div>
        {/if}
    </div>
{/if}

<style>
    .pane {
        position: absolute;
        inset: 0;
    }

    .pane.hidden {
        visibility: hidden;
    }

    .stopped-note {
        position: absolute;
        z-index: 5;
        left: 50%;
        bottom: 14px;
        transform: translateX(-50%);
        max-width: 90%;
        padding: 3px 12px;
        border: 1px solid var(--line);
        border-radius: 12px;
        background: rgba(24, 35, 58, 0.95);
        color: var(--dim);
        font-size: 12px;
        pointer-events: none;
    }

    .stopped-note.hidden {
        visibility: hidden;
    }

    .starting {
        position: absolute;
        z-index: 5;
        pointer-events: none;
        color: var(--text);
    }

    .starting.hidden {
        visibility: hidden;
    }

    .centre {
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 6px;
        padding: 16px;
        text-align: center;
    }

    .centre p {
        margin: 0;
    }

    .title {
        font-family: var(--display);
        font-size: 15px;
        margin-top: 6px;
    }

    .time {
        color: var(--dim);
        font-variant-numeric: tabular-nums;
    }

    .note {
        max-width: 460px;
        color: var(--dim);
        font-size: 12px;
    }

    .note.stuck {
        color: var(--st-question);
    }

    .strip {
        top: 8px;
        right: 18px;
        max-width: min(360px, 70%);
        padding: 3px 9px;
        background: rgba(16, 24, 38, 0.92);
        border: 1px solid var(--line);
        border-radius: 6px;
        font-size: 12px;
    }

    .strip .line {
        display: flex;
        align-items: center;
        gap: 6px;
        font-variant-numeric: tabular-nums;
    }

    .strip .note {
        margin-top: 2px;
        font-size: 11px;
    }
</style>
