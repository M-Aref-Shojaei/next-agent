<script lang="ts">
  import { useNotes } from "./ctx.ts";
  import { notes } from "./store.svelte.ts";

  const ctx = useNotes();
  let filter = $state("");
  let journals = $state(false);
  let sort = $state<"name" | "backlinks" | "blocks">("name");

  const rows = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = notes.pages.filter((p) => (journals || p.journalDay === null) && p.name.toLowerCase().includes(q));
    return [...list].sort((a, b) =>
      sort === "name" ? a.name.localeCompare(b.name) : b[sort] - a[sort] || a.name.localeCompare(b.name),
    );
  });
</script>

<div class="all">
  <div class="bar">
    <input placeholder="Filter pages" aria-label="Filter pages" bind:value={filter} />
    <label><input type="checkbox" bind:checked={journals} /> Journals</label>
    <span class="muted">{rows.length} pages</span>
  </div>
  <table>
    <thead>
      <tr>
        <th><button aria-pressed={sort === "name"} onclick={() => (sort = "name")}>Page</button></th>
        <th class="n"><button aria-pressed={sort === "backlinks"} onclick={() => (sort = "backlinks")}>Backlinks</button></th>
        <th class="n"><button aria-pressed={sort === "blocks"} onclick={() => (sort = "blocks")}>Blocks</button></th>
      </tr>
    </thead>
    <tbody>
      {#each rows as p (p.name)}
        <tr>
          <td><button class="link" onclick={() => ctx.open(p.name)}>{p.name}</button></td>
          <td class="n">{p.backlinks}</td>
          <td class="n">{p.blocks}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .bar { display: flex; gap: 14px; align-items: center; margin-bottom: 10px; }
  .bar input:not([type]) { max-width: 280px; }
  label { display: flex; gap: 6px; align-items: center; white-space: nowrap; }
  label input { width: auto; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--border); }
  th button { all: unset; cursor: pointer; font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  th button[aria-pressed="true"] { color: var(--text); font-weight: 700; }
  .n { text-align: right; width: 90px; font-variant-numeric: tabular-nums; }
  .link { all: unset; cursor: pointer; color: var(--accent); }
  .link:hover { text-decoration: underline; }
</style>
