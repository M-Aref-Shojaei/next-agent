<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onDestroy, onMount } from "svelte";
  import { Banner, Button, Card, Icon, Kbd, SegmentedControl, TextField } from "$lib/ds/index.ts";
  import AllPages from "$lib/notes/AllPages.svelte";
  import { provideNotes } from "$lib/notes/ctx.ts";
  import GraphView from "$lib/notes/GraphView.svelte";
  import JournalsFeed from "$lib/notes/JournalsFeed.svelte";
  import PageEditor from "$lib/notes/PageEditor.svelte";
  import SearchPalette from "$lib/notes/SearchPalette.svelte";
  import { notes } from "$lib/notes/store.svelte.ts";

  let folder = $state("");
  let searching = $state(false);

  const pageName = $derived(page.url.searchParams.get("page"));
  const view = $derived(pageName ? "page" : (page.url.searchParams.get("view") ?? "journals"));

  provideNotes({
    open: (name) => void goto(`/notes?page=${encodeURIComponent(name)}`, { keepFocus: false }),
    pageNames: () => notes.pages.filter((p) => p.journalDay === null).map((p) => p.name),
    today: () => notes.today,
    version: () => notes.version,
  });

  onMount(() => void notes.init());
  onDestroy(() => notes.destroy());

  function onkeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
      event.preventDefault();
      searching = !searching;
    }
  }

  function openFromSearch(name: string) {
    searching = false;
    void goto(`/notes?page=${encodeURIComponent(name)}`);
  }

  const tabs = [
    { value: "journals", label: "Journals" },
    { value: "all", label: "All pages" },
    { value: "graph", label: "Graph" },
  ];
  const TAB_HREF: Record<string, string> = { journals: "/notes", all: "/notes?view=all", graph: "/notes?view=graph" };
</script>

<svelte:window {onkeydown} />

<div class="notes">
  {#if notes.phase === "loading"}
    <p class="center muted">Opening notes…</p>
  {:else if notes.phase === "choose"}
    <form class="center" onsubmit={(e) => { e.preventDefault(); void notes.choose(folder); }}>
      <Card title="Notes">
        <div class="stack">
          <p>Pick the folder that holds your notes. It becomes the vault: pages, journals and task records are plain Markdown files in a Logseq-compatible layout.</p>
          <TextField label="Vault folder path" hideLabel placeholder="/path/to/vault" mono bind:value={folder} />
          <Button variant="primary" type="submit" disabled={!folder.trim()}>Use this folder</Button>
        </div>
      </Card>
    </form>
  {:else if notes.phase === "setup"}
    <div class="center">
      <Card title="Set up notes here?">
        <div class="stack">
          <p>{notes.message}</p>
          <p class="muted">This adds <code>pages/</code>, <code>journals/</code> and <code>logseq/config.edn</code>. Nothing existing is changed.</p>
          <Button variant="primary" onclick={() => void notes.setup()}>Set up notes in the vault</Button>
        </div>
      </Card>
    </div>
  {:else if notes.phase === "error"}
    <div class="center">
      <Card title="Notes could not open">
        <div class="stack">
          <Banner tone="danger">{notes.message}</Banner>
          <Button onclick={() => void notes.init()}>Try again</Button>
        </div>
      </Card>
    </div>
  {:else}
    <header class="top">
      <SegmentedControl
        label="Notes views"
        options={tabs}
        value={pageName ? undefined : view}
        onchange={(id) => void goto(TAB_HREF[id] ?? "/notes")}
      />
      {#if pageName}<span class="crumb" aria-live="polite">{pageName}</span>{/if}
      <div class="search">
        <Button variant="ghost" aria-label="Search notes (Cmd or Ctrl+K)" onclick={() => (searching = true)}>
          {#snippet icon()}<Icon name="search" />{/snippet}
          Search
          {#snippet iconEnd()}<Kbd keys="Ctrl+K" size="sm" />{/snippet}
        </Button>
      </div>
    </header>
    <div class="scroll" class:fill={view === "graph"}>
      <div class="column" class:wide={view !== "page" && view !== "journals"}>
        {#if view === "page" && pageName}
          {#key pageName}<PageEditor name={pageName} focusOnLoad />{/key}
        {:else if view === "all"}
          <AllPages />
        {:else if view === "graph"}
          <GraphView />
        {:else}
          <JournalsFeed />
        {/if}
      </div>
    </div>
    {#if searching}<SearchPalette onopen={openFromSearch} onclose={() => (searching = false)} />{/if}
  {/if}
</div>

<style>
  .notes { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .top { display: flex; align-items: center; gap: var(--space-4); padding: var(--space-2) var(--space-5); border-bottom: var(--border-width) solid var(--color-border); flex-wrap: wrap; }
  .crumb { color: var(--color-text-secondary); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .search { margin-left: auto; }
  .scroll { flex: 1; overflow: auto; padding: var(--space-5) var(--space-5) calc(var(--space-12) + var(--space-3)); }
  .scroll.fill { overflow: hidden; display: flex; flex-direction: column; padding-bottom: var(--space-5); }
  .column { max-width: 820px; margin: 0 auto; width: 100%; }
  .column.wide { max-width: 1100px; }
  .scroll.fill .column { flex: 1; min-height: 0; }
  .center { margin: 12vh auto 0; max-width: 460px; padding: 0 var(--space-4); }
  .stack { display: grid; gap: var(--space-3); }
  .stack p { margin: 0; }
</style>
