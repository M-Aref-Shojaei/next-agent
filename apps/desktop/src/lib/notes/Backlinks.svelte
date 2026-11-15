<script lang="ts">
  import { api } from "$lib/api.ts";
  import { Badge, Banner, Button, Icon } from "$lib/ds/index.ts";
  import type { Backlink, Backlinks } from "./types.ts";
  import { groupByPage } from "./group.ts";
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

  const linked = $derived(groupByPage(refs.linked));
  const unlinked = $derived(groupByPage(refs.unlinked));
</script>

{#snippet group(page: string, items: Backlink[])}
  <div class="group">
    <Button variant="ghost" size="sm" onclick={() => ctx.open(page)}>{page}</Button>
    {#each items as r (r.blockId)}
      <div class="hit">
        {#if r.breadcrumb.length}<div class="crumb">{r.breadcrumb.join(" › ")}</div>{/if}
        <BlockBody content={r.content} />
      </div>
    {/each}
  </div>
{/snippet}

<section class="refs" aria-label="References">
  {#if error}<Banner tone="danger">{error}</Banner>{/if}
  <h3>Linked references <Badge size="sm">{refs.linked.length}</Badge></h3>
  {#each linked as [page, items] (page)}
    {@render group(page, items)}
  {:else}
    <p class="muted">Nothing links here yet.</p>
  {/each}

  {#if refs.unlinked.length}
    <h3>
      <Button variant="ghost" size="sm" aria-expanded={showUnlinked} onclick={() => (showUnlinked = !showUnlinked)}>
        {#snippet icon()}<span class="twist" class:open={showUnlinked}><Icon name="chevron-right" size={14} /></span>{/snippet}
        Unlinked references
        {#snippet iconEnd()}<Badge size="sm">{refs.unlinked.length}</Badge>{/snippet}
      </Button>
    </h3>
    {#if showUnlinked}
      {#each unlinked as [page, items] (page)}
        {@render group(page, items)}
      {/each}
    {/if}
  {/if}
</section>

<style>
  .refs { margin-top: var(--space-6); padding-top: var(--space-3); border-top: var(--border-width) solid var(--color-border); }
  h3 { font-size: var(--font-size-sm); text-transform: uppercase; letter-spacing: var(--letter-spacing-caps); color: var(--color-text-secondary); margin: var(--space-3) 0 var(--space-1-5); font-weight: var(--font-weight-semibold); display: flex; align-items: center; gap: var(--space-1); }
  .twist { display: inline-flex; transition: transform var(--duration-fast) var(--ease-standard); }
  .twist.open { transform: rotate(90deg); }
  .group { margin: var(--space-2) 0; }
  .hit { margin: var(--space-0-5) 0 var(--space-0-5) var(--space-1); padding-left: var(--space-2); border-left: var(--border-width-strong) solid var(--color-border); }
  .crumb { font-size: var(--font-size-xs); color: var(--color-text-secondary); }
</style>
