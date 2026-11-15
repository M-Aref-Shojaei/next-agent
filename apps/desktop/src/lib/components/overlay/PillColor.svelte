<script lang="ts">
  // The colour dot of a pill's row and the palette it opens (Coucou's
  // settings/colors.ts): the palette's colours, and "Default" once changed.
  // The colours are the overlay's data (core/pill-colors.ts), drawn as given.
  import { Button, ColorSwatchGroup } from "$lib/ds/index.ts";
  import { PILL_PALETTE, pillColor, withPillColor } from "$coucou/core/pill-colors";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";

  let { def }: { def: { id: string; name: string; color: string } } = $props();

  let open = $state(false);
  const current = $derived(pillColor(def.id, def.color, store.settings.pillColors));
  const changed = $derived(current !== pillColor(def.id, def.color, null));
  const swatches = PILL_PALETTE.map((hex) => ({ value: hex, label: hex, color: hex }));

  function choose(hex: string | null) {
    void store.set({ pillColors: withPillColor(store.settings.pillColors, def.id, hex, def.color) });
    open = false;
  }
</script>

<span class="pill-color">
  <button
    type="button"
    class="dot"
    style:--dot={current}
    title="Color"
    aria-label="{def.name} · Color"
    aria-expanded={open}
    onclick={() => (open = !open)}
  ></button>
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="palette" onkeydown={(e) => { if (e.key === "Escape") open = false; }}>
      <ColorSwatchGroup label="{def.name} · Color" swatches={swatches} value={current} onchange={choose} />
      {#if changed}<Button size="sm" variant="ghost" onclick={() => choose(null)}>Default</Button>{/if}
    </span>
  {/if}
</span>

<style>
  .pill-color { display: inline-flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  .dot {
    width: 12px;
    height: 12px;
    padding: 0;
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-full);
    background: var(--dot);
    cursor: pointer;
  }
  .dot:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .palette {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1-5) var(--space-2);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-raised);
    box-shadow: var(--shadow-md);
  }
</style>
