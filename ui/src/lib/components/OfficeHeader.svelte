<script lang="ts">
    // The header of the office: logo, name, path, description, and the buttons "Run the setup
    // agent" and "New logo". A failed logo shows its error. `NO_KEY:` shows a field for the key.
    import { store } from "$lib/store.svelte";
    import { SETUP_SLUG, type CliKind } from "$lib/types";
    import Spinner from "./Spinner.svelte";

    const view = $derived(store.floorView);
    const logo = $derived(view ? store.logos[view.path] : undefined);
    const busy = $derived(view?.logoBusy === true);
    const error = $derived(view ? (view.logoError ?? store.logoCallError[view.path] ?? null) : null);
    const noKey = $derived(error !== null && error.startsWith("NO_KEY:"));
    // The floor view names the live setup session. The session events know it sooner, so use both.
    const liveSetup = $derived(store.sessionOfEmployee(SETUP_SLUG));
    const setupId = $derived(store.isLive(liveSetup) ? (liveSetup?.sessionId ?? null) : (view?.setupSessionId ?? null));
    const setupRunning = $derived(setupId !== null);
    // True from the click on "claude" or "codex" until the command returns (see store.launch).
    const setupBusy = $derived(store.isBusy(SETUP_SLUG));
    const needsSetup = $derived(view !== null && !view.description && !setupRunning);

    let askCli = $state(false);
    let logoForm = $state(false);
    let promptDraft = $state("");
    let keyDraft = $state("");
    let keyBusy = $state(false);
    let stopping = $state(false);

    async function runSetup(cli: CliKind): Promise<void> {
        askCli = false;
        await store.startSetup(cli);
    }

    async function stopSetup(): Promise<void> {
        if (setupId === null || stopping) return;
        stopping = true;
        await store.stopSession(setupId);
        stopping = false;
    }

    function toggleLogoForm(): void {
        logoForm = !logoForm;
        if (logoForm) promptDraft = view?.logoPrompt ?? "";
    }

    async function generate(): Promise<void> {
        if (busy) return;
        const text = promptDraft.trim();
        logoForm = false;
        // An empty field is "no change", unless the operator emptied a prompt that had a value:
        // then the empty text removes the stored prompt.
        const hadPrompt = (view?.logoPrompt ?? "").trim() !== "";
        await store.generateLogo(text === "" && !hadPrompt ? null : text);
    }

    async function saveKey(): Promise<void> {
        if (keyDraft.trim() === "" || keyBusy) return;
        keyBusy = true;
        const saved = await store.saveKeyAndRetry(keyDraft);
        keyBusy = false;
        if (saved) keyDraft = "";
    }
</script>

