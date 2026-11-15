<script lang="ts">
  // A provider's or a service's own colour (the overlay's data), next to its
  // name. Decorative: the name beside it says what it is.
  let { color }: { color: string } = $props();
</script>

<span class="accent-dot" style:--dot={color} aria-hidden="true"></span>

<style>
  .accent-dot {
    display: inline-block;
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: var(--radius-full);
    background: var(--dot);
  }
</style>
