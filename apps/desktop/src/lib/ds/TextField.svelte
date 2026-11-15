<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";
  import Field from "./Field.svelte";
  import type { FieldControl, Size } from "./types.ts";

  type Props = Omit<HTMLInputAttributes, "size" | "value" | "prefix"> & {
    /** Bindable. `null` and `undefined` show an empty box, so optional settings bind directly. */
    value?: string | number | null | undefined;
    /** Visible label (wraps the input in <Field>). Without it, pass aria-label. */
    label?: string;
    hint?: string;
    error?: string;
    hideLabel?: boolean;
    size?: Size;
    /** Content inside the box before the text (icon, unit). */
    prefix?: Snippet;
    /** Content inside the box after the text (unit, IconButton). */
    suffix?: Snippet;
    /** The <input> element, bindable: focus it, select it, measure it. */
    ref?: HTMLInputElement | null;
    /** Monospace text (paths, keys, URLs). */
    mono?: boolean;
  };

  let {
    value = $bindable(),
    label,
    hint,
    error,
    hideLabel = false,
    size = "md",
    prefix,
    suffix,
    mono = false,
    ref = $bindable(),
    id,
    required,
    class: className = "",
    ...rest
  }: Props = $props();
</script>

{#snippet input(ctl: FieldControl | undefined)}
  <div class="ds-input {size} {className}" class:invalid={!!error} class:disabled={rest.disabled}>
    {#if prefix}<span class="affix">{@render prefix()}</span>{/if}
    <input
      {...rest}
      bind:this={ref}
      id={ctl?.id ?? id}
      class:mono
      {required}
      aria-invalid={!!error || undefined}
      aria-describedby={ctl?.describedby ?? rest["aria-describedby"] ?? undefined}
      bind:value
    />
    {#if suffix}<span class="affix">{@render suffix()}</span>{/if}
  </div>
{/snippet}

{#if label}
  <Field {label} {hint} {error} {hideLabel} id={id ?? undefined} required={!!required}>
    {#snippet children(ctl)}{@render input(ctl)}{/snippet}
  </Field>
{:else}
  {@render input(undefined)}
{/if}

<style>
  .ds-input {
    width: var(--ds-control-width, auto);
    max-width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-1-5);
    min-width: 0;
    height: var(--control-height-md);
    padding: 0 var(--space-2);
    background: var(--color-surface);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    transition: border-color var(--duration-fast) var(--ease-standard), box-shadow var(--duration-fast) var(--ease-standard);
  }
  .sm { height: var(--control-height-sm); font-size: var(--font-size-sm); }
  .lg { height: var(--control-height-lg); }
  .ds-input:hover { border-color: var(--color-border-strong); }
  .ds-input:focus-within { border-color: var(--color-accent); box-shadow: var(--field-focus-ring); }
  .ds-input.invalid { border-color: var(--color-danger); }
  .ds-input.invalid:focus-within { box-shadow: var(--field-focus-ring-invalid); }
  .ds-input.disabled { background: var(--color-surface-sunken); opacity: 0.7; }
  input {
    flex: 1;
    min-width: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    outline: none;
  }
  input:focus-visible { outline: none; }
  input.mono { font-family: var(--font-mono); font-size: var(--font-size-sm); }
  .affix { display: inline-flex; align-items: center; color: var(--color-text-tertiary); font-size: var(--font-size-sm); }
</style>
