<script lang="ts">
  import { onDestroy } from "svelte";
  import { api } from "$lib/api.ts";
  import Backlinks from "./Backlinks.svelte";
  import Block from "./Block.svelte";
  import { isHiddenProperty } from "./content.ts";
  import { useNotes } from "./ctx.ts";
  import { Outliner } from "./outliner.svelte.ts";
  import { notes } from "./store.svelte.ts";
  import type { PageView } from "./types.ts";

  let {
    name,
    compact = false,
    showReferences = true,
    focusOnLoad = false,
  }: { name: string; compact?: boolean; showReferences?: boolean; focusOnLoad?: boolean } = $props();
  const ctx = useNotes();

  let view = $state.raw<PageView | null>(null);
  let problem = $state("");
  let loadedName = "";
  let saved = false;

  const outliner = new Outliner({
    exists: () => saved || (view?.file ?? null) !== null,
    save: async (tree) => {
      const out = await api.graphSavePage(loadedName, tree);
      saved = true;
      view = out;
    },
  });

  async function load(target: string, adopt: "replace" | "idle") {
    try {
      const page = await api.graphPage(target);
      if (target !== name) return; // navigated away while loading
      view = page;
      problem = "";
      if (adopt === "replace") {
        loadedName = page.name || target;
        saved = false;
        outliner.load(page.blocks);
        if (focusOnLoad && page.blocks.length === 0) outliner.focusFirst();
      } else {
        outliner.reloadIfIdle(page.blocks);
      }
    } catch (e) {
      problem = String(e instanceof Error ? e.message : e);
    }
  }

  // Page changed: write what is pending to the old page first, then load the new one.
  let current = "";
  $effect(() => {
    const target = name;
    if (target === current) return;
    const previous = current;
    current = target;
    void (previous ? outliner.flush() : Promise.resolve()).then(() => load(target, "replace"));
  });

  // The index changed (an external edit, or our own save echoing back).
  $effect(() => {
    const v = ctx.version();
    // Only a change to this page's file (or an unknown one) is worth a re-read.
    if (v > 0 && current && (!view?.file || notes.changed.includes(view.file))) void load(current, "idle");
  });

  onDestroy(() => void outliner.flush());

  const extra = $derived((view?.properties ?? []).filter((p) => !isHiddenProperty(p.key) && p.key !== "title"));
  const missing = $derived(view !== null && view.file === null && !saved);

  function clickBelow(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      const last = outliner.tree[outliner.tree.length - 1];
      if (last) outliner.focus(last.id, "end");
    }
  }
</script>

<article class="page" class:compact>
  <header>
    {#if compact}
      <h2><button class="title" onclick={() => ctx.open(name)}>{view?.name ?? name}</button></h2>
    {:else}
      <h1>{view?.name ?? name}</h1>
    {/if}
    <span class="status" aria-live="polite">
      {#if outliner.status === "dirty" || outliner.status === "saving"}Saving…{:else if outliner.status === "saved"}Saved{:else if outliner.status === "error"}<span class="err">Not saved</span>{/if}
    </span>
  </header>
  {#if outliner.error}<div class="err" role="alert">Could not save: {outliner.error}</div>{/if}
  {#if problem}<div class="err" role="alert">{problem}</div>{/if}
  {#if extra.length}
    <div class="props">{#each extra as p (p.rawKey)}<span><b>{p.key}</b> {p.value}</span>{/each}</div>
  {/if}
  {#if missing && !compact}<p class="muted hint">This page has no file yet. Start typing to create it.</p>{/if}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <ul class="outline" onclick={clickBelow}>
    {#each outliner.tree as block (block.id)}
      <Block {block} {outliner} />
    {/each}
  </ul>
  {#if showReferences && !compact}<Backlinks {name} />{/if}
</article>

<style>
  .page { padding-bottom: 12px; }
  header { display: flex; align-items: baseline; gap: 12px; margin-bottom: 8px; }
  h1 { font-size: 26px; font-weight: 700; }
  h2 { font-size: 17px; font-weight: 650; }
  .title { all: unset; cursor: pointer; }
  .title:hover { color: var(--accent); }
  .status { margin-left: auto; font-size: 11px; color: var(--muted); }
  .err { color: var(--danger); font-size: 12px; }
  .props { display: flex; gap: 10px; flex-wrap: wrap; font-size: 12px; color: var(--muted); margin-bottom: 8px; }
  .props b { font-weight: 600; background: var(--sidebar); border: 1px solid var(--border); border-radius: 4px; padding: 0 5px; margin-right: 4px; }
  .hint { font-size: 12px; margin: 0 0 6px; }
  .outline { margin: 0; padding: 0; min-height: 28px; }
  .compact { border-bottom: 1px solid var(--border); margin-bottom: 14px; }
</style>
