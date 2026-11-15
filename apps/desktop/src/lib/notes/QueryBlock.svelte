<script lang="ts">
  import { api } from "$lib/api.ts";
  import { Badge, Banner, Button } from "$lib/ds/index.ts";
  import type { Backlink } from "./types.ts";
  import { groupByPage } from "./group.ts";
  import BlockBody from "./BlockBody.svelte";
  import { useNotes } from "./ctx.ts";

  let { text }: { text: string } = $props();
  const ctx = useNotes();
  let rows = $state.raw<Backlink[]>([]);
  let error = $state("");
  let loaded = $state(false);

  $effect(() => {
    ctx.version();
    const q = text;
    let live = true;
    void api.graphQuery(q, ctx.today()).then(
      (r) => {
        if (!live) return;
        rows = r;
        error = "";
        loaded = true;
      },
      (e) => {
        if (!live) return;
        error = String(e instanceof Error ? e.message : e);
        loaded = true;
      },
    );
    return () => (live = false);
  });

  const groups = $derived(groupByPage(rows));
</script>

<div class="query">
  <div class="head"><Badge size="sm">query</Badge> <code>{text}</code> {#if loaded && !error}<span class="count">{rows.length} {rows.length === 1 ? "result" : "results"}</span>{/if}</div>
  {#if error}
    <Banner tone="danger">{error}</Banner>
  {:else}
    {#each groups as [page, items] (page)}
      <div class="group">
        <Button variant="ghost" size="sm" onclick={(e) => { e.stopPropagation(); ctx.open(page); }}>{page}</Button>
        {#each items as r (r.blockId)}
          <div class="hit"><BlockBody content={r.content} /></div>
        {/each}
      </div>
    {:else}
      {#if loaded}<div class="muted">No matching blocks.</div>{/if}
    {/each}
  {/if}
</div>

<style>
  .query { border: var(--border-width) solid var(--color-border); border-radius: var(--radius-lg); padding: var(--space-1-5) var(--space-3); margin: var(--space-1) 0; background: var(--color-surface-sunken); }
  .head { font-size: var(--font-size-sm); color: var(--color-text-secondary); display: flex; gap: var(--space-2); align-items: baseline; flex-wrap: wrap; }
  .count { margin-left: auto; }
  .group { margin-top: var(--space-1-5); }
  .hit { padding-left: var(--space-3); border-left: var(--border-width-strong) solid var(--color-border); margin: var(--space-0-5) 0; }
</style>
