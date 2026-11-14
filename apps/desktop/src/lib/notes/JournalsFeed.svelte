<script lang="ts">
  import { api } from "$lib/api.ts";
  import { useNotes } from "./ctx.ts";
  import PageEditor from "./PageEditor.svelte";

  const ctx = useNotes();
  let todayName = $state("");
  let older = $state.raw<string[]>([]);
  let count = $state(6);
  let more = $state(false);
  let problem = $state("");

  $effect(() => {
    ctx.version();
    const limit = count + 2;
    let live = true;
    void (async () => {
      try {
        const name = await api.graphJournalName(ctx.today());
        const entries = await api.graphJournals(limit);
        if (!live) return;
        todayName = name;
        const rest = entries.filter((e) => e.name.toLowerCase() !== name.toLowerCase());
        more = rest.length > count;
        older = rest.slice(0, count).map((e) => e.name);
        problem = "";
      } catch (e) {
        if (live) problem = String(e instanceof Error ? e.message : e);
      }
    })();
    return () => (live = false);
  });
</script>

<div class="feed">
  {#if problem}<div class="err" role="alert">{problem}</div>{/if}
  {#if todayName}<PageEditor name={todayName} compact />{/if}
  {#each older as name (name)}
    <PageEditor {name} compact />
  {/each}
  {#if more}<button class="more" onclick={() => (count += 6)}>Load older journals</button>{/if}
</div>

<style>
  .feed { display: grid; gap: 4px; }
  .more { justify-self: start; }
  .err { color: var(--danger); }
</style>
