<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { app } from "$lib/app.svelte.ts";
  import { upstreams } from "$lib/upstreams.svelte.ts";
  import { Badge, Button, List, ListRow, StatusDot, TextField } from "$lib/ds/index.ts";
  import type { Tone } from "$lib/ds/types.ts";
  onMount(() => void upstreams.init());
  let path = $state("");
  async function open(event: SubmitEvent) {
    event.preventDefault();
    if (await app.openWorkspace(path)) path = "";
  }

  const TASK_TONE: Record<string, Tone> = { running: "info", completed: "ok", failed: "danger" };
  const LINKS = [
    { href: "/notes", label: "Notes" },
    { href: "/boards", label: "Boards" },
    { href: "/design", label: "Design" },
    { href: "/settings/providers", label: "Providers" },
    { href: "/usage", label: "Usage" },
    { href: "/settings/channels", label: "Channels" },
    { href: "/settings/learning", label: "Memory and skills" },
  ];
</script>

<nav class="sidebar" aria-label="Workspaces and tasks">
  <h2>Workspaces</h2>
  {#if app.workspaces.length}
    <List bordered={false} label="Workspaces">
      {#each app.workspaces as ws (ws.id)}
        <ListRow title={ws.name} tooltip={ws.path} selectedAs="current" selected={ws.id === app.activeWorkspaceId} onclick={() => app.selectWorkspace(ws.id)} />
      {/each}
    </List>
  {:else}
    <p class="muted">No workspace yet</p>
  {/if}
  <form onsubmit={open}>
    <TextField size="sm" label="Workspace folder path" hideLabel placeholder="/path/to/project" mono bind:value={path} />
    <Button size="sm" type="submit">Open</Button>
  </form>

  <h2>Tasks</h2>
  {#if app.tasks.length}
    <List bordered={false} label="Tasks">
      {#each app.tasks as task (task.id)}
        <ListRow title={task.title} selectedAs="current" selected={task.id === app.activeTaskId} onclick={() => app.selectTask(task.id)}>
          {#snippet leading()}<StatusDot tone={TASK_TONE[task.status] ?? "neutral"} label={task.status} />{/snippet}
          {#if task.origin}<Badge size="sm">{task.origin}</Badge>{/if}
        </ListRow>
      {/each}
    </List>
  {:else}
    <p class="muted">No tasks yet</p>
  {/if}

  <div class="links">
    <List bordered={false} label="Pages">
      {#each LINKS as link (link.href)}
        <ListRow title={link.label} href={link.href} selected={page.url.pathname === link.href} />
      {/each}
      <ListRow title="Settings" href="/settings" selected={page.url.pathname === "/settings"}>
        {#if upstreams.unseen > 0}
          <Badge tone="accent" variant="solid" size="sm">{upstreams.unseen}<span class="sr-only"> new upstream changes</span></Badge>
        {/if}
      </ListRow>
    </List>
  </div>
</nav>

<style>
  .sidebar {
    flex: 1;
    min-width: 0;
    background: var(--color-surface-sunken);
    border-right: var(--border-width) solid var(--color-border);
    padding: var(--space-3);
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  h2 {
    margin-top: var(--space-2);
    font-size: var(--font-size-xs);
    text-transform: uppercase;
    letter-spacing: var(--letter-spacing-caps);
    color: var(--color-text-secondary);
  }
  p { margin: 0; font-size: var(--font-size-sm); }
  form { display: flex; gap: var(--space-1-5); align-items: center; }
  form :global(.ds-input) { min-width: 0; }
  .links { margin-top: auto; }
</style>
