<script lang="ts">
  // Side-by-side reviewer for an auto-mode task's worktree branch.
  import { Badge, Banner, Button } from "$lib/ds/index.ts";
  import MergeEditor from "./MergeEditor.svelte";
  import type { DiffFileStatus } from "$lib/diff.ts";
  import { diffStats, type DiffViewerProps } from "$lib/diff.ts";
  let { files, branch, onAction, decision, busy = false }: DiffViewerProps = $props();
  const stats = $derived(diffStats(files));
  const kept = $derived(decision?.action === "keep");
  const STATUS_TONE: Partial<Record<DiffFileStatus, "ok" | "danger">> = { added: "ok", deleted: "danger" };

</script>

<section class="diff" aria-label="Diff reviewer">
  {#if files.length === 0}
    <p class="muted">No changes to review. Auto-mode tasks run in a git worktree; their changes appear here when they finish.</p>
  {:else}
    <header>
      <code>{branch ?? "(no branch)"}</code>
      <span>{files.length} file{files.length === 1 ? "" : "s"}, <span class="add">+{stats.added}</span> <span class="rem">−{stats.removed}</span></span>
    </header>
    {#if decision}
      <Banner tone={kept ? "info" : "ok"}>Changes {decision.label}.</Banner>
    {/if}
    {#if !decision || kept}
      <div class="row" role="group" aria-label="Review actions">
        <Button size="sm" variant="primary" disabled={busy} onclick={() => onAction?.("merge")}>Merge</Button>
        <Button size="sm" variant="danger" disabled={busy} onclick={() => onAction?.("discard")}>Discard</Button>
        <Button size="sm" disabled={busy || kept} onclick={() => onAction?.("keep")}>Keep for later</Button>
      </div>
    {/if}
    {#each files as file (file.path)}
      <details open>
        <summary><Badge size="sm" tone={STATUS_TONE[file.status] ?? "neutral"}>{file.status}</Badge> {file.oldPath ? `${file.oldPath} → ` : ""}{file.path}</summary>
        {#if file.binary}
          <p class="muted">Binary file.</p>
        {:else}
          <MergeEditor hunks={file.hunks} />
        {/if}
      </details>
    {/each}
  {/if}
</section>

<style>
  .diff { display: flex; flex-direction: column; gap: var(--space-2); font-size: var(--font-size-sm); }
  header { display: flex; justify-content: space-between; gap: var(--space-2); flex-wrap: wrap; }
  .add { color: var(--color-ok-text); } .rem { color: var(--color-danger-text); }
  .row { display: flex; gap: var(--space-1-5); flex-wrap: wrap; }
  summary { cursor: pointer; font-family: var(--font-mono); word-break: break-all; }
  summary:focus-visible { outline: none; box-shadow: var(--focus-ring); border-radius: var(--radius-sm); }
</style>
