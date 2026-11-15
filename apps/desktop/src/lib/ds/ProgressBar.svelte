<script lang="ts">
  import type { Tone } from "./types.ts";

  let {
    value,
    max = 100,
    /** Accessible name (also shown above the bar unless hideLabel). */
    label,
    hideLabel = false,
    showValue = false,
    tone = "accent",
    format = (v: number, m: number) => `${Math.round((v / m) * 100)}%`,
  }: {
    /** Omit for an indeterminate bar. */
    value?: number;
    max?: number;
    label: string;
    hideLabel?: boolean;
    showValue?: boolean;
    tone?: Exclude<Tone, "neutral">;
    format?: (value: number, max: number) => string;
  } = $props();

  const uid = $props.id();
  const clamped = $derived(value === undefined ? undefined : Math.min(Math.max(value, 0), max));
</script>

<div class="ds-progress {tone}">
  {#if !hideLabel || showValue}
    <div class="head">
      <span id="{uid}-label" class:sr-only={hideLabel}>{label}</span>
      {#if showValue && clamped !== undefined}<span class="val">{format(clamped, max)}</span>{/if}
    </div>
  {/if}
  <div
    class="track"
    role="progressbar"
    aria-label={hideLabel && !showValue ? label : undefined}
    aria-labelledby={hideLabel && !showValue ? undefined : `${uid}-label`}
    aria-valuemin={0}
    aria-valuemax={max}
    aria-valuenow={clamped}
    aria-valuetext={clamped === undefined ? undefined : format(clamped, max)}
  >
    <div
      class="fill"
      class:indeterminate={clamped === undefined}
      style:width={clamped === undefined ? undefined : `${(clamped / max) * 100}%`}
    ></div>
  </div>
</div>

<style>
  .ds-progress { --c: var(--color-accent); display: grid; gap: var(--space-1); min-width: 0; }
  .info { --c: var(--color-info); }
  .ok { --c: var(--color-ok); }
  .warn { --c: var(--color-warn); }
  .danger { --c: var(--color-danger); }
  .head { display: flex; justify-content: space-between; gap: var(--space-2); font-size: var(--font-size-sm); }
  .val { color: var(--color-text-secondary); font-variant-numeric: tabular-nums; }
  .track { position: relative; height: 6px; border-radius: var(--radius-full); background: var(--color-surface-active); overflow: hidden; }
  .fill { height: 100%; border-radius: inherit; background: var(--c); transition: width var(--duration-slow) var(--ease-standard); }
  .indeterminate { position: absolute; width: 35%; animation: slide 1.3s var(--ease-standard) infinite; }
  @keyframes slide { from { left: -35%; } to { left: 100%; } }
  @media (prefers-reduced-motion: reduce) {
    .indeterminate { animation: none; left: 0; width: 100%; opacity: 0.45; }
  }
</style>
