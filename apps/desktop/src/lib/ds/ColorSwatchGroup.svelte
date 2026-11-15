<script lang="ts" generics="V extends string">
  // A row of colour choices (e.g. Coucou's accent palette). The colours are the
  // user's data, passed in `color`; the control itself uses tokens only.
  import { rovingIndex } from "./keyboard.ts";

  interface Swatch {
    value: V;
    label: string;
    /** Any CSS colour: a token var(--…) or a value from settings data. */
    color: string;
  }

  let {
    value = $bindable(),
    swatches,
    label,
    labelledby,
    onchange,
  }: {
    value?: V;
    swatches: readonly Swatch[];
    /** Accessible name when no visible label names the group. */
    label?: string;
    labelledby?: string;
    onchange?: (value: V) => void;
  } = $props();

  let buttons: HTMLButtonElement[] = $state([]);
  const selected = $derived(Math.max(0, swatches.findIndex((s) => s.value === value)));

  function choose(i: number) {
    value = swatches[i].value;
    onchange?.(swatches[i].value);
  }

  function onkeydown(event: KeyboardEvent, i: number) {
    const next = rovingIndex(event.key, i, swatches.map(() => false));
    if (next === null) return;
    event.preventDefault();
    choose(next);
    buttons[next]?.focus();
  }
</script>

<div class="ds-swatches" role="radiogroup" aria-label={labelledby ? undefined : label} aria-labelledby={labelledby}>
  {#each swatches as s, i (s.value)}
    <button
      bind:this={buttons[i]}
      type="button"
      role="radio"
      aria-checked={s.value === value}
      aria-label={s.label}
      title={s.label}
      tabindex={i === selected ? 0 : -1}
      style:--swatch={s.color}
      onclick={() => choose(i)}
      onkeydown={(e) => onkeydown(e, i)}
    ></button>
  {/each}
</div>

<style>
  .ds-swatches { display: inline-flex; flex-wrap: wrap; gap: var(--space-1-5); }
  button {
    width: 22px;
    height: 22px;
    padding: 0;
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-full);
    background: var(--swatch);
    cursor: pointer;
    transition: transform var(--duration-fast) var(--ease-standard), box-shadow var(--duration-fast) var(--ease-standard);
  }
  button:hover { transform: scale(1.08); border-color: var(--color-border); }
  button[aria-checked="true"] { box-shadow: 0 0 0 2px var(--color-bg), 0 0 0 4px var(--swatch); }
  button:focus-visible { outline: none; box-shadow: var(--focus-ring); }
</style>
