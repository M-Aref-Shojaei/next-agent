<script lang="ts">
  // Side-by-side reviewer for an auto-mode task's worktree branch.
  import { diffStats, sideBySide, type DiffViewerProps } from "$lib/diff.ts";
  let { files, branch, onAction, outcome }: DiffViewerProps = $props();
  const stats = $derived(diffStats(files));
  let busy = $state(false);

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
      <p class="outcome" role="status">Changes {outcome}.</p>
    {:else}
      <div class="row" role="group" aria-label="Review actions">
        <button class="primary" disabled={busy} onclick={() => act("merge")}>Merge</button>
        <button class="danger" disabled={busy} onclick={() => act("discard")}>Discard</button>
        <button disabled={busy} onclick={() => act("keep")}>Keep for later</button>
      </div>
    {/if}
    {#each files as file (file.path)}
      <details open>
        <summary><span class="status {file.status}">{file.status}</span> {file.oldPath ? `${file.oldPath} → ` : ""}{file.path}</summary>
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
  .diff { display: flex; flex-direction: column; gap: 8px; font-size: 12px; }
  header { display: flex; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .add { color: var(--ok); } .rem { color: var(--danger); }
  .row { display: flex; gap: 6px; flex-wrap: wrap; }
  .outcome { color: var(--ok); margin: 0; }
  summary { cursor: pointer; font-family: ui-monospace, monospace; word-break: break-all; }
  .status { font-family: inherit; font-size: 11px; padding: 0 4px; border-radius: 3px; background: var(--border); }
  .status.added { background: var(--ok); color: #fff; } .status.deleted { background: var(--danger); color: #fff; }
  .hunk { width: 100%; border-collapse: collapse; table-layout: fixed; margin-top: 4px; font-family: ui-monospace, monospace; }
  .num { width: 2.5em; color: var(--muted); text-align: right; padding-right: 4px; user-select: none; vertical-align: top; }
  .code { white-space: pre-wrap; word-break: break-all; padding: 0 4px; vertical-align: top; }
  .code.add { background: color-mix(in srgb, var(--ok) 18%, transparent); }
  .code.remove { background: color-mix(in srgb, var(--danger) 18%, transparent); }
  .code.blank { background: color-mix(in srgb, var(--border) 40%, transparent); }
</style>
