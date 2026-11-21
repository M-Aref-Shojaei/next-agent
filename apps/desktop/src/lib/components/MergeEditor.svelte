<script lang="ts">
  import { Badge } from "$lib/ds/index.ts";
  import { sideBySide, type DiffHunk, type SideCell } from "$lib/diff.ts";

  type Props = { hunks: DiffHunk[] };
  const props: Props = $props();

  function cellTone(cell: SideCell): "ok" | "danger" | "neutral" {
    if (cell.kind === "add") return "ok";
    if (cell.kind === "remove") return "danger";
    return "neutral";
  }
</script>

<div class="merge-editor" aria-label="Read-only diff">
  {#each props.hunks as hunk, hunkIndex (hunkIndex)}
    <section class="hunk" aria-label={`Hunk ${hunkIndex + 1}`}>
      <header>
        <code>@@ -{hunk.oldStart},{hunk.oldLines} +{hunk.newStart},{hunk.newLines} @@</code>
      </header>
      <div class="rows">
        {#each sideBySide(hunk) as row, rowIndex (rowIndex)}
          <div class="diff-row">
            {#if row.left}
              <div class:removed={row.left.kind === "remove"} class="cell">
                <span class="line-num">{row.left.line}</span>
                <span class="code">{row.left.text}</span>
                {#if row.left.kind === "remove"}<Badge size="sm" tone={cellTone(row.left)}>−</Badge>{/if}
              </div>
            {:else}
              <div class="cell empty" aria-hidden="true"><span class="line-num"> </span></div>
            {/if}
            {#if row.right}
              <div class:added={row.right.kind === "add"} class="cell">
                <span class="line-num">{row.right.line}</span>
                <span class="code">{row.right.text}</span>
                {#if row.right.kind === "add"}<Badge size="sm" tone={cellTone(row.right)}>+</Badge>{/if}
              </div>
            {:else}
              <div class="cell empty" aria-hidden="true"><span class="line-num"> </span></div>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  .merge-editor { font-family: var(--font-mono); font-size: var(--font-size-sm); }
  .hunk { margin-top: var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius-sm); overflow: hidden; }
  .hunk:first-child { margin-top: 0; }
  header { padding: var(--space-1) var(--space-2); background: var(--color-surface-subtle); color: var(--color-text-secondary); }
  .diff-row { display: grid; grid-template-columns: 1fr 1fr; border-top: 1px solid var(--color-border-subtle); }
  .cell { display: flex; align-items: flex-start; gap: var(--space-1); min-width: 0; padding: var(--space-0-5) var(--space-1); }
  .cell + .cell { border-left: 1px solid var(--color-border-subtle); }
  .line-num { flex: 0 0 3em; color: var(--color-text-tertiary); text-align: right; user-select: none; }
  .code { flex: 1; min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .added { background: var(--color-ok-subtle); color: var(--color-ok-text); }
  .removed { background: var(--color-danger-subtle); color: var(--color-danger-text); }
  .empty { background: var(--color-surface-subtle); }
</style>
