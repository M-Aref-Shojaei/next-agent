<script lang="ts">
  import type { Snippet } from "svelte";
  import { describedBy, type FieldControl } from "./types.ts";

  let {
    label,
    description,
    error,
    /** Put the control under the text (wide controls: text areas, lists). */
    stacked = false,
    disabled = false,
    /** Id for the control; generated when omitted. */
    id,
    children,
  }: {
    label: string;
    description?: string;
    error?: string;
    stacked?: boolean;
    disabled?: boolean;
    id?: string;
    /**
     * The control, given {id, describedby, invalid, labelId}. Put `id` on a
     * single control (Switch, TextField, Select, ShortcutField) and
     * `labelledby={labelId}` on a group (SegmentedControl, ColorSwatchGroup).
     */
    children?: Snippet<[FieldControl]>;
  } = $props();

  const uid = $props.id();
  const controlId = $derived(id ?? `${uid}-control`);
  const descId = $derived(description ? `${controlId}-desc` : undefined);
  const errorId = $derived(error ? `${controlId}-error` : undefined);
</script>

<div class="ds-settings-row" class:stacked class:disabled>
  <div class="text">
    <label for={controlId} id="{controlId}-label">{label}</label>
    {#if description}<p class="desc" id={descId}>{description}</p>{/if}
    {#if error}<p class="error" id={errorId}>{error}</p>{/if}
  </div>
  {#if children}
    <div class="control">
      {@render children({
        id: controlId,
        describedby: describedBy(errorId, descId),
        invalid: !!error,
        labelId: `${controlId}-label`,
      })}
    </div>
  {/if}
</div>

<style>
  .ds-settings-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-2) var(--space-6);
    min-height: 52px;
    padding: var(--space-3) var(--space-4);
  }
  .stacked { grid-template-columns: minmax(0, 1fr); }
  .disabled { opacity: 0.55; }
  .text { display: grid; gap: var(--space-0-5); min-width: 0; }
  label { font-weight: var(--font-weight-medium); color: var(--color-text); }
  p { margin: 0; font-size: var(--font-size-sm); line-height: var(--line-height-normal); }
  .desc { color: var(--color-text-secondary); }
  .error { color: var(--color-danger-text); }
  .control { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-2); min-width: 0; flex-wrap: wrap; }
  /* Text inputs get a usable width beside the label. */
  .control > :global(.ds-input) { width: 260px; max-width: 100%; }
  .stacked .control { justify-content: stretch; }
  .stacked .control > :global(*) { flex: 1; }
  @container (max-width: 480px) {
    .ds-settings-row { grid-template-columns: minmax(0, 1fr); }
    .control { justify-content: flex-start; }
  }
</style>
