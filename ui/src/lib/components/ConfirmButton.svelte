<script lang="ts">
    // A button that needs two clicks. No browser confirm dialog.
    import { onDestroy } from "svelte";

    let {
        label,
        confirmLabel = "sure?",
        title = "",
        onconfirm,
        class: klass = "",
    }: {
        label: string;
        confirmLabel?: string;
        title?: string;
        onconfirm: () => void;
        class?: string;
    } = $props();

    let armed = $state(false);
    let timer: ReturnType<typeof setTimeout> | undefined;

    function click(): void {
        if (armed) {
            armed = false;
            clearTimeout(timer);
            onconfirm();
            return;
        }
        armed = true;
        timer = setTimeout(() => (armed = false), 3000);
    }

    onDestroy(() => clearTimeout(timer));
</script>

<button type="button" class="{klass} {armed ? 'danger armed' : ''}" {title} onclick={click}>
    {armed ? confirmLabel : label}
</button>

<style>
    .armed {
        border-color: var(--danger);
        color: var(--danger);
    }
</style>
