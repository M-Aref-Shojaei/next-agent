<script lang="ts">
  import { api } from "$lib/api.ts";
  import type { BlockRef } from "./types.ts";
  import { editableText } from "./content.ts";
  import { useNotes } from "./ctx.ts";

  let { uuid }: { uuid: string } = $props();
  const ctx = useNotes();
  let target = $state<BlockRef | null | undefined>(undefined);

  $effect(() => {
    ctx.version();
    let live = true;
    void api.graphBlock(uuid).then(
      (b) => live && (target = b),
      () => live && (target = null),
    );
    return () => (live = false);
  });

  const text = $derived(target ? (editableText(target.content).split("\n")[0] ?? "").replace(/^(TODO|DOING|DONE|NOW|LATER|WAITING|CANCELED|CANCELLED)\s+/, "") : "");
  function go(event: Event) {
    event.stopPropagation();
    if (target) ctx.open(target.page);
  }
</script>

{#if target}
  <button class="blockref" title={`From ${target.page}`} onclick={go}>{text || "(empty block)"}</button>
{:else}
  <span class="missing" title="No block with this id in the graph">(({uuid.slice(0, 8)}…))</span>
{/if}

<style>
  .blockref { all: unset; cursor: pointer; border-bottom: 1px solid var(--accent); color: var(--text); }
  .blockref:hover { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .missing { color: var(--muted); font-size: 0.9em; }
</style>
