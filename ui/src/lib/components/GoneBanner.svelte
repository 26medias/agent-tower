<script lang="ts">
    // The banner of a floor whose folder is gone (`FloorView.gone`, docs/ipc-final.md). It takes its
    // own row at the top of the work area, under the attention bar, so it covers nothing. It stays
    // until the folder is back. The live sessions keep running: their terminals stay usable, and
    // their "stop" buttons stay enabled. Every control that writes in the floor is disabled.
    import { store } from "$lib/store.svelte";
    import ConfirmButton from "./ConfirmButton.svelte";

    const view = $derived(store.floorView);
    let checking = $state(false);

    async function check(): Promise<void> {
        if (checking) return;
        checking = true;
        await store.checkFloorAgain();
        checking = false;
    }
</script>

<!-- The wrapper is always there, so that the page can measure it. It has no height while the floor is not gone. -->
<div class="wrap">
    {#if view && view.gone}
        <div class="gone" role="alert" data-testid="gone-banner" data-path={view.path}>
            <span class="mark" aria-hidden="true">!</span>
            <span class="text">
                The folder of this floor is gone: <strong class="path" data-testid="gone-path">{view.path}</strong>. The app cannot read or change
                this floor.
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
</div>

<style>
    .gone {
        display: flex;
        align-items: center;
        gap: 10px;
        min-height: 34px;
        padding: 4px 10px;
        background: #3a1822;
        border-bottom: 2px solid var(--danger);
        color: var(--text);
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
        background: var(--danger);
        color: #2a0a12;
        font: 700 14px var(--display);
    }

    .text {
        flex: 1;
        min-width: 0;
        overflow-wrap: anywhere;
    }

    .path {
        font-family: var(--mono);
        font-weight: 600;
    }

    .buttons {
        display: flex;
        flex: none;
        flex-wrap: wrap;
        justify-content: flex-end;
        gap: 6px;
    }
</style>
