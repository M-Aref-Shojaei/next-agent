<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";

  type Props = Omit<HTMLButtonAttributes, "onchange" | "children" | "role"> & {
    checked?: boolean;
    /** Visible label next to the switch. In a <SettingsRow>, leave it out: the row's label names it. */
    label?: string;
    size?: "sm" | "md";
    onchange?: (checked: boolean) => void;
  };

  let {
    checked = $bindable(false),
    label,
    size = "md",
    disabled = false,
    onchange,
    class: className = "",
    ...rest
  }: Props = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

{#snippet track()}
  <button
    {...rest}
    type="button"
    role="switch"
    aria-checked={checked}
    {disabled}
    class="ds-switch {size} {label ? '' : className}"
    onclick={toggle}
  ><span class="thumb"></span></button>
{/snippet}

{#if label}
  <!-- svelte-ignore a11y_label_has_associated_control -->
  <label class="ds-switch-label {className}" class:disabled>
    {@render track()}
    <span>{label}</span>
  </label>
{:else}
  {@render track()}
{/if}

<style>
  .ds-switch {
    --w: var(--switch-width);
    --h: var(--switch-height);
    --pad: var(--space-0-5);
    position: relative;
    flex: none;
    width: var(--w);
    height: var(--h);
    padding: 0;
    border: 0;
    border-radius: var(--radius-full);
    background: var(--color-border-strong);
    cursor: pointer;
    transition: background-color var(--duration-normal) var(--ease-standard);
  }
  .sm { --w: var(--switch-sm-width); --h: var(--switch-sm-height); }
  .ds-switch:hover:not(:disabled) { background: var(--color-text-tertiary); }
  .ds-switch[aria-checked="true"] { background: var(--color-accent); }
  .ds-switch[aria-checked="true"]:hover:not(:disabled) { background: var(--color-accent-hover); }
  .ds-switch:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .ds-switch:disabled { opacity: 0.45; cursor: not-allowed; }
  .thumb {
    position: absolute;
    top: var(--pad);
    left: var(--pad);
    width: calc(var(--h) - 2 * var(--pad));
    height: calc(var(--h) - 2 * var(--pad));
    border-radius: var(--radius-full);
    background: var(--color-control-thumb);
    box-shadow: var(--shadow-sm);
    transition: transform var(--duration-normal) var(--ease-standard);
  }
  [aria-checked="true"] .thumb { transform: translateX(calc(var(--w) - var(--h))); }
  .ds-switch-label { display: inline-flex; align-items: center; gap: var(--space-2); cursor: pointer; }
  .ds-switch-label.disabled { cursor: not-allowed; color: var(--color-text-disabled); }
</style>
