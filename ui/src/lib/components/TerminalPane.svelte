<script lang="ts">
    // One terminal for one session. It stays mounted while its tab is hidden.
    import { untrack } from "svelte";
    import { store } from "$lib/store.svelte";
    import { SessionTerminal } from "$lib/terminal";

    let { sessionId, live, active }: { sessionId: string; live: boolean; active: boolean } = $props();

    // A prop getter re-runs its effect each time the parent object changes, even when the value
    // is the same. A $derived compares values, so the effects below run only on a real change.
    // Without this, each status change would rebuild the terminal and steal the focus.
    const id = untrack(() => sessionId);
    const isLive = $derived(live);
    const isActive = $derived(active);

    let host: HTMLDivElement;
    let controller = $state.raw<SessionTerminal | null>(null);

    $effect(() => {
        const c = new SessionTerminal(id, host, {
            onError: (text) => store.toast(text),
            onSize: (rows, cols) => store.reportSize(rows, cols),
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
                if (isActive && (!a || a === document.body || a.closest(".xterm"))) c.focus();
            });
        }
    });
</script>

<div class="pane" class:hidden={!isActive} bind:this={host} data-session={id}></div>

<style>
    .pane {
        position: absolute;
        inset: 0;
    }

    .pane.hidden {
        visibility: hidden;
    }
</style>
