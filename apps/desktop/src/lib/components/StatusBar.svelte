<script lang="ts">
  import { app, type RouterHealth } from "$lib/app.svelte.ts";
  import ModeBadge from "./ModeBadge.svelte";

  const LABEL: Record<RouterHealth, string> = {
    ok: "ok", degraded: "degraded", down: "down", unknown: "checking…",
  };
</script>

<footer class="status" aria-label="Status bar">
  <ModeBadge mode={app.mode} workspaceName={app.activeWorkspace?.name ?? ""} onselect={(m) => app.setMode(m)} />
  <span class="item" role="status" title={app.routerDetail} aria-label="Router health: {LABEL[app.router]}">
    <span class="dot {app.router}" aria-hidden="true"></span> router {LABEL[app.router]}
  </span>
  <span class="item">branch {app.branch ?? "—"}</span>
  <span class="item">{app.pendingCount} pending</span>
  {#if app.mode === "bypassPermissions"}
    <span class="banner" role="alert">BYPASS ACTIVE — sandbox unlocked</span>
  {/if}
</footer>

<style>
  .status {
    grid-column: 1 / -1; display: flex; align-items: center; gap: 16px; padding: 4px 12px;
    border-top: 1px solid var(--border); background: var(--sidebar); font-size: 12px;
  }
  .dot { display: inline-block; width: 9px; height: 9px; border-radius: 50%; background: var(--muted); }
  .dot.ok { background: var(--ok); } .dot.degraded { background: var(--warn); } .dot.down { background: var(--danger); }
  .banner { margin-left: auto; background: var(--danger); color: #fff; padding: 1px 10px; border-radius: 4px; font-weight: 700; }
</style>
