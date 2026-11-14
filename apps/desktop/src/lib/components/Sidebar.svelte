<script lang="ts">
  import { app } from "$lib/app.svelte.ts";
  let path = $state("");
  async function open(event: SubmitEvent) {
    event.preventDefault();
    if (await app.openWorkspace(path)) path = "";
  }
</script>

<nav class="sidebar" aria-label="Workspaces and tasks">
  <h2>Workspaces</h2>
  <ul>
    {#each app.workspaces as ws (ws.id)}
      <li>
        <button aria-current={ws.id === app.activeWorkspaceId} title={ws.path} onclick={() => app.selectWorkspace(ws.id)}>
          {ws.name}
        </button>
      </li>
    {:else}
      <li class="muted">No workspace yet</li>
    {/each}
  </ul>
  <form onsubmit={open}>
    <label class="sr-only" for="ws-path">Workspace folder path</label>
    <input id="ws-path" placeholder="/path/to/project" bind:value={path} />
    <button type="submit">Open</button>
  </form>

  <h2>Tasks</h2>
  <ul aria-label="Tasks">
    {#each app.tasks as task (task.id)}
      <li>
        <button aria-current={task.id === app.activeTaskId} onclick={() => app.selectTask(task.id)}>
          <span class="s {task.status}" aria-hidden="true"></span>{#if task.origin}<span class="chan" title={task.origin}>chat</span>{/if}{task.title}
          <span class="sr-only">({task.status})</span>
        </button>
      </li>
    {:else}
      <li class="muted">No tasks yet</li>
    {/each}
  </ul>
  <a class="settings" href="/boards">Boards</a>
  <a class="nav" href="/settings/providers">Providers</a>
  <a class="nav" href="/usage">Usage</a>
  <a class="nav" href="/settings/channels">Channels</a>
  <a class="nav" href="/settings">Settings</a>
</nav>

<style>
  .sidebar { background: var(--sidebar); border-right: 1px solid var(--border); padding: 12px; overflow: auto; display: flex; flex-direction: column; gap: 8px; }
  h2 { font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); margin-top: 8px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  li button { width: 100%; text-align: left; border-color: transparent; background: transparent; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li button[aria-current="true"] { background: var(--panel); border-color: var(--border); font-weight: 600; }
  form { display: flex; gap: 6px; }
  .s { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 6px; background: var(--muted); }
  .s.running { background: var(--info); } .s.completed { background: var(--ok); } .s.failed { background: var(--danger); }
  .settings { margin-top: auto; color: var(--accent); }
  .chan { font-size: 10px; border: 1px solid var(--border); border-radius: 4px; padding: 0 3px; margin-right: 4px; color: var(--muted); }
  .nav { color: var(--accent); }
  nav > a { color: var(--accent); }
</style>
