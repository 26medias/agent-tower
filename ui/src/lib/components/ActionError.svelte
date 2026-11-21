<script lang="ts">
    // The error of the last action of a modal, as a line in the modal. The toast of the same error stays under the
    // backdrop while the modal is open, so the operator could not see it. The text comes from the store (`actionErrors`).
    import { store } from "$lib/store.svelte";

    let { area }: { area: string } = $props();

    const text = $derived(store.actionErrors[area] ?? null);
</script>

{#if text}
    <p class="action-error" role="alert" data-testid="action-error-{area}">{text}</p>
{/if}

<style>
    .action-error {
        margin: 0;
        color: var(--danger);
        font-size: 12px;
        overflow-wrap: anywhere;
        user-select: text;
    }
</style>