{#if view}
    <div class="head">
        <div class="logo" class:busy aria-busy={busy}>
            {#if logo}
                <img src={logo} alt="Logo of {view.name}" />
            {:else}
                <span class="plain" aria-hidden="true">{[...view.name.trim()][0]?.toUpperCase() ?? "?"}</span>
            {/if}
            {#if busy}
                <span class="spinner" role="status" aria-label="Making a logo"></span>
            {/if}
        </div>
        <div class="text">
            <h2 class="name">{view.name}</h2>
            <span class="path" title={view.path}>{view.path}</span>
            {#if view.description}
                <p class="description">{view.description}</p>
            {/if}
        </div>
    </div>

    <div class="buttons">
        {#if setupRunning}
            <button type="button" class="setup stop-setup" disabled={stopping} onclick={stopSetup}>Stop the setup</button>
        {:else if setupBusy}
            <button type="button" class="setup busy-setup" disabled aria-busy="true"><Spinner size={11} /> starting...</button>
        {:else if askCli}
            <span class="ask">Run the setup agent with</span>
            <button type="button" class="primary cli-claude" onclick={() => runSetup("claude")}>claude</button>
            <button type="button" class="cli-codex" onclick={() => runSetup("codex")}>codex</button>
            <button type="button" class="ghost" onclick={() => (askCli = false)}>cancel</button>
        {:else}
            <button type="button" class="setup" class:primary={needsSetup} onclick={() => (askCli = true)}>Run the setup agent</button>
        {/if}
        <button type="button" class="newlogo" aria-expanded={logoForm} disabled={busy} onclick={toggleLogoForm}>
            {busy ? "Making a logo..." : "New logo"}
        </button>
    </div>

    {#if needsSetup}
        <p class="hint">This floor has no description yet. The setup agent reads the project and proposes a team.</p>
    {/if}

    {#if logoForm && !busy}
        <form
            class="logoform"
            onsubmit={(e) => {
                e.preventDefault();
                void generate();
            }}
        >
            <label for="logo-prompt">Logo subject</label>
            <div class="line">
                <input
                    id="logo-prompt"
                    type="text"
                    autocomplete="off"
                    placeholder="A golden honeycomb hexagon with a small bee."
                    bind:value={promptDraft}
                />
                <button type="submit" class="primary">generate</button>
            </div>
        </form>
    {/if}

    {#if error}
        <p class="error" role="alert" data-error="logo">{error}</p>
        {#if noKey}
            <form
                class="keyform"
                onsubmit={(e) => {
                    e.preventDefault();
                    void saveKey();
                }}
            >
                <label for="openai-key">OpenAI key</label>
                <div class="line">
                    <input
                        id="openai-key"
                        type="password"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="sk-..."
                        bind:value={keyDraft}
                    />
                    <button type="submit" class="primary" disabled={keyDraft.trim() === "" || keyBusy}>save key</button>
                </div>
            </form>
        {/if}
    {/if}

    {#if view.employeeErrors.length > 0}
        <ul class="errors" aria-label="Employee files that did not load">
            {#each view.employeeErrors as err (err)}
                <li>{err}</li>
            {/each}
        </ul>
    {/if}
{:else}
    <p class="hint">Open a floor in the building, or open a folder.</p>
{/if}

<style>
    .head {
        display: flex;
        gap: 10px;
        align-items: flex-start;
        min-width: 0;
    }

    .logo {
        position: relative;
        flex: none;
        width: 56px;
        height: 56px;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .logo img {
        width: 100%;
        height: 100%;
        object-fit: contain;
    }

    .logo.busy img,
    .logo.busy .plain {
        opacity: 0.3;
    }

    .plain {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        border: 4px solid #fff;
        border-radius: 14px;
        background: #33487a;
        font: 700 28px var(--display);
    }

    .spinner {
        position: absolute;
        inset: 12px;
        border: 4px solid var(--line);
        border-top-color: var(--accent);
        border-radius: 50%;
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

    .text {
        min-width: 0;
        display: flex;
        flex-direction: column;
    }

    .name {
        font-family: var(--display);
        font-size: 17px;
        color: var(--text);
        text-transform: none;
        letter-spacing: 0.02em;
    }

    .path {
        color: var(--dim);
        font-size: 11px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .description {
        margin: 2px 0 0;
        color: var(--text);
        font-size: 12px;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
    }

    .buttons {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
    }

    .buttons button {
        text-transform: none;
    }

    .buttons button.busy-setup {
        display: inline-flex;
        align-items: center;
        gap: 5px;
    }

    .ask {
        color: var(--dim);
        font-size: 12px;
    }

    .hint {
        margin: 0;
        color: var(--dim);
        font-size: 12px;
    }

    form {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    label {
        font-size: 11px;
        color: var(--dim);
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    .line {
        display: flex;
        gap: 4px;
    }

    .line input {
        flex: 1;
    }

    .error {
        margin: 0;
        padding: 4px 8px;
        background: #2a1620;
        border: 1px solid var(--danger);
        border-radius: 6px;
        color: var(--danger);
        font-size: 12px;
        overflow-wrap: anywhere;
    }

    .errors {
        margin: 0;
        padding: 4px 8px 4px 24px;
        background: #2a1620;
        border-radius: 6px;
        color: var(--danger);
        font-size: 12px;
    }
</style>
