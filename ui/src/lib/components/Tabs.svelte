<script lang="ts">
    import { store } from "$lib/store.svelte";
    import { SETUP_SLUG } from "$lib/types";
    import StatusDot from "./StatusDot.svelte";

    // Two employees with the same name get their role in the tab, so that the tabs differ.
    const doubles = $derived.by(() => {
        const seen = new Set<string>();
        const twice = new Set<string>();
        for (const e of store.floorView?.employees ?? []) {
            const key = e.name.trim().toLowerCase();
            if (seen.has(key)) twice.add(key);
            seen.add(key);
        }
        return twice;
    });

    function nameOf(slug: string): string {
        if (slug === SETUP_SLUG) return "Setup";
        const emp = store.floorView?.employees.find((e) => e.slug === slug);
        if (!emp) return slug;
        return doubles.has(emp.name.trim().toLowerCase()) ? `${emp.name} (${emp.role})` : emp.name;
    }
</script>

<div class="tabs" role="tablist" aria-label="Sessions">
    {#each store.tabs as tab (tab.sessionId)}
        <div
            class="tab"
            class:active={tab.sessionId === store.activeId}
            class:stopped={tab.status === "stopped"}
            class:asking={tab.status === "question"}
            data-session={tab.sessionId}
            data-status={tab.status}
        >
            <button
                type="button"
                class="main"
                role="tab"
                aria-selected={tab.sessionId === store.activeId}
                onclick={() => store.selectTab(tab.sessionId)}
            >
                <StatusDot status={tab.status} spin />
                <span class="label">{nameOf(tab.slug)}</span>
                {#if tab.status === "question"}
                    <span class="asks" data-testid="tab-asks">asks</span>
                {:else if store.isUnread(tab.sessionId)}
                    <span class="unread" data-testid="tab-unread" title="This session finished a turn. Open the tab to read it." role="img" aria-label="Finished a turn, not read yet"></span>
                {/if}
                {#if !tab.messageWaiting && store.hasMsgMark(tab.sessionId)}
                    <!-- A message was delivered to this session while its tab was not in view. It shows for 10 s. -->
                    <span class="got" data-testid="tab-msg" title="A message from the team arrived in this session." role="img" aria-label="A message arrived">MSG</span>
                {/if}
            </button>
            {#if tab.messageWaiting}
                <button
                    type="button"
                    class="mark waiting"
                    title="A message waits. Click to clear your draft flag so it can be delivered."
                    aria-label="Message waiting. Clear the draft flag."
                    onclick={() => store.clearDraft(tab.sessionId)}
                >
                    msg
                </button>
            {:else if tab.draft}
                <button
                    type="button"
                    class="mark draft"
                    title="The app thinks that you have unsent text here. It holds messages while this flag is set. Click to clear the flag, for example when you deleted the text."
                    aria-label="Draft flag set. Clear the draft flag."
                    onclick={() => store.clearDraft(tab.sessionId)}
                >
                    draft
                </button>
            {/if}
            {#if tab.status === "stopped"}
                <button
                    type="button"
                    class="close ghost"
                    title="Close this tab"
                    aria-label="Close tab"
                    onclick={() => store.closeTab(tab.sessionId)}
                >
                    x
                </button>
            {/if}
        </div>
    {:else}
        <span class="none">No live session</span>
    {/each}
</div>

<style>
    .tabs {
        display: flex;
        align-items: flex-end;
        gap: 2px;
        padding: 4px 8px 0;
        /* A fixed height: the row must not change when the first tab comes (the terminal would jump). */
        height: 33px;
        overflow-x: auto;
        background: var(--bg);
        border-bottom: 1px solid var(--line);
    }

    .tab {
        display: flex;
        align-items: center;
        gap: 2px;
        padding-right: 4px;
        background: var(--panel);
        border: 1px solid var(--line);
        border-bottom: 0;
        border-radius: 6px 6px 0 0;
        margin-bottom: -2px;
    }

    .tab.active {
        background: var(--bg-deep);
        border-color: var(--accent);
    }

    .tab.stopped .label {
        color: var(--dim);
    }

    /* A session that waits for an answer: an amber tab with the word "asks". It pulses slowly. */
    .tab.asking,
    .tab.asking.active {
        background: var(--st-question);
        border-color: var(--st-question);
        color: var(--accent-ink);
    }

    .tab.asking .label {
        color: #1a1400;
        font-weight: 700;
    }

    /* The dot is amber too: on the amber tab it turns dark. */
    .tab.asking :global(.dot) {
        background: #1a1400;
        border-color: #1a1400;
        animation: none;
    }

    .asks {
        padding: 0 5px;
        border-radius: 8px;
        background: #1a1400;
        color: var(--st-question);
        font-size: 11px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.06em;
    }

    .unread {
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--accent);
        box-shadow: 0 0 0 2px rgba(94, 224, 192, 0.25);
    }

    /* The look of the unread mark (the accent colour and its soft ring), as a small word. No new strong colour. */
    .got {
        padding: 0 4px;
        border: 1px solid var(--accent);
        border-radius: 8px;
        color: var(--accent);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        box-shadow: 0 0 0 2px rgba(94, 224, 192, 0.25);
    }

    /* On the amber tab of a question the word is dark, like "asks". */
    .tab.asking .got {
        border-color: #1a1400;
        color: #1a1400;
        box-shadow: none;
    }

    @media (prefers-reduced-motion: no-preference) {
        .tab.asking {
            animation: ask-pulse 2.4s ease-in-out infinite;
        }
    }

    @keyframes ask-pulse {
        0%,
        100% {
            background: #ffc56b;
            box-shadow: 0 0 0 0 rgba(255, 197, 107, 0);
        }
        50% {
            background: #ffe0a3;
            box-shadow: 0 0 8px 1px rgba(255, 197, 107, 0.7);
        }
    }

    .main {
        display: flex;
        align-items: center;
        gap: 6px;
        background: transparent;
        border: 0;
        padding: 3px 6px;
        text-transform: none;
    }

    .mark {
        padding: 0 4px;
        font-size: 11px;
        border-radius: 6px;
        text-transform: uppercase;
    }

    .waiting {
        background: var(--st-question);
        border-color: var(--st-question);
        color: var(--accent-ink);
        font-weight: 700;
    }

    .draft {
        background: transparent;
        border: 1px dashed var(--dim);
        color: var(--dim);
    }

    .draft:hover {
        color: var(--text);
        border-color: var(--accent);
    }

    .close {
        padding: 0 4px;
    }

    .none {
        color: var(--dim);
        padding: 4px 0 6px;
    }
</style>
