<script lang="ts">
    import { store } from "$lib/store.svelte";

    let text = $state("");
    let result = $state<{ ok: boolean; text: string } | null>(null);
    let busy = $state(false);

    const session = $derived(store.activeSession);

    async function send(): Promise<void> {
        const target = store.activeId;
        if (!target || text === "" || busy) return;
        busy = true;
        const refusal = await store.debugPaste(target, text);
        busy = false;
        if (refusal === null) {
            result = { ok: true, text: "sent" };
            text = "";
        } else {
            result = { ok: false, text: refusal };
        }
    }
</script>

<form
    class="debug"
    onsubmit={(e) => {
        e.preventDefault();
        void send();
    }}
>
    <label for="debug-paste">debug paste</label>
    <input
        id="debug-paste"
        type="text"
        autocomplete="off"
        placeholder={session ? "Text for the active session" : "No active session"}
        disabled={!session}
        bind:value={text}
        oninput={() => (result = null)}
    />
    <button type="submit" disabled={!session || text === "" || busy}>send</button>
    {#if result}
        <span class="result" class:ok={result.ok} class:refused={!result.ok} role="status">{result.text}</span>
    {/if}
</form>

<style>
    .debug {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 5px 8px;
        background: var(--panel);
        border-top: 2px solid var(--line);
    }

    label {
        color: var(--dim);
        text-transform: uppercase;
        font-size: 11px;
        letter-spacing: 0.1em;
        white-space: nowrap;
    }

    input {
        flex: 1;
    }

    .result {
        white-space: nowrap;
    }

    .ok {
        color: var(--ok);
    }

    .refused {
        color: var(--danger);
        font-weight: 700;
    }
</style>
