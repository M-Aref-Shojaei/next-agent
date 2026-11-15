<script lang="ts">
  import { Button, Checkbox, TextField } from "$lib/ds/index.ts";
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
    <div class="filter"><TextField placeholder="Filter pages" aria-label="Filter pages" bind:value={filter} /></div>
    <Checkbox label="Journals" bind:checked={journals} />
    <span class="muted">{rows.length} pages</span>
  </div>
  <table>
    <thead>
      <tr>
        <th><Button variant="ghost" size="sm" pressed={sort === "name"} onclick={() => (sort = "name")}>Page</Button></th>
        <th class="n"><Button variant="ghost" size="sm" pressed={sort === "backlinks"} onclick={() => (sort = "backlinks")}>Backlinks</Button></th>
        <th class="n"><Button variant="ghost" size="sm" pressed={sort === "blocks"} onclick={() => (sort = "blocks")}>Blocks</Button></th>
      </tr>
    </thead>
    <tbody>
      {#each rows as p (p.name)}
        <tr>
          <td><Button variant="ghost" size="sm" onclick={() => ctx.open(p.name)}>{p.name}</Button></td>
          <td class="n">{p.backlinks}</td>
          <td class="n">{p.blocks}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .bar { display: flex; gap: var(--space-4); align-items: center; margin-bottom: var(--space-3); flex-wrap: wrap; }
  .filter { flex: 1; max-width: 280px; min-width: 140px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: var(--space-1) var(--space-2); border-bottom: var(--border-width) solid var(--color-border-subtle); }
  th { font-size: var(--font-size-xs); text-transform: uppercase; letter-spacing: var(--letter-spacing-caps); font-weight: var(--font-weight-semibold); }
  .n { text-align: right; width: 110px; font-variant-numeric: tabular-nums; }
</style>
