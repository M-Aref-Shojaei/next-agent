<script lang="ts">
  import { app, type RouterHealth } from "$lib/app.svelte.ts";
  import { Badge, StatusDot } from "$lib/ds/index.ts";
  import type { Tone } from "$lib/ds/types.ts";
  import ModeBadge from "./ModeBadge.svelte";

  const LABEL: Record<RouterHealth, string> = {
    ok: "ok", degraded: "degraded", down: "down", unknown: "checking…",
  };
  const TONE: Record<RouterHealth, Tone> = { ok: "ok", degraded: "warn", down: "danger", unknown: "neutral" };
</script>

<footer class="status" aria-label="Status bar">
  <ModeBadge mode={app.mode} workspaceName={app.activeWorkspace?.name ?? ""} onselect={(m) => app.setMode(m)} />
  <span class="item" role="status" title={app.routerDetail} aria-label="Model provider health: {LABEL[app.router]}">
    <StatusDot tone={TONE[app.router]} text="model {LABEL[app.router]}" />
  </span>
  <span class="item">branch {app.branch ?? "—"}</span>
  <span class="item">{app.pendingCount} pending</span>
  {#if app.mode === "bypassPermissions"}
    <span class="bypass" role="alert"><Badge tone="danger" variant="solid">BYPASS ACTIVE — sandbox unlocked</Badge></span>
  {/if}
</footer>

<style>
  .status {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-4);
    padding: var(--space-1) var(--space-3);
    border-top: var(--border-width) solid var(--color-border);
    background: var(--color-surface-sunken);
    font-size: var(--font-size-sm);
  }
  .bypass { margin-left: auto; }
</style>
