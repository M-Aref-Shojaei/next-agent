<script lang="ts">
  import type { ApprovalChoice, ApprovalItem } from "$lib/timeline.ts";
  import { Button, Card } from "$lib/ds/index.ts";
  let { item, onrespond }: { item: ApprovalItem; onrespond: (choice: ApprovalChoice) => void } = $props();
</script>

<section class="approval" role="group" aria-label="Approval needed for {item.tool}">
  <Card padding="sm">
    <header><strong>Approval needed</strong> <code>{item.tool}</code></header>
    <p class="summary">{item.summary}</p>
    <p class="muted">{item.reason}</p>
    <div class="row">
      <Button variant="primary" onclick={() => onrespond("allow")}>Allow</Button>
      <Button variant="danger" onclick={() => onrespond("deny")}>Deny</Button>
      {#if item.canAlways}
        <Button onclick={() => onrespond("always")}>Always for this workspace</Button>
      {/if}
    </div>
  </Card>
</section>

<style>
  /* The warn bar marks it as waiting on the user. */
  .approval :global(.ds-card) { border-left: var(--border-width-strong) solid var(--color-warn); }
  .summary { font-family: var(--font-mono); margin: var(--space-1-5) 0 var(--space-0-5); overflow-wrap: anywhere; }
  p { margin: var(--space-0-5) 0; }
  .row { display: flex; gap: var(--space-2); margin-top: var(--space-2); flex-wrap: wrap; }
</style>
