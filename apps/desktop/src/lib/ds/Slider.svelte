<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  type Props = Omit<HTMLInputAttributes, "type" | "value" | "min" | "max" | "step" | "onchange"> & {
    value?: number;
    min?: number;
    max?: number;
    step?: number;
    /** Shows the current value at the end. */
    showValue?: boolean;
    /** Formats the shown value and aria-valuetext (e.g. v => `${v}%`). */
    format?: (value: number) => string;
    onchange?: (value: number) => void;
  };

  let {
    value = $bindable(0),
    min = 0,
    max = 100,
    step = 1,
    showValue = true,
    format = String,
    onchange,
    class: className = "",
    ...rest
  }: Props = $props();

  const pct = $derived(max > min ? ((value - min) / (max - min)) * 100 : 0);
</script>

<div class="ds-slider {className}">
  <input
    {...rest}
    type="range"
    {min}
    {max}
    {step}
    aria-valuetext={format(value)}
    style:--pct="{pct}%"
    bind:value
    onchange={() => onchange?.(value)}
  />
  {#if showValue}<output class="value" aria-hidden="true">{format(value)}</output>{/if}
</div>

<style>
  .ds-slider { display: flex; align-items: center; gap: var(--space-3); min-width: 0; }
  input {
    flex: 1;
    min-width: calc(2 * var(--space-10));
    height: var(--space-5);
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    appearance: none;
    cursor: pointer;
  }
  input:disabled { cursor: not-allowed; opacity: 0.5; }
  input:focus-visible { outline: none; }
  input::-webkit-slider-runnable-track {
    height: var(--space-1);
    border-radius: var(--radius-full);
    background: linear-gradient(to right, var(--color-accent) var(--pct), var(--color-border-strong) var(--pct));
  }
  input::-moz-range-track { height: var(--space-1); border-radius: var(--radius-full); background: var(--color-border-strong); }
  input::-moz-range-progress { height: var(--space-1); border-radius: var(--radius-full); background: var(--color-accent); }
  input::-webkit-slider-thumb {
    appearance: none;
    width: var(--space-4);
    height: var(--space-4);
    margin-top: calc(-1 * var(--space-1-5));
    border-radius: var(--radius-full);
    background: var(--color-control-thumb);
    border: var(--border-width) solid var(--color-border-strong);
    box-shadow: var(--shadow-md);
    transition: box-shadow var(--duration-fast) var(--ease-standard);
  }
  input::-moz-range-thumb {
    width: var(--space-4);
    height: var(--space-4);
    border-radius: var(--radius-full);
    background: var(--color-control-thumb);
    border: var(--border-width) solid var(--color-border-strong);
  }
  input:focus-visible::-webkit-slider-thumb { box-shadow: var(--focus-ring); }
  input:focus-visible::-moz-range-thumb { box-shadow: var(--focus-ring); }
  .value {
    min-width: 4ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
  }
</style>
