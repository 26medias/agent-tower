<script lang="ts">
    import { onMount, untrack } from "svelte";
    import {
        afterCliChange,
        afterModelChange,
        defaultModelFor,
        effortChoices,
        effortList,
        floorDefaultLabel,
        listFor,
        modelChoices,
    } from "$lib/models";
    import { modal } from "$lib/modal";
    import { POLICY_IDS, POLICY_INFO, defaultManager } from "$lib/policies";
    import { isValidSlug, slugify, uniqueSlug } from "$lib/slug";
    import { store } from "$lib/store.svelte";
    import type { CliKind, Employee, Policy } from "$lib/types";

    let { employee, onclose }: { employee: Employee | null; onclose: () => void } = $props();

    // The form edits a copy. The parent re-creates the form when it needs another employee.
    const initial = untrack(() => employee);
    const editing = initial !== null;
    const taken = (store.floorView?.employees ?? []).map((e) => e.slug);

    let name = $state(initial?.name ?? "");
    let role = $state(initial?.role ?? "Engineer");
    // A new employee reports to the Project Manager (else the CEO) by default.
    let manager = $state<string>(initial ? (initial.manager ?? "") : defaultManager(store.floorView?.employees ?? []));
    let cli = $state<CliKind>(initial?.cli ?? "claude");
    let model = $state(initial?.model ?? "");
    let effort = $state(initial?.effort ?? "");
    let policy = $state<Policy>(initial?.policy ?? "read_only");
    let roleText = $state(initial?.roleText ?? "");
    let busy = $state(false);

    // A new employee gets the slug from the name. An existing slug never changes.
    const slug = $derived(editing ? initial.slug : uniqueSlug(slugify(name), taken));
    const managers = $derived((store.floorView?.employees ?? []).filter((e) => e.slug !== slug));
    // The model select shows the list of the selected CLI. The effort select shows the efforts of
    // the selected model, or of the floor default model when the model is "floor default".
    const defaults = $derived(store.floorView?.defaults ?? null);
    const list = $derived(listFor(store.models, cli));
    const fallbackModel = $derived(defaultModelFor(defaults, cli));
    const modelOptions = $derived(modelChoices(list, model));
    const effortOptions = $derived(effortChoices(effortList(list, model, fallbackModel), effort));

    onMount(() => {
        // The Codex file can change while the app runs: read the lists again.
        void store.loadModels();
    });

    function changeCli(value: CliKind): void {
        cli = value;
        const next = defaultModelFor(defaults, value);
        const fixed = afterCliChange(listFor(store.models, value), model, effort, next);
        model = fixed.model;
        effort = fixed.effort;
    }

    function changeModel(value: string): void {
        model = value;
        effort = afterModelChange(list, value, effort, fallbackModel);
    }

    // What blocks the button, as text. The operator reads it next to the button and under the field.
    const nameProblem = $derived(
        name.trim() === "" ? "Write a name." : !isValidSlug(slug) ? "The name needs at least one letter or digit." : null,
    );
    const roleProblem = $derived(role.trim() === "" ? "Write a role." : null);
    const valid = $derived(nameProblem === null && roleProblem === null);
    const blocked = $derived(nameProblem ?? roleProblem);

    // A name that is used twice is allowed, but tabs and tags then look alike.
    const sameName = $derived(
        name.trim() === ""
            ? null
            : ((store.floorView?.employees ?? []).find(
                  (e) => e.slug !== slug && e.name.trim().toLowerCase() === name.trim().toLowerCase(),
              ) ?? null),
    );
    const noInstructions = $derived(roleText.trim() === "");

    async function submit(): Promise<void> {
        if (!valid || busy || store.gone) return;
        busy = true;
        const saved = await store.saveEmployee({
            slug,
            name: name.trim(),
            role: role.trim(),
            manager: manager === "" ? null : manager,
            cli,
            model: model === "" ? null : model,
            effort: effort === "" ? null : effort,
            policy,
            roleText,
        });
        busy = false;
        if (saved) onclose();
    }
</script>

