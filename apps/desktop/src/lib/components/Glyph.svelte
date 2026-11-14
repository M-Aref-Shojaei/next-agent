<script lang="ts">
  // A Lucide glyph (see $lib/island/icons.ts), drawn at `size` px.
  import type { Glyph } from "$lib/island/icons.ts";

  let { glyph, size = 14, stroke = 2 }: { glyph: Glyph; size?: number; stroke?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each glyph.paths as d (d)}<path {d} />{/each}
  {#each glyph.circles ?? [] as c (c.cx + ":" + c.cy)}<circle cx={c.cx} cy={c.cy} r={c.r} />{/each}
</svg>
