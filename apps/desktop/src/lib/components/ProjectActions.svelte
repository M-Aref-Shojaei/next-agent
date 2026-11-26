<script lang="ts">
  import { tick } from "svelte";
  import { api } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
  import { Banner, Button, TextField } from "$lib/ds/index.ts";

  type Action = "open" | "create";

  let mode = $state<Action | null>(null);
  let progress = $state("");
  let error = $state<string | null>(null);
  let path = $state("");
  let parent = $state("");
  let name = $state("");
  let pathInput = $state<HTMLInputElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
  let parentInput = $state<HTMLInputElement | null>(null);
  let form = $state<HTMLFormElement | null>(null);
  let launchers = $state<HTMLDivElement | null>(null);
  const busy = $derived(progress !== "");

  async function pickFolder(): Promise<string | null> {
    progress = "Choosing folder…";
    try {
      return await api.showOpenFilePicker();
    } catch {
      // Manual paths also work on platforms without a folder picker.
      return null;
    } finally {
      progress = "";
    }
  }

  async function openProject() {
    mode = "open";
    const selected = await pickFolder();
    if (selected) {
      path = selected;
      await register("open");
    } else {
      await tick();
      if (formOwnsFocus()) pathInput?.focus();
    }
  }

  async function startCreate() {
    mode = "create";
    await tick();
    nameInput?.focus();
  }

  async function browseParent() {
    if (busy) return;
    const selected = await pickFolder();
    if (selected) parent = selected;
    await tick();
    if (formOwnsFocus()) parentInput?.focus();
  }

  async function register(action: Action) {
    error = null;
    progress = action === "open" ? "Opening project…" : "Creating project…";
    try {
      error = action === "open" ? await app.openWorkspace(path) : await app.createWorkspace(parent, name);
    } finally {
      progress = "";
    }
    if (!error) {
      await closeForm(action);
    } else {
      await tick();
      if (formOwnsFocus()) (action === "open" ? pathInput : nameInput)?.focus();
    }
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (mode && !busy) void register(mode);
  }

  function formOwnsFocus(): boolean {
    const active = document.activeElement;
    return active === document.body || !!form?.contains(active);
  }

  async function closeForm(action: Action) {
    const restoreFocus = formOwnsFocus();
    mode = null;
    error = null;
    path = "";
    parent = "";
    name = "";
    await tick();
    if (restoreFocus && formOwnsFocus()) {
      launchers?.querySelector<HTMLButtonElement>(`[data-project-action="${action}"]`)?.focus();
    }
  }
</script>

{#if mode}
  <form bind:this={form} aria-labelledby="project-action-title" aria-busy={busy} onsubmit={submit}>
    <h3 id="project-action-title">{mode === "open" ? "Open Project" : "New Project"}</h3>
    {#if error}
      <div class="form-error" dir="auto"><Banner tone="danger" compact>{error}</Banner></div>
    {/if}
    {#if mode === "open"}
      <TextField size="sm" label="Workspace folder path" name="project-path" data-project-entry placeholder="/path/to/project…" autocomplete="off"
        spellcheck={false} dir="ltr" mono bind:value={path} bind:ref={pathInput} required disabled={busy} />
    {:else}
      <TextField size="sm" label="Project name" name="project-name" data-project-entry autocomplete="off" dir="auto"
        bind:value={name} bind:ref={nameInput} required disabled={busy} />
      <div class="folder">
        <TextField size="sm" label="Parent folder path" name="project-parent" placeholder="/path/to/projects…" autocomplete="off"
          spellcheck={false} dir="ltr" mono bind:value={parent} bind:ref={parentInput} required disabled={busy} />
        <Button variant="secondary" size="sm" disabled={busy} onclick={browseParent}>Browse for parent folder</Button>
      </div>
    {/if}
    <div class="actions">
      <Button size="sm" variant="primary" type="submit" disabled={busy || (mode === "open" ? !path.trim() : !name.trim() || !parent.trim())}>
        {mode === "open" ? "Open" : "Create project"}
      </Button>
      <Button size="sm" variant="ghost" disabled={busy} onclick={() => mode && void closeForm(mode)}>Cancel</Button>
    </div>
  </form>
  <div class="progress" role="status">{progress}</div>
{:else}
  <div class="actions" bind:this={launchers}>
    <Button size="sm" variant="primary" data-project-action="open" data-project-entry onclick={openProject}>Open Project</Button>
    <Button size="sm" data-project-action="create" onclick={startCreate}>New Project</Button>
  </div>
{/if}

<style>
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-1); }
  form, .folder { display: grid; min-inline-size: 0; gap: var(--space-2); }
  .folder { justify-items: start; }
  .folder :global(.ds-field) { inline-size: 100%; min-inline-size: 0; }
  h3 { margin: 0; font-size: var(--font-size-sm); font-weight: var(--font-weight-semibold); }
  .form-error { overflow-wrap: anywhere; }
  .progress { color: var(--color-text-secondary); font-size: var(--font-size-sm); }
  .progress:empty { position: absolute; }
</style>