<div class="backdrop" role="presentation" use:modal={{ onclose }} onclick={(e) => e.target === e.currentTarget && onclose()}>
    <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
    <form
        class="panel"
        role="dialog"
        aria-modal="true"
        aria-label={editing ? "Edit employee" : "Add employee"}
        onsubmit={(e) => {
            e.preventDefault();
            void submit();
        }}
    >
        <h2>{editing ? "Edit employee" : "Add employee"}</h2>

        <label>
            <span>Name</span>
            <input type="text" name="name" placeholder="Name, for example Lena" autocomplete="off" aria-invalid={nameProblem !== null && name !== ""} bind:value={name} />
        </label>
        {#if name !== "" && nameProblem}
            <p class="problem" role="alert" data-testid="name-problem">{nameProblem}</p>
        {/if}
        {#if sameName}
            <p class="warn" role="status" data-testid="name-duplicate">
                {sameName.name} ({sameName.role}) has this name already. You can save, but the tabs and the name tags will look alike.
            </p>
        {/if}
        <div class="slug" title="The app keeps this employee in the file .swarm/employees/{slug || 'name'}.md. The file name does not change later.">
            File name: <code>{slug ? `${slug}.md` : "(from the name)"}</code>
        </div>

        <div class="two">
            <label>
                <span>Role</span>
                <input type="text" name="role" placeholder="Engineer" autocomplete="off" bind:value={role} />
            </label>
            <label>
                <span>Manager</span>
                <select name="manager" bind:value={manager}>
                    <option value="">(none)</option>
                    {#each managers as m (m.slug)}
                        <option value={m.slug}>{m.name} ({m.role})</option>
                    {/each}
                </select>
            </label>
        </div>

        <div class="two">
            <label>
                <span>CLI</span>
                <select name="cli" value={cli} onchange={(e) => changeCli(e.currentTarget.value as CliKind)}>
                    <option value="claude">claude</option>
                    <option value="codex">codex</option>
                </select>
            </label>
            <label>
                <span>Policy</span>
                <select name="policy" bind:value={policy}>
                    {#each POLICY_IDS as id (id)}
                        <option value={id}>{POLICY_INFO[id].label}</option>
                    {/each}
                </select>
            </label>
        </div>

        <p class="help" data-testid="policy-help">{POLICY_INFO[policy].help}</p>

        <div class="two">
            <label>
                <span>Model</span>
                <select name="model" value={model} onchange={(e) => changeModel(e.currentTarget.value)}>
                    <option value="">{floorDefaultLabel(defaults, cli)}</option>
                    {#each modelOptions as choice (choice.value)}
                        <option value={choice.value}>{choice.label}</option>
                    {/each}
                </select>
            </label>
            <label>
                <span>Effort</span>
                <select name="effort" value={effort} onchange={(e) => (effort = e.currentTarget.value)}>
                    <option value="">default</option>
                    {#each effortOptions as choice (choice.value)}
                        <option value={choice.value}>{choice.label}</option>
                    {/each}
                </select>
            </label>
        </div>

        <label>
            <span>Role text (instructions)</span>
            <textarea name="roleText" rows="6" placeholder="You are the engineer. You write small, tested changes." bind:value={roleText}></textarea>
        </label>
        {#if noInstructions}
            <p class="warn" role="status" data-testid="no-instructions">This employee has no instructions. You can save, but it will not know its job.</p>
        {/if}

        <div class="buttons">
            {#if blocked}
                <span class="reason" data-testid="form-reason">{blocked}</span>
            {/if}
            <button type="submit" class="primary" disabled={!valid || busy || store.gone} title={store.goneTitle()}>{editing ? "save" : "add"}</button>
            <button type="button" onclick={onclose}>cancel</button>
        </div>
    </form>
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
        width: min(520px, 94vw);
        max-height: 92vh;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 12px;
        background: var(--panel);
        border: 1px solid var(--accent);
        border-radius: 6px;
        box-shadow: 0 8px 28px rgba(0, 0, 0, 0.6);
    }

    h2 {
        color: var(--accent);
        font-size: 14px;
    }

    label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        flex: 1;
    }

    label span {
        font-size: 11px;
        color: var(--dim);
        text-transform: uppercase;
        letter-spacing: 0.1em;
    }

    .two {
        display: flex;
        gap: 8px;
    }

    .slug {
        color: var(--dim);
        font-size: 12px;
    }

    code {
        color: var(--accent);
    }

    textarea {
        resize: vertical;
    }

    .buttons {
        display: flex;
        align-items: center;
        gap: 6px;
        justify-content: flex-end;
    }

    .reason {
        margin-right: auto;
        color: var(--danger);
        font-size: 12px;
    }

    .problem {
        margin: -4px 0 0;
        color: var(--danger);
        font-size: 12px;
    }

    .warn {
        margin: 0;
        padding: 4px 8px;
        border: 1px solid var(--st-question);
        border-radius: 6px;
        background: rgba(255, 197, 107, 0.08);
        color: var(--st-question);
        font-size: 12px;
    }

    /* The help line sits under the two rows of selects. */
    .help {
        margin: -2px 0 0;
        color: var(--dim);
        font-size: 12px;
    }

    input[aria-invalid="true"] {
        border-color: var(--danger);
    }
</style>
