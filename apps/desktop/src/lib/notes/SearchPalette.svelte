<script lang="ts">
  import { tick } from "svelte";
  import { api } from "$lib/api.ts";
  import { Badge, Banner, Icon, Kbd, TextField } from "$lib/ds/index.ts";
  import { editableText } from "./content.ts";
  import { snippetParts, type SearchHit } from "./types.ts";
  import { notes } from "./store.svelte.ts";

  let { onopen, onclose }: { onopen: (page: string) => void; onclose: () => void } = $props();

  let query = $state("");
  let hits = $state.raw<SearchHit[]>([]);
  let selected = $state(0);
  let problem = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  type Row = { kind: "page"; name: string } | { kind: "create"; name: string } | { kind: "block"; hit: SearchHit };

  const rows = $derived.by((): Row[] => {
    const q = query.trim();
    if (!q) return [];
    const lc = q.toLowerCase();
    const pages: Row[] = notes.pages
      .filter((p) => p.name.toLowerCase().includes(lc))
      .sort((a, b) => Number(b.name.toLowerCase().startsWith(lc)) - Number(a.name.toLowerCase().startsWith(lc)) || a.name.length - b.name.length)
      .slice(0, 6)
      .map((p) => ({ kind: "page", name: p.name }));
    const exact = notes.pages.some((p) => p.name.toLowerCase() === lc);
    return [...pages, ...(exact ? [] : [{ kind: "create", name: q } as Row]), ...hits.map((hit): Row => ({ kind: "block", hit }))];
  });

  $effect(() => {
    input?.focus();
  });

  function oninput() {
    selected = 0;
    clearTimeout(timer);
    const q = query.trim();
    if (!q) {
      hits = [];
      return;
    }
    const mine = ++seq;
    timer = setTimeout(() => {
      api.graphSearch(q, 30).then(
        (r) => mine === seq && ((hits = r), (problem = "")),
        (e) => mine === seq && (problem = String(e instanceof Error ? e.message : e)),
      );
    }, 120);
  }

  function choose(row: Row | undefined) {
    if (!row) return;
    onopen(row.kind === "block" ? row.hit.page : row.name);
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onclose();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      selected = rows.length ? (selected + 1) % rows.length : 0;
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      selected = rows.length ? (selected + rows.length - 1) % rows.length : 0;
    } else if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      choose(rows[selected]);
    }
  }

  $effect(() => {
    void selected;
    void tick().then(() => document.querySelector("[data-search-selected='true']")?.scrollIntoView({ block: "nearest" }));
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onmousedown={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Search notes">
    <div class="field">
      <TextField
        bind:value={query}
        bind:ref={input}
        {oninput}
        {onkeydown}
        size="lg"
        placeholder="Search pages and blocks…"
        aria-label="Search pages and blocks"
        role="combobox"
        aria-expanded="true"
        aria-controls="search-results"
        autocomplete="off"
        spellcheck="false"
      >
        {#snippet prefix()}<Icon name="search" />{/snippet}
      </TextField>
    </div>
    <ul id="search-results" role="listbox">
      {#each rows as row, i (i)}
        <li role="option" aria-selected={i === selected} data-search-selected={i === selected}>
          <button class:on={i === selected} onmouseenter={() => (selected = i)} onclick={() => choose(row)}>
            {#if row.kind === "page"}
              <span class="tag"><Badge size="sm">page</Badge></span><span class="main">{row.name}</span>
            {:else if row.kind === "create"}
              <span class="tag"><Badge size="sm" tone="ok">new</Badge></span><span class="main">Create page “{row.name}”</span>
            {:else}
              <span class="tag"><Badge size="sm">block</Badge></span>
              <span class="main">
                <span class="snip">{#each snippetParts(row.hit.snippet || editableText(row.hit.content)) as p, j (j)}{#if p.match}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
                <span class="where">{row.hit.page}</span>
              </span>
            {/if}
          </button>
        </li>
      {/each}
      {#if query.trim() && rows.length === 0}<li class="empty muted">No results</li>{/if}
    </ul>
    {#if problem}<Banner tone="danger">{problem}</Banner>{/if}
    <footer class="muted"><span><Kbd keys="Up" size="sm" /><Kbd keys="Down" size="sm" /> select</span><span><Kbd keys="Enter" size="sm" /> open</span><span><Kbd keys="Esc" size="sm" /> close</span></footer>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: var(--z-modal); background: var(--color-scrim); display: flex; justify-content: center; align-items: flex-start; padding: 12vh var(--space-4) 0; }
  .palette { width: min(640px, 100%); background: var(--color-surface-overlay); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-xl); box-shadow: var(--shadow-xl); overflow: hidden; }
  .field { padding: var(--space-3); border-bottom: var(--border-width) solid var(--color-border); }
  ul { list-style: none; margin: 0; padding: var(--space-1-5); max-height: 50vh; overflow: auto; }
  li button { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: var(--space-2); align-items: baseline; padding: var(--space-1-5) var(--space-2); border-radius: var(--radius-md); cursor: pointer; }
  li button.on { background: var(--color-accent-subtle); }
  .tag { flex: none; width: 48px; }
  .main { min-width: 0; flex: 1; display: grid; }
  .snip { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { font-size: var(--font-size-xs); color: var(--color-text-secondary); }
  mark { background: var(--color-warn-subtle); color: inherit; border-radius: var(--radius-xs); }
  .empty { padding: var(--space-2); }
  footer { display: flex; gap: var(--space-4); padding: var(--space-1-5) var(--space-3); border-top: var(--border-width) solid var(--color-border); font-size: var(--font-size-xs); }
  footer span { display: inline-flex; align-items: center; gap: var(--space-1); }
</style>
