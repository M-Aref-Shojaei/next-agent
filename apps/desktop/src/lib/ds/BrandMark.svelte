<script lang="ts">
  // The Next Agent brand mark: the red gradient orb the island shows at rest
  // (IslandMascot.svelte, tone "rest"). Static here; the island animates it.
  let {
    size = 32,
    /** Accessible name; decorative when omitted. */
    label,
  }: { size?: number; label?: string } = $props();
</script>

<span
  class="ds-brand"
  style:width="{size}px"
  style:height="{size}px"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
></span>

<style>
  .ds-brand {
    display: inline-block;
    flex: none;
    border-radius: var(--radius-full);
    background: var(--brand-gradient);
    box-shadow: 0 0 6px var(--color-brand);
  }
</style>
