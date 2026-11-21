<script lang="ts">
    import type { Status } from "$lib/types";

    // `spin`: a session that starts shows a small turning ring in place of the dot (the tab does this).
    let { status, spin = false }: { status: Status; spin?: boolean } = $props();
</script>

<span class="dot {status}" class:spin={spin && status === "starting"} title={status} data-status={status}></span>

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

    /* A ring that turns, in place of the dot. The ring is still with reduced motion. */
    .dot.spin {
        background: transparent;
        border: 2px solid var(--line);
        border-top-color: var(--accent);
    }

    @media (prefers-reduced-motion: no-preference) {
        .dot.spin {
            animation: spin 0.8s linear infinite;
        }
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
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
