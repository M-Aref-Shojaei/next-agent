<script lang="ts">
  import { log } from "../log.ts";
  import { api } from "$lib/api.ts";
  import { Banner, Button } from "$lib/ds/index.ts";
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
        log.warn("ui_error", { kind: "journals" });
        if (live) problem = String(e instanceof Error ? e.message : e);
      }
    })();
    return () => (live = false);
  });
</script>

<div class="feed">
  {#if problem}<Banner tone="danger">{problem}</Banner>{/if}
  {#if todayName}<PageEditor name={todayName} compact />{/if}
  {#each older as name (name)}
    <PageEditor {name} compact />
  {/each}
  {#if more}<div class="more"><Button onclick={() => (count += 6)}>Load older journals</Button></div>{/if}
</div>

<style>
  .feed { display: grid; gap: var(--space-1); }
  .more { justify-self: start; }
</style>
