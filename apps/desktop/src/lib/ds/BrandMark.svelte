<script lang="ts">
  // The Next Agent logo (static/brand/logo.png, made from
  // src-tauri/icons/source/logo.png). The island's resting orb is a separate
  // mark: IslandMascot.svelte with --orb-gradient.
  let {
    size = 32,
    /** Accessible name; decorative when omitted. */
    label,
  }: { size?: number; label?: string } = $props();
</script>

<img
  class="ds-brand"
  src="/brand/logo.png"
  width={size}
  height={size}
  alt={label ?? ""}
  aria-hidden={label ? undefined : "true"}
  draggable="false"
/>

<style>
  .ds-brand {
    display: inline-block;
    flex: none;
    object-fit: contain;
    user-select: none;
  }
</style>
