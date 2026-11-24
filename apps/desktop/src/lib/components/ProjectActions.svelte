<script lang="ts">
  import { tick } from "svelte";
  import { api } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
  import { Button, TextField } from "$lib/ds/index.ts";

  let mode = $state<"open" | "create" | null>(null);
  let busy = $state(false);
  let path = $state("");
  let parent = $state("");
  let name = $state("");
  let pathInput = $state<HTMLInputElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);

  async function pickFolder(): Promise<string | null> {
    try {
      return await api.showOpenFilePicker();
    } catch {
      return null;
    }
  }

  async function openProject() {
    if (busy) return;
    mode = "open";
    busy = true;
    try {
      const selected = await pickFolder();
      if (selected) {
        path = selected;
        if (await app.openWorkspace(path)) reset();
      }
    } finally {
      busy = false;
    }
    await tick();
    pathInput?.focus();
  }

  async function startCreate() {
    mode = "create";
    await tick();
    nameInput?.focus();
  }

  async function browseParent() {
    if (busy) return;
    busy = true;
    try {
      const selected = await pickFolder();
      if (selected) parent = selected;
    } finally {
      busy = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !mode) return;
    busy = true;
    try {
      const success = mode === "open" ? await app.openWorkspace(path) : await app.createWorkspace(parent, name);
      if (success) reset();
    } finally {
      busy = false;
    }
  }

  function reset() {
    mode = null;
    path = "";
    parent = "";
    name = "";
  }
</script>

<div class="actions">
  <Button size="sm" disabled={busy} onclick={openProject}>Open Project</Button>
  <Button size="sm" disabled={busy} onclick={startCreate}>New Project</Button>
</div>

{#if mode}
  <form onsubmit={submit}>
    {#if mode === "open"}
      <TextField size="sm" label="Workspace folder path" placeholder="/path/to/project" mono bind:value={path} bind:ref={pathInput} required disabled={busy} />
    {:else}
      <TextField size="sm" label="Project name" bind:value={name} bind:ref={nameInput} required disabled={busy} />
      <TextField size="sm" label="Parent folder path" placeholder="/path/to/projects" mono bind:value={parent} required disabled={busy} />
      <Button size="sm" disabled={busy} onclick={browseParent}>Browse for parent folder</Button>
    {/if}
    <div class="actions">
      <Button size="sm" variant="primary" type="submit" disabled={busy || (mode === "open" ? !path.trim() : !name.trim() || !parent.trim())}>
        {mode === "open" ? "Open" : "Create project"}
      </Button>
      <Button size="sm" variant="ghost" disabled={busy} onclick={reset}>Cancel</Button>
    </div>
  </form>
{/if}

<style>
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-1); }
  form { min-width: 0; display: grid; gap: var(--space-2); }
</style>
