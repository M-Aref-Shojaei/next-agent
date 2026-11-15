<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Tone } from "./types.ts";

  let {
    label,
    value,
    /** A change against before ("+12%", "3 failed"). Coloured by `tone`; the text says it too. */
    delta,
    /** Small supporting text under the value. */
    hint,
    tone = "neutral",
    children,
  }: {
    label: string;
    value: string | number;
    delta?: string;
    hint?: string;
    tone?: Tone;
    /** Extra content under the hint. */
    children?: Snippet;
  } = $props();

  const uid = $props.id();
</script>

<!-- A group of label + value: a described-by pair reads well in any screen reader. -->
<div class="ds-stat {tone}" role="group" aria-labelledby="{uid}-label">
  <span class="label" id="{uid}-label">{label}</span>
  <span class="value">{value}</span>
  {#if delta}<span class="delta">{delta}</span>{/if}
  {#if hint}<span class="hint">{hint}</span>{/if}
  {#if children}<span class="extra">{@render children()}</span>{/if}
</div>

<style>
  .ds-stat {
    --t-fg: var(--color-text-secondary);
    display: grid;
    gap: var(--space-0-5);
    align-content: start;
    min-width: 0;
    font-size: var(--font-size-sm);
  }
  .accent { --t-fg: var(--color-accent-text); }
  .info { --t-fg: var(--color-info-text); }
  .ok { --t-fg: var(--color-ok-text); }
  .warn { --t-fg: var(--color-warn-text); }
  .danger { --t-fg: var(--color-danger-text); }
  .label { color: var(--color-text-tertiary); }
  .value {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    line-height: var(--line-height-tight);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .delta { color: var(--t-fg); font-weight: var(--font-weight-medium); }
  .hint { color: var(--color-text-tertiary); }
</style>
