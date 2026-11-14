<script lang="ts">
  import { api } from "$lib/api.ts";
  import type { Backlink } from "./types.ts";
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

  const groups = $derived(
    Object.entries(
      rows.reduce<Record<string, Backlink[]>>((acc, r) => ((acc[r.page] ??= []).push(r), acc), {}),
    ),
  );
</script>

<div class="query">
  <div class="head"><span class="tag">query</span> <code>{text}</code> {#if loaded && !error}<span class="count">{rows.length} {rows.length === 1 ? "result" : "results"}</span>{/if}</div>
  {#if error}
    <div class="err" role="alert">{error}</div>
  {:else}
    {#each groups as [page, items] (page)}
      <div class="group">
        <button class="page" onclick={(e) => { e.stopPropagation(); ctx.open(page); }}>{page}</button>
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
  .query { border: 1px solid var(--border); border-radius: 8px; padding: 6px 10px; margin: 4px 0; background: color-mix(in srgb, var(--sidebar) 60%, transparent); }
  .head { font-size: 12px; color: var(--muted); display: flex; gap: 8px; align-items: baseline; flex-wrap: wrap; }
  .head code { font-size: 12px; }
  .tag { text-transform: uppercase; letter-spacing: 0.06em; font-size: 10px; border: 1px solid var(--border); border-radius: 4px; padding: 0 4px; }
  .count { margin-left: auto; }
  .group { margin-top: 6px; }
  .page { all: unset; cursor: pointer; font-weight: 600; font-size: 12px; color: var(--accent); }
  .page:hover { text-decoration: underline; }
  .hit { padding-left: 10px; border-left: 2px solid var(--border); margin: 2px 0; }
  .err { color: var(--danger); font-size: 12px; margin-top: 4px; }
</style>
