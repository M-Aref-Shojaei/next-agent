<script lang="ts">
  // STUB: renders a plain-text summary. The CodeMirror 6 merge view and
  // Merge / Discard / Keep actions get wired once worktree isolation (Step 4) merges.
  import { diffStats, type DiffViewerProps } from "$lib/diff.ts";
  let { files, branch, onAction }: DiffViewerProps = $props();
  const stats = $derived(diffStats(files));
</script>

<section aria-label="Diff reviewer (stub)">
  {#if files.length === 0}
    <p class="muted">No changes to review. The side-by-side diff appears here for Auto-mode worktrees.</p>
  {:else}
    <p>{branch ?? "(no branch)"} — {files.length} file(s), +{stats.added} −{stats.removed}</p>
    <ul>{#each files as file (file.path)}<li>{file.status}: {file.path}</li>{/each}</ul>
    <div class="row">
      <button onclick={() => onAction?.("merge")}>Merge</button>
      <button onclick={() => onAction?.("discard")}>Discard</button>
      <button onclick={() => onAction?.("keep")}>Keep</button>
    </div>
  {/if}
</section>
