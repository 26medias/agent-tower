<script lang="ts">
    import type { Status } from "$lib/types";

    let { status }: { status: Status } = $props();
</script>

<span class="dot {status}" title={status} data-status={status}></span>

<style>
    .dot {
        display: inline-block;
        width: 12px;
        height: 12px;
        flex: none;
        border: 1px solid #0b111c;
        border-radius: 6px;
        background: var(--st-stopped);
    }

    .starting {
        background: var(--st-starting);
    }

    .working {
        background: var(--st-working);
    }

    .idle {
        background: var(--st-idle);
    }

    .done {
        background: var(--st-done);
    }

    .stopped {
        background: var(--st-stopped);
    }

    .question {
        background: var(--st-question);
        animation: pulse 1.1s ease-in-out infinite;
    }

    @keyframes pulse {
        0%,
        100% {
            box-shadow: 0 0 0 0 rgba(255, 176, 32, 0.8);
            transform: scale(1);
        }
        50% {
            box-shadow: 0 0 0 5px rgba(255, 176, 32, 0);
            transform: scale(1.2);
        }
    }
</style>
