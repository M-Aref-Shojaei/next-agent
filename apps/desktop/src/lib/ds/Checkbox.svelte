<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  type Props = Omit<HTMLInputAttributes, "type" | "checked" | "onchange"> & {
    checked?: boolean;
    /** "Some but not all" state (e.g. a select-all box). */
    indeterminate?: boolean;
    /** Visible label. Without it, pass aria-label. */
    label?: string;
    description?: string;
    onchange?: (checked: boolean) => void;
  };

  let {
    checked = $bindable(false),
    indeterminate = $bindable(false),
    label,
    description,
    disabled = false,
    onchange,
    class: className = "",
    ...rest
  }: Props = $props();

  const uid = $props.id();
</script>

<label class="ds-check {className}" class:disabled>
  <input
    {...rest}
    type="checkbox"
    {disabled}
    aria-describedby={description ? `${uid}-desc` : rest["aria-describedby"]}
    bind:checked
    bind:indeterminate
    onchange={() => onchange?.(checked)}
  />
  <span class="box" aria-hidden="true">
    <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
      {#if indeterminate}<path d="M4 8h8" />{:else}<path d="M3.5 8.5l3 3 6-7" />{/if}
    </svg>
  </span>
  {#if label || description}
    <span class="text">
      {#if label}<span class="label">{label}</span>{/if}
      {#if description}<span class="desc" id="{uid}-desc">{description}</span>{/if}
    </span>
  {/if}
</label>

<style>
  .ds-check { position: relative; display: inline-flex; align-items: flex-start; gap: var(--space-2); cursor: pointer; }
  .ds-check.disabled { cursor: not-allowed; opacity: 0.55; }
  input {
    position: absolute;
    opacity: 0;
    width: 16px;
    height: 16px;
    margin: 0;
    cursor: inherit;
  }
  .box {
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    margin-top: 2px;
    border: var(--border-width) solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    color: transparent;
    transition: background-color var(--duration-fast) var(--ease-standard), border-color var(--duration-fast) var(--ease-standard);
  }
  .ds-check:hover .box { border-color: var(--color-accent); }
  input:checked + .box, input:indeterminate + .box {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-on-accent);
  }
  input:focus-visible + .box { box-shadow: var(--focus-ring); }
  .text { display: grid; gap: var(--space-0-5); line-height: var(--line-height-normal); }
  .desc { color: var(--color-text-secondary); font-size: var(--font-size-sm); }
</style>
