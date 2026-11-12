<script lang="ts">
  import DiffViewer from "./DiffViewer.svelte";
  const TABS = ["Diff", "Files", "Vault"] as const;
  let tab = $state<(typeof TABS)[number]>("Diff");
</script>

<aside class="panel" aria-label="Details panel">
  <div role="tablist" aria-label="Panel sections">
    {#each TABS as t (t)}
      <button role="tab" id="tab-{t}" aria-selected={tab === t} aria-controls="pane-{t}" onclick={() => (tab = t)}>{t}</button>
    {/each}
  </div>
  <div role="tabpanel" id="pane-{tab}" aria-labelledby="tab-{tab}" class="body">
    {#if tab === "Diff"}
      <DiffViewer files={[]} />
    {:else if tab === "Files"}
      <p class="muted">File tree — placeholder.</p>
    {:else}
      <p class="muted">Vault browser — placeholder.</p>
    {/if}
  </div>
</aside>

<style>
  .panel { border-left: 1px solid var(--border); background: var(--panel); display: flex; flex-direction: column; min-width: 0; }
  [role="tablist"] { display: flex; border-bottom: 1px solid var(--border); }
  [role="tab"] { border: 0; border-radius: 0; background: transparent; }
  [role="tab"][aria-selected="true"] { border-bottom: 2px solid var(--accent); font-weight: 600; }
  .body { padding: 12px; overflow: auto; }
</style>
