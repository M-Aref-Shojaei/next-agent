<script lang="ts">
  import { app } from "$lib/app.svelte.ts";
  import { Tabs } from "$lib/ds/index.ts";
  import DiffViewer from "./DiffViewer.svelte";
  const TABS = [
    { id: "Diff", label: "Diff" },
    { id: "Files", label: "Files" },
    { id: "Vault", label: "Vault" },
  ] as const;
  let tab = $state<(typeof TABS)[number]["id"]>("Diff");
  const task = $derived(app.activeTask);
</script>

<aside class="panel" aria-label="Details panel">
  <Tabs tabs={TABS} bind:value={tab} label="Panel sections">
    {#snippet children(active)}
      <div class="body">
        {#if active === "Diff"}
          <DiffViewer
            files={task?.review?.files ?? []}
            branch={task?.review?.branch}
            outcome={task?.review?.outcome}
            onAction={(action) => (task ? app.review(task.id, action) : undefined)}
          />
        {:else if active === "Files"}
          <p class="muted">File tree — placeholder.</p>
        {:else if task?.recordPath}
          <p>Task record:</p>
          <code class="path">{task.recordPath}</code>
        {:else}
          <p class="muted">Finished tasks are written to the vault folder set in Settings.</p>
        {/if}
      </div>
    {/snippet}
  </Tabs>
</aside>

<style>
  .panel {
    border-left: var(--border-width) solid var(--color-border);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: var(--space-2) var(--space-3) 0;
  }
  .body { overflow: auto; }
  .path { word-break: break-all; font-size: var(--font-size-sm); }
</style>
