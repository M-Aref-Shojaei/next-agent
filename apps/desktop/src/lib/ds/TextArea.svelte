<script lang="ts">
  import type { HTMLTextareaAttributes } from "svelte/elements";
  import Field from "./Field.svelte";
  import type { FieldControl } from "./types.ts";

  type Props = Omit<HTMLTextareaAttributes, "value"> & {
    value?: string | null;
    /** Visible label (wraps in <Field>). Without it, pass aria-label. */
    label?: string;
    hint?: string;
    error?: string;
    hideLabel?: boolean;
    mono?: boolean;
    /** Allow the user to resize vertically (default) or not at all. */
    resize?: "vertical" | "none";
  };

  let {
    value = $bindable(""),
    label,
    hint,
    error,
    hideLabel = false,
    mono = false,
    resize = "vertical",
    rows = 4,
    id,
    required,
    class: className = "",
    ...rest
  }: Props = $props();
</script>

{#snippet area(ctl: FieldControl | undefined)}
  <textarea
    {...rest}
    {rows}
    {required}
    id={ctl?.id ?? id}
    class="ds-textarea {className}"
    class:mono
    class:invalid={!!error}
    style:resize
    aria-invalid={!!error || undefined}
    aria-describedby={ctl?.describedby ?? rest["aria-describedby"] ?? undefined}
    bind:value
  ></textarea>
{/snippet}

{#if label}
  <Field {label} {hint} {error} {hideLabel} id={id ?? undefined} required={!!required}>
    {#snippet children(ctl)}{@render area(ctl)}{/snippet}
  </Field>
{:else}
  {@render area(undefined)}
{/if}

<style>
  .ds-textarea {
    display: block;
    width: 100%;
    min-height: calc(var(--control-height-md) * 2);
    padding: var(--space-1-5) var(--space-2);
    background: var(--color-surface);
    color: var(--color-text);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    font: inherit;
    line-height: var(--line-height-normal);
    transition: border-color var(--duration-fast) var(--ease-standard), box-shadow var(--duration-fast) var(--ease-standard);
  }
  .ds-textarea:hover { border-color: var(--color-border-strong); }
  .ds-textarea:focus-visible, .ds-textarea:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: var(--field-focus-ring);
  }
  .ds-textarea.invalid { border-color: var(--color-danger); }
  .ds-textarea:disabled { background: var(--color-surface-sunken); opacity: 0.7; }
  .mono { font-family: var(--font-mono); font-size: var(--font-size-sm); }
</style>
