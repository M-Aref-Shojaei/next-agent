<script lang="ts">
  import { tick } from "svelte";
  import type { HTMLTextareaAttributes } from "svelte/elements";
  import Field from "./Field.svelte";
  import type { FieldControl } from "./types.ts";

  type Props = Omit<HTMLTextareaAttributes, "value"> & {
    /** Bindable. `null` and `undefined` show an empty box. */
    value?: string | null | undefined;
    /** Visible label (wraps in <Field>). Without it, pass aria-label. */
    label?: string;
    hint?: string;
    error?: string;
    hideLabel?: boolean;
    mono?: boolean;
    /** Grows with its content instead of scrolling. `rows` is the minimum height. */
    autogrow?: boolean;
    /** Tight padding and a small radius, to edit text in place (an outline block). */
    compact?: boolean;
    /** The <textarea> element, bindable. */
    ref?: HTMLTextAreaElement | null;
    /** Allow the user to resize vertically (default) or not at all. */
    resize?: "vertical" | "none";
  };

  let {
    value = $bindable(),
    label,
    hint,
    error,
    hideLabel = false,
    mono = false,
    resize = "vertical",
    autogrow = false,
    compact = false,
    ref = $bindable(),
    rows = 4,
    id,
    required,
    class: className = "",
    ...rest
  }: Props = $props();

  /** Fits the height to the content. Call it after changing `ref.value` from code (typing and `value` are handled). */
  export function fit() {
    if (!autogrow || !ref) return;
    ref.style.height = "auto";
    // scrollHeight leaves the border out; the box is border-box.
    ref.style.height = `${ref.scrollHeight + ref.offsetHeight - ref.clientHeight}px`;
  }

  $effect(() => {
    void value;
    void rows;
    if (autogrow) void tick().then(fit);
    else if (ref) ref.style.height = "";
  });

  // Wrapping changes with the width, so refit when the box is resized.
  $effect(() => {
    if (!autogrow || !ref || typeof ResizeObserver === "undefined") return;
    let width = ref.offsetWidth;
    const observer = new ResizeObserver(() => {
      if (ref && ref.offsetWidth !== width) { width = ref.offsetWidth; fit(); }
    });
    observer.observe(ref);
    return () => observer.disconnect();
  });
</script>

{#snippet area(ctl: FieldControl | undefined)}
  <textarea
    {...rest}
    bind:this={ref}
    oninput={(e) => { fit(); rest.oninput?.(e); }}
    {rows}
    {required}
    id={ctl?.id ?? id}
    class="ds-textarea {className}"
    class:mono
    class:compact
    class:autogrow
    class:invalid={!!error}
    style:resize={autogrow ? "none" : resize}
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
  .compact { min-height: 0; padding: 0 var(--space-1); border-radius: var(--radius-sm); }
  .autogrow { overflow: hidden; min-height: 0; }
  .mono { font-family: var(--font-mono); font-size: var(--font-size-sm); }
</style>
