<script lang="ts">
    // The settings panel (docs/ipc-settings.md, section 6): API keys, the logo image model, the
    // model lists, the floor defaults and the program paths. Escape and a click on the backdrop
    // close it. It is a modal (see modal.ts): the focus stays inside, and the page behind is inert.
    import { onMount, tick } from "svelte";
    import { modal } from "$lib/modal";
    import { store } from "$lib/store.svelte";
    import ActionError from "./ActionError.svelte";
    import FloorDefaultsForm from "./FloorDefaultsForm.svelte";
    import ModelListEditor from "./ModelListEditor.svelte";
    import PermissionsForm from "./PermissionsForm.svelte";
    import ProgramsPanel from "./ProgramsPanel.svelte";

    let { onclose }: { onclose: () => void } = $props();

    let keyDraft = $state("");
    let keyBusy = $state(false);

    onMount(() => {
        // The Codex file can change while the app runs: read the lists again.
        void store.loadModels();
        void store.loadSettings();
        // The switch of the Codex sandbox needs a fresh probe. It runs in the background.
        void store.testSandbox();
        // A toast can ask for one section, for example the program paths.
        const want = store.settingsSection;
        store.settingsSection = null;
        if (want) {
            void tick().then(() => document.querySelector(`[data-section="${want}"]`)?.scrollIntoView({ block: "start" }));
        }
    });

    const keyText = $derived(
        !store.settings || !store.settings.openaiKeySet
            ? "not set"
            : store.settings.openaiKeySource === "env"
              ? "set by the environment variable OPENAI_API_KEY"
              : "set in the app config file",
    );

    const imageChoices = $derived.by(() => {
        const list = store.models?.image ?? [];
        const current = store.settings?.imageModel ?? "";
        const choices = list.map((m) => ({ value: m.id, label: m.label === m.id ? m.id : `${m.label} (${m.id})` }));
        if (current !== "" && !list.some((m) => m.id === current)) {
            choices.push({ value: current, label: `${current} (not in the list)` });
        }
        return choices;
    });

    async function saveKey(): Promise<void> {
        if (keyDraft.trim() === "" || keyBusy) return;
        keyBusy = true;
        const ok = await store.setOpenaiKey(keyDraft);
        keyBusy = false;
        if (ok) keyDraft = "";
    }

    async function removeKey(): Promise<void> {
        if (keyBusy) return;
        keyBusy = true;
        await store.setOpenaiKey("");
        keyBusy = false;
    }

    const probe = $derived(store.sandboxProbe);
    const sandboxOn = $derived(store.settings?.codexSandbox ?? false);
    // The switch can be turned ON only when the last probe says `works`. It can always be turned OFF.
    const sandboxDisabled = $derived(!store.settings || store.sandboxTesting || (!sandboxOn && probe?.state !== "works"));
    const sandboxReason = $derived(
        !store.settings || sandboxOn || probe?.state === "works"
            ? null
            : store.sandboxTesting || !probe
              ? "The switch needs the result of the test."
              : "The switch stays off because the test did not pass.",
    );
    const sandboxHelp = $derived(
        probe?.state === "missing"
            ? "Install bubblewrap, so that the program bwrap is in the PATH of the sessions."
            : "Codex needs an AppArmor profile that lets bwrap use user namespaces. See docs/spec.md, decision O7.",
    );

    const codexNote = $derived(
        store.models?.codexSource === "cache" ? "from the Codex file models_cache.json" : "built-in list (no Codex file found)",
    );
</script>

