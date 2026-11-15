<script lang="ts">
  import type { ToolItem } from "$lib/timeline.ts";
  let { item }: { item: ToolItem } = $props();
  const ICON = { running: "…", waiting: "?", ok: "✓", failed: "✗", denied: "⊘" } as const;
</script>

<details class="tool {item.status}">
  <summary aria-label="Tool call {item.tool}: {item.status}">
    <span class="icon" aria-hidden="true">{ICON[item.status]}</span>
    <code>{item.tool}</code>
    {#if item.decision}<span class="muted">· {item.decision}</span>{/if}
  </summary>
  {#if item.reason}<p class="muted">{item.reason}</p>{/if}
  {#if item.output}<pre>{item.output}</pre>{:else}<p class="muted">No output.</p>{/if}
</details>

<style>
  .tool {
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: var(--shadow-sm);
    padding: var(--space-1-5) var(--space-3);
  }
  summary { cursor: pointer; display: flex; gap: var(--space-2); align-items: center; }
  summary:focus-visible { outline: none; box-shadow: var(--focus-ring); border-radius: var(--radius-sm); }
  .running .icon { color: var(--color-info-text); }
  .waiting .icon { color: var(--color-warn-text); }
  .ok .icon { color: var(--color-ok-text); }
  .failed .icon, .denied .icon { color: var(--color-danger-text); }
  pre {
    margin: var(--space-1-5) 0 0;
    padding: var(--space-2);
    background: var(--color-surface-sunken);
    border-radius: var(--radius-md);
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  p { margin: var(--space-1-5) 0 0; }
</style>
