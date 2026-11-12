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
  .tool { border: 1px solid var(--border); border-radius: 8px; background: var(--panel); padding: 6px 10px; }
  summary { cursor: pointer; display: flex; gap: 8px; align-items: center; }
  .running .icon { color: var(--info); } .waiting .icon { color: var(--warn); } .ok .icon { color: var(--ok); }
  .failed .icon, .denied .icon { color: var(--danger); }
  pre { margin: 6px 0 0; padding: 8px; background: var(--bg); border-radius: 6px; overflow: auto; white-space: pre-wrap; }
  p { margin: 6px 0 0; }
</style>
