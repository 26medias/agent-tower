<script lang="ts">
    import { untrack } from "svelte";
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
    let manager = $state<string>(initial?.manager ?? "");
    let cli = $state<CliKind>(initial?.cli ?? "claude");
    let model = $state(initial?.model ?? "");
    let effort = $state(initial?.effort ?? "");
    let policy = $state<Policy>(initial?.policy ?? "read_only");
    let roleText = $state(initial?.roleText ?? "");
    let busy = $state(false);

    // A new employee gets the slug from the name. An existing slug never changes.
    const slug = $derived(editing ? initial.slug : uniqueSlug(slugify(name), taken));
    const managers = $derived((store.floorView?.employees ?? []).filter((e) => e.slug !== slug));
    const modelHint = $derived(cli === "claude" ? "haiku" : "gpt-6-luna");
    const valid = $derived(name.trim() !== "" && role.trim() !== "" && isValidSlug(slug));

    async function submit(): Promise<void> {
        if (!valid || busy) return;
        busy = true;
        const saved = await store.saveEmployee({
            slug,
            name: name.trim(),
            role: role.trim(),
            manager: manager === "" ? null : manager,
            cli,
            model: model.trim() === "" ? null : model.trim(),
            effort: effort.trim() === "" ? null : effort.trim(),
            policy,
            roleText,
        });
        busy = false;
        if (saved) onclose();
    }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
    <form
        class="panel"
        aria-label={editing ? "Edit employee" : "Add employee"}
        onsubmit={(e) => {
            e.preventDefault();
            void submit();
        }}
    >
        <h2>{editing ? "Edit employee" : "Add employee"}</h2>

        <label>
            <span>Name</span>
            <input type="text" name="name" placeholder="Maya" autocomplete="off" bind:value={name} />
        </label>
        <div class="slug">slug: <code>{slug || "(from the name)"}</code></div>

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
                <select name="cli" bind:value={cli}>
                    <option value="claude">claude</option>
                    <option value="codex">codex</option>
                </select>
            </label>
            <label>
                <span>Policy</span>
                <select name="policy" bind:value={policy}>
                    <option value="read_only">read_only</option>
                    <option value="edit_in_worktree">edit_in_worktree</option>
                    <option value="ask_always">ask_always</option>
                </select>
            </label>
        </div>

        <div class="two">
            <label>
                <span>Model</span>
                <input type="text" name="model" placeholder={modelHint} autocomplete="off" bind:value={model} />
            </label>
            <label>
                <span>Effort</span>
                <input type="text" name="effort" placeholder="medium" autocomplete="off" bind:value={effort} />
            </label>
        </div>

        <label>
            <span>Role text (instructions)</span>
            <textarea name="roleText" rows="6" placeholder="You are the engineer. You write small, tested changes." bind:value={roleText}></textarea>
        </label>

        <div class="buttons">
            <button type="submit" class="primary" disabled={!valid || busy}>{editing ? "save" : "add"}</button>
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
        background: rgba(8, 7, 14, 0.7);
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
        border: 2px solid var(--accent);
        border-radius: 2px;
        box-shadow: 6px 6px 0 rgba(0, 0, 0, 0.6);
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
        gap: 6px;
        justify-content: flex-end;
    }
</style>
