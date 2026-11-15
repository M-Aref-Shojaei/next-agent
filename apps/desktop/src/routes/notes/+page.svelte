<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onDestroy, onMount } from "svelte";
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
    { id: "journals", label: "Journals", href: "/notes" },
    { id: "all", label: "All pages", href: "/notes?view=all" },
    { id: "graph", label: "Graph", href: "/notes?view=graph" },
  ];
</script>

<svelte:window {onkeydown} />

<div class="notes">
  {#if notes.phase === "loading"}
    <p class="center muted">Opening notes…</p>
  {:else if notes.phase === "choose"}
    <form class="center card" onsubmit={(e) => { e.preventDefault(); void notes.choose(folder); }}>
      <h1>Notes</h1>
      <p>Pick the folder that holds your notes. It becomes the vault: pages, journals and task records are plain Markdown files in a Logseq-compatible layout.</p>
      <label class="sr-only" for="vault-path">Vault folder path</label>
      <input id="vault-path" placeholder="/path/to/vault" bind:value={folder} />
      <button class="primary" type="submit" disabled={!folder.trim()}>Use this folder</button>
    </form>
  {:else if notes.phase === "setup"}
    <div class="center card">
      <h1>Set up notes here?</h1>
      <p>{notes.message}</p>
      <p class="muted">This adds <code>pages/</code>, <code>journals/</code> and <code>logseq/config.edn</code>. Nothing existing is changed.</p>
      <button class="primary" onclick={() => void notes.setup()}>Set up notes in the vault</button>
    </div>
  {:else if notes.phase === "error"}
    <div class="center card"><h1>Notes could not open</h1><p class="err" role="alert">{notes.message}</p><button onclick={() => void notes.init()}>Try again</button></div>
  {:else}
    <header class="top">
      <nav aria-label="Notes views">
        {#each tabs as t (t.id)}
          <a href={t.href} aria-current={view === t.id ? "page" : undefined}>{t.label}</a>
        {/each}
      </nav>
      {#if pageName}<span class="crumb" aria-live="polite">{pageName}</span>{/if}
      <button class="search" onclick={() => (searching = true)} aria-label="Search notes (Cmd or Ctrl+K)">Search <kbd>⌘K</kbd></button>
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
  .top { display: flex; align-items: center; gap: 16px; padding: 8px 20px; border-bottom: 1px solid var(--border); }
  nav { display: flex; gap: 4px; }
  nav a { padding: 4px 10px; border-radius: 6px; color: var(--muted); text-decoration: none; }
  nav a[aria-current="page"] { background: var(--panel); border: 1px solid var(--border); color: var(--text); font-weight: 600; }
  .crumb { color: var(--muted); }
  .search { margin-left: auto; display: flex; gap: 8px; align-items: center; color: var(--muted); }
  kbd { font: inherit; font-size: 11px; border: 1px solid var(--border); border-radius: 4px; padding: 0 4px; }
  .scroll { flex: 1; overflow: auto; padding: 18px 20px 60px; }
  .scroll.fill { overflow: hidden; display: flex; flex-direction: column; padding-bottom: 20px; }
  .column { max-width: 820px; margin: 0 auto; width: 100%; }
  .column.wide { max-width: 1100px; }
  .scroll.fill .column { flex: 1; min-height: 0; }
  .center { margin: 12vh auto 0; max-width: 460px; display: grid; gap: 10px; }
  .card { background: var(--panel); border: 1px solid var(--border); border-radius: 12px; padding: 22px; }
  .err { color: var(--danger); }
</style>
