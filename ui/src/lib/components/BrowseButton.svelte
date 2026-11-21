<script lang="ts">
    // "Browse..." next to a folder field. It opens the system folder dialog and gives the chosen
    // path to `onpick`. It does not open the floor. In a browser (the mock) it is disabled, and
    // the title says why.
    import { BROWSE_MOCK_TITLE, browseFolder, canBrowse } from "$lib/browse";
    import { errorText } from "$lib/ipc";
    import { store } from "$lib/store.svelte";

    let { onpick, label = "Browse for the project folder" }: { onpick: (path: string) => void; label?: string } = $props();

    const available = canBrowse();
    let busy = $state(false);

    async function browse(): Promise<void> {
        if (busy) return;
        busy = true;
        try {
            const path = await browseFolder();
            if (path !== null) onpick(path);
        } catch (e) {
            store.toast(errorText(e));
        } finally {
            busy = false;
        }
    }
</script>

<button
    type="button"
    class="browse"
    data-testid="browse"
    aria-label={label}
    title={available ? "Choose the folder in the system dialog" : BROWSE_MOCK_TITLE}
    disabled={!available || busy}
    onclick={() => void browse()}
>
    Browse...
</button>
