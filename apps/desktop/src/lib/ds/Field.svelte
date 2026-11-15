<script lang="ts">
  import type { Snippet } from "svelte";
  import { describedBy, type FieldControl } from "./types.ts";

  let {
    label,
    hint,
    error,
    required = false,
    /** Hide the label visually (still read by screen readers). */
    hideLabel = false,
    /** Id for the control; generated when omitted. */
    id,
    children,
  }: {
    label: string;
    hint?: string;
    /** Shown in place of nothing below the control; marks the control invalid. */
    error?: string;
    required?: boolean;
    hideLabel?: boolean;
    id?: string;
    /** Render the control with the ids it needs: {id, describedby, invalid}. */
    children: Snippet<[FieldControl]>;
  } = $props();

  const uid = $props.id();
  const controlId = $derived(id ?? `${uid}-control`);
  const hintId = $derived(hint ? `${controlId}-hint` : undefined);
  const errorId = $derived(error ? `${controlId}-error` : undefined);
</script>

<div class="ds-field" class:invalid={!!error}>
  <label for={controlId} id="{controlId}-label" class:sr-only={hideLabel}>
    {label}{#if required}<span class="req" aria-hidden="true">*</span>{/if}
  </label>
  {@render children({
    id: controlId,
    describedby: describedBy(errorId, !error && hintId),
    invalid: !!error,
    labelId: `${controlId}-label`,
  })}
  {#if error}
    <p class="error" id={errorId}>{error}</p>
  {:else if hint}
    <p class="hint" id={hintId}>{hint}</p>
  {/if}
</div>

<style>
  .ds-field { display: grid; gap: var(--space-1); min-width: 0; }
  label { font-size: var(--font-size-sm); font-weight: var(--font-weight-medium); color: var(--color-text); }
  .req { color: var(--color-danger-text); margin-left: var(--space-0-5); }
  p { margin: 0; font-size: var(--font-size-sm); line-height: var(--line-height-normal); }
  .hint { color: var(--color-text-secondary); }
  .error { color: var(--color-danger-text); }
</style>
