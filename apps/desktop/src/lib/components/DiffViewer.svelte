<script lang="ts">
  // Side-by-side reviewer for an auto-mode task's worktree branch.
  import { Badge, Banner, Button } from "$lib/ds/index.ts";
  import type { DiffFileStatus } from "$lib/diff.ts";
  import { diffStats, sideBySide, type DiffViewerProps } from "$lib/diff.ts";
  let { files, branch, onAction, outcome }: DiffViewerProps = $props();
  const stats = $derived(diffStats(files));
  let busy = $state(false);
  const STATUS_TONE: Partial<Record<DiffFileStatus, "ok" | "danger">> = { added: "ok", deleted: "danger" };

  async function act(action: "merge" | "discard" | "keep") {
    busy = true;
    try {
      await onAction?.(action);
    } finally {
      busy = false;
    }
  }
</script>

<section class="diff" aria-label="Diff reviewer">
  {#if files.length === 0}
    <p class="muted">No changes to review. Auto-mode tasks run in a git worktree; their changes appear here when they finish.</p>
  {:else}
    <header>
      <code>{branch ?? "(no branch)"}</code>
      <span>{files.length} file{files.length === 1 ? "" : "s"}, <span class="add">+{stats.added}</span> <span class="rem">−{stats.removed}</span></span>
    </header>
    {#if outcome}
      <Banner tone="ok">Changes {outcome}.</Banner>
    {:else}
      <div class="row" role="group" aria-label="Review actions">
        <Button size="sm" variant="primary" disabled={busy} onclick={() => act("merge")}>Merge</Button>
        <Button size="sm" variant="danger" disabled={busy} onclick={() => act("discard")}>Discard</Button>
        <Button size="sm" disabled={busy} onclick={() => act("keep")}>Keep for later</Button>
      </div>
    {/if}
    {#each files as file (file.path)}
      <details open>
        <summary><Badge size="sm" tone={STATUS_TONE[file.status] ?? "neutral"}>{file.status}</Badge> {file.oldPath ? `${file.oldPath} → ` : ""}{file.path}</summary>
        {#if file.binary}
          <p class="muted">Binary file.</p>
        {:else}
          {#each file.hunks as hunk, h (h)}
            <table class="hunk" aria-label="Changes at line {hunk.newStart}">
              <tbody>
                {#each sideBySide(hunk) as row, r (r)}
                  <tr>
                    <td class="num">{row.left?.line ?? ""}</td>
                    <td class="code {row.left?.kind ?? 'blank'}">{row.left?.text ?? ""}</td>
                    <td class="num">{row.right?.line ?? ""}</td>
                    <td class="code {row.right?.kind ?? 'blank'}">{row.right?.text ?? ""}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/each}
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
  .hunk { width: 100%; border-collapse: collapse; table-layout: fixed; margin-top: var(--space-1); font-family: var(--font-mono); }
  .num { width: 2.5em; color: var(--color-text-tertiary); text-align: right; padding-right: var(--space-1); user-select: none; vertical-align: top; }
  .code { white-space: pre-wrap; word-break: break-all; padding: 0 var(--space-1); vertical-align: top; }
  .code.add { background: var(--color-ok-subtle); }
  .code.remove { background: var(--color-danger-subtle); }
  .code.blank { background: var(--color-surface-sunken); }
</style>
