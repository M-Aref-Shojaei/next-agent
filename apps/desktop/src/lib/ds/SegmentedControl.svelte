<script lang="ts" generics="V extends string">
  import { rovingIndex } from "./keyboard.ts";
  import type { Option } from "./types.ts";

  let {
    value = $bindable(),
    options,
    /** Accessible name of the group, when no visible label names it. */
    label,
    /** Id of a visible label (a SettingsRow's `labelId`); used instead of `label`. */
    labelledby,
    size = "md",
    disabled = false,
    /** Stretch segments to fill the width. */
    block = false,
    onchange,
  }: {
    value?: V;
    options: readonly Option<V>[];
    label?: string;
    labelledby?: string;
    size?: "sm" | "md";
    disabled?: boolean;
    block?: boolean;
    onchange?: (value: V) => void;
  } = $props();

  let buttons: HTMLButtonElement[] = $state([]);
  const selected = $derived(Math.max(0, options.findIndex((o) => o.value === value)));

  function choose(i: number) {
    const opt = options[i];
    if (!opt || opt.disabled || disabled) return;
    value = opt.value;
    onchange?.(opt.value);
  }

  function onkeydown(event: KeyboardEvent, i: number) {
    const next = rovingIndex(event.key, i, options.map((o) => !!o.disabled || disabled));
    if (next === null) return;
    event.preventDefault();
    choose(next);
    buttons[next]?.focus();
  }
</script>

<div
  class="ds-seg {size}"
  class:block
  role="radiogroup"
  aria-label={labelledby ? undefined : label}
  aria-labelledby={labelledby}
  aria-disabled={disabled || undefined}
>
  {#each options as opt, i (opt.value)}
    <button
      bind:this={buttons[i]}
      type="button"
      role="radio"
      aria-checked={i === selected && value !== undefined}
      tabindex={i === selected ? 0 : -1}
      disabled={disabled || opt.disabled}
      title={opt.description}
      onclick={() => choose(i)}
      onkeydown={(e) => onkeydown(e, i)}
    >{opt.label}</button>
  {/each}
</div>

<style>
  .ds-seg {
    display: inline-flex;
    gap: var(--space-0-5);
    padding: var(--space-0-5);
    width: fit-content;
    min-width: 0;
    max-width: 100%;
    overflow-x: auto;
    height: var(--control-height-md);
    background: var(--color-surface-sunken);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
  }
  .sm { height: var(--control-height-sm); }
  .block { display: flex; width: 100%; }
  .block button { flex: 1; }
  button {
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);
  }
  .sm button { padding: 0 var(--space-2); font-size: var(--font-size-xs); }
  button:hover:not(:disabled) { color: var(--color-text); border-color: transparent; }
  button[aria-checked="true"] {
    background: var(--color-surface-raised);
    color: var(--color-text);
    box-shadow: var(--shadow-sm), 0 0 0 var(--border-width) var(--color-border);
  }
  button:focus-visible { outline: none; box-shadow: var(--focus-ring-inset); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
</style>
