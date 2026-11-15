<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    label = "Form actions",
    /** Padding for a bar inside a bordered box (a SettingsSection's rows). Without it the bar sits under its content. */
    inset = false,
    /** Buttons in reading order: secondary actions first, the primary one last. */
    children,
  }: {
    /** Accessible name of the group. */
    label?: string;
    inset?: boolean;
    children: Snippet;
  } = $props();
</script>

<!-- The wrapper is the container the bar's narrow layout measures. -->
<div class="ds-form-actions" class:inset>
  <div class="bar" role="group" aria-label={label}>{@render children()}</div>
</div>

<style>
  .ds-form-actions { container-type: inline-size; margin-top: var(--space-3); }
  .inset { margin-top: 0; padding: var(--space-3) var(--space-4); }
  .bar { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--space-2); }

  /* Narrow: one full-width button per line, the primary action on top. */
  @container (max-width: 420px) {
    .bar { flex-direction: column-reverse; align-items: stretch; }
    .bar > :global(*) { width: 100%; justify-content: center; }
  }
</style>
