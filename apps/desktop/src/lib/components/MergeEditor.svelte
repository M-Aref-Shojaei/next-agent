<script lang="ts">
  // Simplified merge editor using the existing sideBySide helper.
  // A full CodeMirror 6 merge view can be added later with proper CSS imports and theme integration.
  import { Badge, Button } from "$lib/ds/index.ts";
  import type { DiffFileStatus } from "$lib/diff.ts";
  import { diffStats, sideBySide, type DiffViewerProps } from "$lib/diff.ts";

  const props = $props<{ oldText: string; newText: string; language?: string; onAccept?: (hunks: number[]) => void; onReject?: (hunks: number[]) => void }>();
  let oldText = $derived(props.oldText);
  let newText = $derived(props.newText);

  const acceptHunks = $state<number[]>([]);
  const rejectHunks = $state<number[]>([]);

  function handleAccept() {
    // Accept all lines in the current view
    props.onAccept?.([...acceptHunks]);
    acceptHunks.length = 0;
  }

  function handleReject() {
    // Reject all lines in the current view
    props.onReject?.([...rejectHunks]);
    rejectHunks.length = 0;
  }

  function toggleAccept(hunkIndex: number) {
    if (acceptHunks.includes(hunkIndex)) {
      acceptHunks.splice(acceptHunks.indexOf(hunkIndex), 1);
    } else {
      acceptHunks.push(hunkIndex);
    }
  }

  function toggleReject(hunkIndex: number) {
    if (rejectHunks.includes(hunkIndex)) {
      rejectHunks.splice(rejectHunks.indexOf(hunkIndex), 1);
    } else {
      rejectHunks.push(hunkIndex);
    }
  }
</script>

<div class="merge-editor">
  <div class="toolbar" role="group" aria-label="Merge controls">
    <Button size="sm" variant="primary" onclick={handleAccept}>Accept all</Button>
    <Button size="sm" variant="danger" onclick={handleReject}>Reject all</Button>
  </div>

  {#each oldText.split("\n") as line, i (i)}
    <div class="hunk-row">
      <span class="line-num">{i + 1}</span>
      <pre class="code accept" on:click={() => toggleAccept(i)}>{line}</pre>
      {#if rejectHunks.includes(i)}<Badge size="sm" tone="danger">Rejected</Badge>{/if}
    </div>
  {/each}

  {#each newText.split("\n") as line, i (i)}
    <div class="hunk-row">
      <span class="line-num">{i + 1}</span>
      <pre class="code reject" on:click={() => toggleReject(i)}>{line}</pre>
      {#if acceptHunks.includes(i)}<Badge size="sm" tone="ok">Accepted</Badge>{/if}
    </div>
  {/each}

  {#if acceptHunks.length > 0 || rejectHunks.length > 0}
    <p class="muted small">
      Accepted: {acceptHunks.length} lines | Rejected: {rejectHunks.length} lines
    </p>
  {/if}
</div>

<style>
  .merge-editor { font-family: var(--font-mono); font-size: var(--font-size-sm); }
  .toolbar { display: flex; gap: var(--space-1); margin-bottom: var(--space-2); }
  .hunk-row { display: flex; align-items: center; gap: var(--space-1); padding: var(--space-0-5) var(--space-1); border-radius: var(--radius-sm); }
  .line-num { color: var(--color-text-tertiary); min-width: 3em; text-align: right; }
  .code { flex: 1; white-space: pre-wrap; margin: 0; padding: var(--space-0-5) var(--space-1); border-radius: var(--radius-sm); cursor: pointer; font-family: inherit; font-size: inherit; }
  .accept { background: var(--color-ok-subtle); color: var(--color-ok-text); }
  .reject { background: var(--color-danger-subtle); color: var(--color-danger-text); }
  .hunk-row:hover .code.accept { background: var(--color-ok); color: var(--color-on-ok); }
  .hunk-row:hover .code.reject { background: var(--color-danger); color: var(--color-on-danger); }
</style>
