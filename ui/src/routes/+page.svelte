<svelte:head>
    <title>Agent Tower</title>
</svelte:head>

<main class="container">
    <h1>Agent Tower</h1>
</main>

<style>
    :global(:root) {
        font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
        font-size: 16px;
        line-height: 24px;
        color: #0f0f0f;
        background-color: #f6f6f6;
    }

    .container {
        padding-top: 10vh;
        text-align: center;
    }

    @media (prefers-color-scheme: dark) {
        :global(:root) {
            color: #f6f6f6;
            background-color: #2f2f2f;
        }
    }
</style>
