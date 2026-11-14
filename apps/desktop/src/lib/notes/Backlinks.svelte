<script lang="ts">
  import { api } from "$lib/api.ts";
  import type { Backlink, Backlinks } from "./types.ts";
  import BlockBody from "./BlockBody.svelte";
  import { useNotes } from "./ctx.ts";

  let { name }: { name: string } = $props();
  const ctx = useNotes();
  let refs = $state.raw<Backlinks>({ linked: [], unlinked: [] });
  let error = $state("");
  let showUnlinked = $state(false);

  $effect(() => {
    ctx.version();
    const target = name;
    let live = true;
    void api.graphBacklinks(target).then(
      (r) => live && ((refs = r), (error = "")),
      (e) => live && (error = String(e instanceof Error ? e.message : e)),
    );
    return () => (live = false);
  });

  const group = (rows: Backlink[]) =>
    Object.entries(rows.reduce<Record<string, Backlink[]>>((acc, r) => ((acc[r.page] ??= []).push(r), acc), {}));
  const linked = $derived(group(refs.linked));
  const unlinked = $derived(group(refs.unlinked));
</script>

<section class="refs" aria-label="References">
  {#if error}<div class="err" role="alert">{error}</div>{/if}
  <h3>Linked references <span class="count">{refs.linked.length}</span></h3>
  {#each linked as [page, items] (page)}
    <div class="group">
      <button class="page" onclick={() => ctx.open(page)}>{page}</button>
      {#each items as r (r.blockId)}
        <div class="hit">
          {#if r.breadcrumb.length}<div class="crumb">{r.breadcrumb.join(" › ")}</div>{/if}
          <BlockBody content={r.content} />
        </div>
      {/each}
    </div>
  {:else}
    <p class="muted">Nothing links here yet.</p>
  {/each}

  {#if refs.unlinked.length}
    <h3>
      <button class="toggle" aria-expanded={showUnlinked} onclick={() => (showUnlinked = !showUnlinked)}>
        <span class:open={showUnlinked}>▸</span> Unlinked references <span class="count">{refs.unlinked.length}</span>
      </button>
    </h3>
    {#if showUnlinked}
      {#each unlinked as [page, items] (page)}
        <div class="group">
          <button class="page" onclick={() => ctx.open(page)}>{page}</button>
          {#each items as r (r.blockId)}
            <div class="hit">
              {#if r.breadcrumb.length}<div class="crumb">{r.breadcrumb.join(" › ")}</div>{/if}
              <BlockBody content={r.content} />
            </div>
          {/each}
        </div>
      {/each}
    {/if}
  {/if}
</section>

<style>
  .refs { margin-top: 28px; padding-top: 12px; border-top: 1px solid var(--border); }
  h3 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); margin: 14px 0 6px; font-weight: 600; }
  .count { font-weight: 400; margin-left: 4px; background: var(--sidebar); border-radius: 8px; padding: 0 6px; }
  .toggle { all: unset; cursor: pointer; }
  .toggle span:first-child { display: inline-block; transition: transform 0.1s; }
  .toggle span.open { transform: rotate(90deg); }
  .group { margin: 8px 0; }
  .page { all: unset; cursor: pointer; font-weight: 600; color: var(--accent); }
  .page:hover { text-decoration: underline; }
  .hit { margin: 3px 0 3px 4px; padding-left: 10px; border-left: 2px solid var(--border); }
  .crumb { font-size: 11px; color: var(--muted); }
  .err { color: var(--danger); }
</style>
