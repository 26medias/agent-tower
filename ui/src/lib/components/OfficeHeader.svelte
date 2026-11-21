<script lang="ts">
    // The header of the office: logo, description, and ONE row of buttons: one main button and a
    // "More" menu for the others. The name and the path of the floor are in the top strip
    // (TopStrip.svelte). A failed logo shows its error. `NO_KEY:` shows a field for the key.
    import { tick } from "svelte";
    import { splitFileError, tidyError } from "$lib/messages";
    import { resumeButton, resumeWho } from "$lib/resume";
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

    // The button "Resume the last team (N)" (docs/ipc-m3.md). The app resumes one employee at a time.
    const resume = $derived(resumeButton(view?.resumable ?? [], view?.resumeProgress ?? null));
    let resumeBusy = $state(false);
    // Who the button resumes, for a line under the buttons and for the tooltip.
    const resumeLabels = $derived(
        (view?.resumable ?? []).map((slug) => {
            const emp = view?.employees.find((e) => e.slug === slug);
            return emp ? `${emp.name} (${emp.role})` : slug;
        }),
    );
    // The setup agent starts with the floor default CLI when there is one. Else the operator chooses.
    const defaultCli = $derived(view?.defaults.cli ?? null);
    // One primary (green) button at a time: the resume of the team first, then the setup agent.
    const resumePrimary = $derived(resume.visible && !resume.busy);

    async function resumeTeam(): Promise<void> {
        if (resumeBusy || resume.busy) return;
        resumeBusy = true;
        await store.resumeTeam();
        resumeBusy = false;
    }

    // The menu "More": the setup agent (when the resume is the main button), the other CLI and the new logo.
    let moreOpen = $state(false);
    let menuEl = $state<HTMLElement | null>(null);

    function onWindowClick(e: MouseEvent): void {
        if (moreOpen && menuEl && !menuEl.contains(e.target as Node)) moreOpen = false;
    }

    function onMenuKey(e: KeyboardEvent): void {
        if (e.key === "Escape" && moreOpen) {
            e.stopPropagation();
            moreOpen = false;
        }
    }

    // The description shows two lines. "more" shows all of it, when it is longer.
    let expanded = $state(false);
    let clipped = $state(false);

    function watchClip(node: HTMLElement, _text: string | null) {
        const check = (): void => {
            if (!expanded) clipped = node.scrollHeight > node.clientHeight + 1;
        };
        const observer = new ResizeObserver(check);
        observer.observe(node);
        check();
        return {
            // The new text is in the page after the next tick. Then the clip can be measured.
            update: () => {
                expanded = false;
                void tick().then(check);
            },
            destroy: () => observer.disconnect(),
        };
    }

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

    function clickSetup(): void {
        if (defaultCli) void runSetup(defaultCli);
        else askCli = true;
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
        if (busy || store.gone) return;
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

<svelte:window onclick={onWindowClick} />

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
        <div class="about">
            {#if view.description}
                <p class="description" class:expanded use:watchClip={view.description} data-testid="description">{view.description}</p>
                {#if clipped || expanded}
                    <button type="button" class="more-text" aria-expanded={expanded} data-testid="description-more" onclick={() => (expanded = !expanded)}>
                        {expanded ? "less" : "more"}
                    </button>
                {/if}
            {:else if needsSetup}
                <p class="hint">This floor has no description yet. The setup agent reads the project and proposes a team.</p>
            {/if}
        </div>
    </div>

    <div class="buttons">
        {#if setupRunning}
            <button type="button" class="setup stop-setup" disabled={stopping} onclick={stopSetup}>Stop the setup</button>
        {:else if setupBusy}
            <button type="button" class="setup busy-setup" disabled aria-busy="true"><Spinner size={11} /> starting...</button>
        {:else if askCli}
            <!-- The choice has its own row. The menu is not part of it. -->
            <div class="choose" role="group" aria-label="Choose the CLI of the setup agent">
                <span class="ask">Run the setup agent with</span>
                <button type="button" class="cli-claude" class:primary={(defaultCli ?? "claude") === "claude"} disabled={store.gone} title={store.goneTitle()} onclick={() => runSetup("claude")}>claude</button>
                <button type="button" class="cli-codex" class:primary={defaultCli === "codex"} disabled={store.gone} title={store.goneTitle()} onclick={() => runSetup("codex")}>codex</button>
                <button type="button" class="ghost" onclick={() => (askCli = false)}>cancel</button>
            </div>
        {:else if resume.visible}
            <button
                type="button"
                class="resume-team main"
                class:primary={resumePrimary}
                data-testid="resume-team"
                disabled={resume.busy || resumeBusy || store.gone}
                aria-busy={resume.busy}
                title={store.goneTitle(`Resume the sessions that were live when the app closed, one by one: ${resumeLabels.join(", ")}`)}
                onclick={resumeTeam}
            >
                {#if resume.busy || resumeBusy}<Spinner size={11} />{/if}
                {resume.text}
            </button>
        {:else}
            <button
                type="button"
                class="setup main"
                class:primary={needsSetup}
                disabled={store.gone}
                title={store.goneTitle(defaultCli ? `Starts the setup agent with ${defaultCli}, the CLI that the floor defaults name.` : "Choose claude or codex, then the setup agent reads the project and proposes a team.")}
                onclick={clickSetup}
            >
                Run the setup agent
            </button>
        {/if}
        {#if !askCli}
            <div class="menu-wrap" bind:this={menuEl} role="presentation" onkeydown={onMenuKey}>
                <button
                    type="button"
                    class="more"
                    data-testid="more"
                    aria-haspopup="menu"
                    aria-expanded={moreOpen}
                    title="More actions"
                    onclick={() => (moreOpen = !moreOpen)}
                >
                    More <span aria-hidden="true">&#9662;</span>
                </button>
                <!-- Always in the page, hidden while the menu is closed. -->
                <div class="menu" role="menu" hidden={!moreOpen} data-testid="more-menu">
                    {#if resume.visible && !setupRunning && !setupBusy}
                        <button
                            type="button"
                            role="menuitem"
                            class="setup"
                            disabled={store.gone}
                            title={store.goneTitle(defaultCli ? `Starts the setup agent with ${defaultCli}, the CLI that the floor defaults name.` : "Choose claude or codex, then the setup agent reads the project and proposes a team.")}
                            onclick={() => {
                                moreOpen = false;
                                clickSetup();
                            }}
                        >
                            Run the setup agent
                        </button>
                    {/if}
                    {#if defaultCli && !setupRunning && !setupBusy}
                        <button type="button" role="menuitem" class="other-cli" disabled={store.gone} title={store.goneTitle()} onclick={() => { moreOpen = false; askCli = true; }}>
                            Run it with the other CLI
                        </button>
                    {/if}
                    <button
                        type="button"
                        role="menuitem"
                        class="newlogo"
                        aria-expanded={logoForm}
                        disabled={busy || store.gone}
                        title={store.goneTitle()}
                        onclick={() => {
                            moreOpen = false;
                            toggleLogoForm();
                        }}
                    >
                        {busy ? "Making a logo..." : "New logo"}
                    </button>
                </div>
            </div>
        {/if}
    </div>

    {#if resume.visible && resumeLabels.length > 0 && !resume.busy}
        <p class="hint who" data-testid="resume-who">Resumes: {resumeWho(resumeLabels)}</p>
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
                <button type="submit" class="primary" disabled={store.gone} title={store.goneTitle()}>generate</button>
            </div>
        </form>
    {/if}

    {#if error}
        <p class="error" role="alert" data-error="logo">{tidyError(error)}</p>
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
        <ul class="errors" aria-label="Employee files that did not load" data-testid="employee-errors">
            {#each view.employeeErrors as err (err)}
                {@const e = splitFileError(err)}
                <li>
                    {#if e.file}
                        <span class="e-file">{e.file}</span>: {e.message}
                        <span class="e-folder">{e.folder}</span>
                    {:else}
                        {e.message}
                    {/if}
                </li>
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
        width: 40px;
        height: 40px;
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
        border: 3px solid #fff;
        border-radius: 11px;
        background: #33487a;
        font: 700 20px var(--display);
    }

    .spinner {
        position: absolute;
        inset: 11px;
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

    .about {
        flex: 1;
        min-width: 0;
        min-height: 40px;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        justify-content: center;
    }

    /* Two lines. "more" shows all of the text. */
    .description,
    .about .hint {
        margin: 0;
        font-size: 12px;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
        overflow-wrap: anywhere;
    }

    .description {
        color: var(--text);
    }

    .description.expanded {
        -webkit-line-clamp: unset;
        line-clamp: unset;
        display: block;
    }

    .more-text {
        padding: 0;
        border: 0;
        background: none;
        color: var(--accent);
        font-size: 11px;
    }

    .more-text:hover:not(:disabled) {
        text-decoration: underline;
    }

    /* One row: the main button and the menu. It does not wrap. */
    .buttons {
        display: flex;
        align-items: center;
        gap: 6px;
        min-width: 0;
    }

    .buttons button {
        text-transform: none;
    }

    .buttons .main {
        flex: 0 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .buttons .resume-team {
        display: inline-flex;
        align-items: center;
        gap: 5px;
    }

    /* The "stop" button stays above the dim of a gone floor (see Office.svelte), and it stays enabled. */
    .buttons button.stop-setup {
        position: relative;
        z-index: 3;
    }

    .buttons button.busy-setup {
        display: inline-flex;
        align-items: center;
        gap: 5px;
    }

    .menu-wrap {
        position: relative;
        flex: none;
        margin-left: auto;
    }

    .menu-wrap .more {
        color: var(--dim);
        background: transparent;
    }

    .menu {
        position: absolute;
        z-index: 5;
        top: calc(100% + 4px);
        right: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-width: 170px;
        padding: 4px;
        background: var(--panel);
        border: 1px solid var(--line);
        border-radius: 8px;
        box-shadow: 0 8px 20px rgba(0, 0, 0, 0.5);
    }

    .menu[hidden] {
        display: none;
    }

    .menu button {
        text-align: left;
        border-color: transparent;
        background: transparent;
        white-space: nowrap;
    }

    .menu button:hover:not(:disabled) {
        background: var(--panel-2);
    }

    .ask {
        color: var(--dim);
        font-size: 12px;
    }

    /* The CLI choice fills its own row. */
    .choose {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
        width: 100%;
    }

    .hint.who {
        margin-top: -2px;
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
        overflow-wrap: anywhere;
    }

    .e-file {
        font-weight: 700;
    }

    /* The folder is on its own line and wraps: the file name is the part that matters. */
    .e-folder {
        display: block;
        color: var(--dim);
        font-size: 11px;
    }
</style>
