<script lang="ts">
    // A small ring that turns. With `prefers-reduced-motion: reduce` it stays still (as a ring with
    // one bright arc), like the logo spinner in OfficeHeader.svelte. The text next to it says what is
    // going on, so the spinner is hidden from screen readers.
    let { size = 14 }: { size?: number } = $props();
</script>

<span class="spinner" aria-hidden="true" style:--size="{size}px"></span>

<style>
    .spinner {
        display: inline-block;
        flex: none;
        width: var(--size);
        height: var(--size);
        border: max(2px, calc(var(--size) / 8)) solid var(--line);
        border-top-color: var(--accent);
        border-radius: 50%;
        vertical-align: middle;
    }

    @media (prefers-reduced-motion: no-preference) {
        .spinner {
            animation: spin 0.8s linear infinite;
        }
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }
</style>
