<script lang="ts">
  // One generated direction in the grid: sandboxed thumbnail, rationale,
  // critique scores, lint findings and the pick button.
  import DesignFrame from "$lib/components/DesignFrame.svelte";
  import { Badge, Button, Card } from "$lib/ds/index.ts";
  import { DIRECTION_SANDBOX, averageScore, type DesignDirection } from "$lib/design.ts";

  let { d, onpick }: { d: DesignDirection; onpick: (d: DesignDirection) => void } = $props();

  const score = $derived(averageScore(d.critique));
  const p0 = $derived(d.lint.filter((f) => f.severity === "p0").length);
</script>

<li class="direction" class:chosen={d.chosen}>
  <Card padding="sm">
    <div class="body">
      <DesignFrame frame={d.frame} title="Thumbnail: {d.title}" sandbox={DIRECTION_SANDBOX} srcdoc={d.srcdoc} thumb />
      <div class="meta">
        <h3>{d.title}{#if d.chosen} <Badge tone="accent" size="sm">Chosen</Badge>{/if}</h3>
        {#if d.rationale}<p class="muted small">{d.rationale}</p>{/if}
        {#if d.critique}
          <div class="scores" title={d.critique.notes}>
            <span class="score">{score}/5{#if d.critique.revised} · revised{/if}</span>
            {#each d.critique.scores as [dim, s] (dim)}
              <span class="dim" title="{dim}: {s}/5"><span class="sr-only">{dim} {s} of 5</span><i style:height="{s * 20}%" class:low={s < 3}></i></span>
            {/each}
          </div>
        {/if}
        {#if d.lint.length}
          <details class="lint"><summary>{p0 ? `${p0} issue${p0 > 1 ? "s" : ""}` : `${d.lint.length} note${d.lint.length > 1 ? "s" : ""}`}</summary>
            <ul>{#each d.lint as f (f.rule)}<li><Badge tone={f.severity === "p0" ? "danger" : "warn"} size="sm">{f.severity.toUpperCase()}</Badge> {f.message}</li>{/each}</ul>
          </details>
        {/if}
        <Button variant="primary" onclick={() => onpick(d)}>Pick this direction</Button>
      </div>
    </div>
  </Card>
</li>

<style>
  .direction { list-style: none; border-radius: var(--radius-lg); }
  .direction.chosen { outline: var(--border-width-strong) solid var(--color-accent); }
  .body { display: flex; flex-direction: column; gap: var(--space-2); }
  .meta { display: grid; gap: var(--space-1-5); }
  .meta h3 { font-size: var(--font-size-md); }
  .small { font-size: var(--font-size-sm); margin: 0; }
  .scores { display: flex; align-items: flex-end; gap: var(--space-0-5); height: 22px; }
  .score { font-size: var(--font-size-sm); margin-right: var(--space-1-5); align-self: center; }
  .dim { width: var(--space-1-5); height: 100%; background: var(--color-surface-sunken); border-radius: var(--radius-xs); display: flex; align-items: flex-end; overflow: hidden; }
  .dim i { width: 100%; background: var(--color-ok); display: block; }
  .dim i.low { background: var(--color-warn); }
  .lint { font-size: var(--font-size-sm); }
  .lint ul { margin: var(--space-1) 0 0; padding-left: var(--space-4); }
</style>
