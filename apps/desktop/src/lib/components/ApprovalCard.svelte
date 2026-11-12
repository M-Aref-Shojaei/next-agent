<script lang="ts">
  import type { ApprovalChoice, ApprovalItem } from "$lib/timeline.ts";
  let { item, onrespond }: { item: ApprovalItem; onrespond: (choice: ApprovalChoice) => void } = $props();
</script>

<section class="card" role="group" aria-label="Approval needed for {item.tool}">
  <header><strong>Approval needed</strong> <code>{item.tool}</code></header>
  <p class="summary">{item.summary}</p>
  <p class="muted">{item.reason}</p>
  <div class="row">
    <button class="primary" onclick={() => onrespond("allow")}>Allow</button>
    <button class="danger" onclick={() => onrespond("deny")}>Deny</button>
    <button onclick={() => onrespond("always")}>Always for this workspace</button>
  </div>
</section>

<style>
  .card { border: 1px solid var(--warn); border-left-width: 4px; border-radius: 8px; padding: 10px 12px; background: var(--panel); }
  .summary { font-family: ui-monospace, monospace; margin: 6px 0 2px; word-break: break-all; }
  p { margin: 2px 0; }
  .row { display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap; }
</style>
