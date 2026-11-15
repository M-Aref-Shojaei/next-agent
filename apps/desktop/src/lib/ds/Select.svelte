<script lang="ts" generics="V extends string">
  import type { HTMLSelectAttributes } from "svelte/elements";
  import Field from "./Field.svelte";
  import Icon from "./Icon.svelte";
  import type { FieldControl, Option, Size } from "./types.ts";

  type Props = Omit<HTMLSelectAttributes, "value" | "size"> & {
    value?: V;
    options: readonly Option<V>[];
    /** Visible label (wraps in <Field>). Without it, pass aria-label. */
    label?: string;
    hint?: string;
    error?: string;
    hideLabel?: boolean;
    /** Unselectable first entry shown while no value is chosen. */
    placeholder?: string;
    size?: Size;
  };

  let {
    value = $bindable(),
    options,
    label,
    hint,
    error,
    hideLabel = false,
    placeholder,
    size = "md",
    id,
    required,
    class: className = "",
    ...rest
  }: Props = $props();
</script>

{#snippet control(ctl: FieldControl | undefined)}
  <div class="ds-select {size} {className}" class:invalid={!!error}>
    <select
      {...rest}
      {required}
      id={ctl?.id ?? id}
      aria-invalid={!!error || undefined}
      aria-describedby={ctl?.describedby ?? rest["aria-describedby"] ?? undefined}
      bind:value
    >
      {#if placeholder}<option value="" disabled selected={value === undefined || value === ""}>{placeholder}</option>{/if}
      {#each options as opt (opt.value)}
        <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
      {/each}
    </select>
    <span class="chev"><Icon name="chevron-down" size={14} /></span>
  </div>
{/snippet}

{#if label}
  <Field {label} {hint} {error} {hideLabel} id={id ?? undefined} required={!!required}>
    {#snippet children(ctl)}{@render control(ctl)}{/snippet}
  </Field>
{:else}
  {@render control(undefined)}
{/if}

<style>
  .ds-select { position: relative; display: flex; width: var(--ds-control-width, auto); max-width: 100%; min-width: 0; }
  select {
    appearance: none;
    width: 100%;
    height: var(--control-height-md);
    padding: 0 calc(var(--space-2) + 18px) 0 var(--space-2);
    background: var(--color-surface);
    color: var(--color-text);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    font: inherit;
    cursor: pointer;
    transition: border-color var(--duration-fast) var(--ease-standard), box-shadow var(--duration-fast) var(--ease-standard);
  }
  .sm select { height: var(--control-height-sm); font-size: var(--font-size-sm); }
  .lg select { height: var(--control-height-lg); }
  select:hover { border-color: var(--color-border-strong); }
  select:focus-visible { outline: none; border-color: var(--color-accent); box-shadow: var(--field-focus-ring); }
  .invalid select { border-color: var(--color-danger); }
  select:disabled { background: var(--color-surface-sunken); opacity: 0.7; cursor: not-allowed; }
  .chev {
    position: absolute;
    right: var(--space-2);
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    color: var(--color-text-tertiary);
    pointer-events: none;
  }
</style>
