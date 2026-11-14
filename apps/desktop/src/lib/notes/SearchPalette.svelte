<script lang="ts">
  import { tick } from "svelte";
  import { api } from "$lib/api.ts";
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
    void tick().then(() => input?.focus());
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
    <input
      bind:this={input}
      bind:value={query}
      {oninput}
      {onkeydown}
      placeholder="Search pages and blocks…"
      aria-label="Search pages and blocks"
      role="combobox"
      aria-expanded="true"
      aria-controls="search-results"
      autocomplete="off"
      spellcheck="false"
    />
    <ul id="search-results" role="listbox">
      {#each rows as row, i (i)}
        <li role="option" aria-selected={i === selected} data-search-selected={i === selected}>
          <button class:on={i === selected} onmouseenter={() => (selected = i)} onclick={() => choose(row)}>
            {#if row.kind === "page"}
              <span class="tag">page</span><span class="main">{row.name}</span>
            {:else if row.kind === "create"}
              <span class="tag new">new</span><span class="main">Create page “{row.name}”</span>
            {:else}
              <span class="tag">block</span>
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
    {#if problem}<div class="err" role="alert">{problem}</div>{/if}
    <footer class="muted"><span>↑↓ select</span><span>↵ open</span><span>esc close</span></footer>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 100; background: rgb(0 0 0 / 0.35); display: flex; justify-content: center; align-items: flex-start; padding-top: 12vh; }
  .palette { width: min(640px, calc(100vw - 32px)); background: var(--panel); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 20px 60px rgb(0 0 0 / 0.35); overflow: hidden; }
  input { border: 0; border-bottom: 1px solid var(--border); border-radius: 0; padding: 14px 16px; font-size: 16px; }
  input:focus-visible { outline: none; }
  ul { list-style: none; margin: 0; padding: 6px; max-height: 50vh; overflow: auto; }
  li button { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 10px; align-items: baseline; padding: 6px 10px; border-radius: 6px; cursor: pointer; }
  li button.on { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .tag { flex: none; width: 42px; font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  .tag.new { color: var(--ok); }
  .main { min-width: 0; flex: 1; display: grid; }
  .snip { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { font-size: 11px; color: var(--muted); }
  mark { background: color-mix(in srgb, var(--warn) 35%, transparent); color: inherit; border-radius: 2px; }
  .empty { padding: 10px; }
  footer { display: flex; gap: 14px; padding: 6px 14px; border-top: 1px solid var(--border); font-size: 11px; }
  .err { color: var(--danger); padding: 6px 14px; font-size: 12px; }
</style>
