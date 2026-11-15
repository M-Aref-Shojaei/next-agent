<script lang="ts">
  // Download / progress / installed, for the voice model (Coucou's
  // hub/pages/flowsettings.ts modelsControl). Removing asks once more.
  import { Button, ProgressBar } from "$lib/ds/index.ts";
  import { overlay } from "$lib/overlay-settings.ts";
  import { flowStore } from "$lib/overlay-store.svelte.ts";

  /** Called when the model turns ready while this is on screen. */
  let { onready }: { onready?: () => void } = $props();

  let confirming = $state(false);
  const m = $derived(flowStore.status.models.parakeet);

  let wasReady = flowStore.status.models.parakeet.state === "ready";
  $effect(() => {
    const ready = m.state === "ready";
    if (!wasReady && ready) onready?.();
    wasReady = ready;
  });
</script>

<span class="model">
  {#if m.state === "missing"}
    <Button variant="primary" onclick={() => void overlay.flowModelsDownload()}>Download</Button>
  {:else if m.state === "downloading"}
    <ProgressBar label="Downloading the voice model" hideLabel showValue value={Math.round(m.fraction * 100)} />
  {:else if m.state === "failed"}
    <span class="error" title={m.error}>{m.error}</span>
    <Button onclick={() => void overlay.flowModelsDownload()}>Retry</Button>
  {:else if confirming}
    <Button
      variant="danger"
      size="sm"
      onclick={() => {
        confirming = false;
        void overlay.flowModelsRemove();
      }}
    >Remove the models</Button>
    <Button size="sm" onclick={() => (confirming = false)}>Cancel</Button>
  {:else}
    <Button variant="ghost" title="Remove the models" onclick={() => (confirming = true)}>✓ Installed</Button>
  {/if}
</span>

<style>
  .model { display: inline-flex; align-items: center; gap: var(--space-2); min-width: 180px; }
  .model :global(.ds-progress) { flex: 1; min-width: 140px; }
  .error {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--font-size-sm);
    color: var(--color-danger-text);
  }
</style>
