<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    label,
    /** Draw a border around the list (standalone) or not (inside a Card/SettingsSection). */
    bordered = true,
    children,
  }: {
    /** Accessible name of the list. */
    label?: string;
    bordered?: boolean;
    /** <ListRow> items. */
    children: Snippet;
  } = $props();
</script>

<ul class="ds-list" class:bordered aria-label={label}>{@render children()}</ul>

<style>
  .ds-list { list-style: none; margin: 0; padding: 0; display: grid; min-width: 0; }
  .bordered {
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    overflow: hidden;
  }
</style>
