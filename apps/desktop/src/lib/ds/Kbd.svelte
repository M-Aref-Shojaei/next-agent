<script lang="ts">
  import { currentPlatform, shortcutLabel, shortcutParts, type Platform } from "./shortcut.ts";

  let {
    keys,
    platform = currentPlatform(),
    size = "md",
  }: {
    /** A canonical shortcut ("Ctrl+Shift+K") or a single key ("Esc"). */
    keys: string;
    platform?: Platform;
    size?: "sm" | "md";
  } = $props();

  const parts = $derived(shortcutParts(keys, platform));
</script>

<kbd class="ds-kbd {size}" aria-label={shortcutLabel(keys, platform)}>
  {#each parts as part, i (i)}<kbd class="cap" aria-hidden="true">{part}</kbd>{/each}
</kbd>

<style>
  .ds-kbd { display: inline-flex; align-items: center; gap: var(--space-0-5); font: inherit; vertical-align: middle; }
  .cap {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 20px;
    padding: 0 var(--space-1);
    border: var(--border-width) solid var(--color-border);
    border-bottom-width: var(--border-width-strong);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    color: var(--color-text-secondary);
    font-family: var(--font-sans);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    line-height: 1;
  }
  .sm .cap { min-width: 16px; height: 16px; font-size: calc(var(--font-size-xs) - 1px); }
</style>