<div class="backdrop" role="presentation" use:modal={{ onclose }} onclick={(e) => e.target === e.currentTarget && onclose()}>
    <div class="panel" role="dialog" aria-modal="true" aria-label="Settings">
        <header>
            <h2>Settings</h2>
            <button type="button" class="ghost close" aria-label="Close settings" onclick={onclose}>close</button>
        </header>

        <section data-section="keys" aria-label="API keys">
            <h3>API keys</h3>
            <div class="keystate">
                <span class="label">OpenAI key</span>
                <span class="state" data-source={store.settings?.openaiKeySource ?? "none"}>{keyText}</span>
            </div>
            <form
                class="keyform"
                onsubmit={(e) => {
                    e.preventDefault();
                    void saveKey();
                }}
            >
                <input
                    id="settings-openai-key"
                    type="password"
                    placeholder="paste a new key"
                    aria-label="OpenAI key"
                    autocomplete="off"
                    spellcheck="false"
                    bind:value={keyDraft}
                />
                <button type="submit" class="primary" disabled={keyDraft.trim() === "" || keyBusy}>set</button>
                {#if store.settings?.openaiKeySource === "config"}
                    <button type="button" class="danger remove-key" disabled={keyBusy} onclick={() => void removeKey()}>remove</button>
                {/if}
            </form>
            <ActionError area="keys" />
            <p class="hint">
                The panel never shows the key. A key that you paste here is stored in the app config file. The app uses it before the
                environment variable.
            </p>
        </section>

        <section data-section="image" aria-label="Logo image model">
            <h3>Logo image model</h3>
            <select
                name="imageModel"
                aria-label="Logo image model"
                value={store.settings?.imageModel ?? ""}
                disabled={!store.settings}
                onchange={(e) => void store.setImageModel(e.currentTarget.value)}
            >
                {#each imageChoices as choice (choice.value)}
                    <option value={choice.value}>{choice.label}</option>
                {/each}
            </select>
            <ActionError area="image" />
        </section>

        <section data-section="models" aria-label="Models">
            <h3>Models</h3>
            {#if store.models}
                <div class="lists">
                    <ModelListEditor kind="claude" title="Claude" models={store.models.claude} note="built-in aliases" />
                    <ModelListEditor kind="codex" title="Codex" models={store.models.codex} note={codexNote} />
                    <ModelListEditor kind="image" title="Image" models={store.models.image} note="built-in list" />
                </div>
            {:else}
                <p class="hint">Loading...</p>
            {/if}
        </section>

        {#if store.floorView}
            <section data-section="defaults" aria-label="Floor defaults">
                <h3>Floor defaults <span class="floor">{store.floorView.name}</span></h3>
                {#key store.floorView.path}
                    <FloorDefaultsForm />
                {/key}
            </section>
        {/if}

        {#if store.floorView}
            <section data-section="permissions" aria-label="Commands with no question">
                <h3>Commands with no question <span class="floor">{store.floorView.name}</span></h3>
                {#key store.floorView.path}
                    <PermissionsForm />
                {/key}
            </section>
        {/if}

        <section data-section="programs" aria-label="Programs">
            <h3>Programs</h3>
            <ProgramsPanel />
        </section>

        <section data-section="codex-sandbox" aria-label="Codex sandbox">
            <h3>Codex sandbox</h3>
            <div class="sandbox-state" data-testid="sandbox-state" data-state={store.sandboxTesting ? "testing" : (probe?.state ?? "unknown")}>
                {#if store.sandboxTesting}
                    <span class="probe testing">Testing the sandbox...</span>
                {:else if !probe}
                    <span class="probe testing">Not tested yet.</span>
                {:else if probe.state === "works"}
                    <span class="probe ok">The Codex sandbox works on this machine.</span>
                {:else}
                    <span class="probe bad">{probe.detail}</span>
                {/if}
                <button type="button" class="ghost" name="sandboxTest" disabled={store.sandboxTesting} onclick={() => void store.testSandbox()}>
                    Test again
                </button>
            </div>
            {#if !store.sandboxTesting && probe && probe.state !== "works"}
                <p class="hint" data-testid="sandbox-help">{sandboxHelp}</p>
            {/if}
            <label class="check">
                <input
                    type="checkbox"
                    name="codexSandbox"
                    checked={sandboxOn}
                    disabled={sandboxDisabled}
                    onchange={async (e) => {
                        const box = e.currentTarget;
                        const ok = await store.setCodexSandbox(box.checked);
                        // A refusal leaves the stored value as it was: show that.
                        if (!ok) box.checked = store.settings?.codexSandbox ?? false;
                    }}
                />
                <span>Use the Codex sandbox for Codex employees</span>
            </label>
            {#if sandboxReason}
                <p class="hint" data-testid="sandbox-reason">{sandboxReason}</p>
            {/if}
            <ActionError area="codex-sandbox" />
            <p class="hint">
                On: the Codex CLI enforces the policy of each Codex employee. A <em>read-only</em> employee can write only the
                reports folder. An <em>edit-in-worktree</em> employee can write only the task worktrees, the reports folder and
                the temporary folders. In a folder that is not a git repository it can write the project folder. The commands
                have no network access. An action outside the policy asks you for approval. Off: the CLI enforces no policy, and
                only the role text limits a Codex employee. The change applies at the next start of a session.
            </p>
        </section>

        <section data-section="interface" aria-label="Interface">
            <h3>Interface</h3>
            <label class="check">
                <input
                    type="checkbox"
                    name="debugBar"
                    checked={store.debugVisible}
                    onchange={(e) => store.setDebugVisible(e.currentTarget.checked)}
                />
                <span>Show the debug paste bar under the terminal (Ctrl+Shift+D)</span>
            </label>
            <label class="check">
                <input
                    type="checkbox"
                    name="notifyOnQuestion"
                    checked={store.settings?.notifyOnQuestion ?? true}
                    disabled={!store.settings}
                    onchange={(e) => void store.setNotifyOnQuestion(e.currentTarget.checked)}
                />
                <span>Desktop notification when an agent asks a question</span>
            </label>
            <ActionError area="interface" />
            <p class="hint">The app shows it only while its window has no focus, or is minimized. A click on it does nothing.</p>
        </section>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        z-index: 40;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(6, 10, 18, 0.7);
    }

    .panel {
        width: min(640px, 94vw);
        max-height: 92vh;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 12px;
        background: var(--panel);
        border: 1px solid var(--accent);
        border-radius: 6px;
        box-shadow: 0 8px 28px rgba(0, 0, 0, 0.6);
    }

    header {
        display: flex;
        justify-content: space-between;
        align-items: center;
    }

    h2 {
        color: var(--accent);
        font-size: 14px;
    }

    section {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding-top: 10px;
        border-top: 1px solid var(--line);
    }

    h3 {
        font-size: 12px;
        color: var(--accent);
    }

    .floor {
        color: var(--dim);
        font-family: var(--sans);
        font-weight: 400;
        letter-spacing: 0.02em;
        text-transform: none;
        margin-left: 6px;
    }

    .keystate {
        display: flex;
        gap: 8px;
        align-items: baseline;
    }

    .label {
        color: var(--dim);
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    .state {
        color: var(--ok);
    }

    .state[data-source="none"] {
        color: var(--danger);
    }

    .keyform {
        display: flex;
        gap: 4px;
    }

    .keyform input {
        flex: 1;
    }

    .hint {
        margin: 0;
        color: var(--dim);
        font-size: 11px;
    }

    .lists {
        display: flex;
        flex-direction: column;
        gap: 10px;
    }

    select {
        align-self: flex-start;
        max-width: 100%;
    }

    .sandbox-state {
        display: flex;
        gap: 8px;
        align-items: baseline;
        flex-wrap: wrap;
    }

    .sandbox-state .ok {
        color: var(--ok);
    }

    .sandbox-state .bad {
        color: var(--danger);
        overflow-wrap: anywhere;
        user-select: text;
    }

    .sandbox-state .testing {
        color: var(--dim);
    }

    .check {
        display: flex;
        align-items: center;
        gap: 8px;
    }
</style>
