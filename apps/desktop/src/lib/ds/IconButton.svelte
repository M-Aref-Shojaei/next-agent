<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import type { Size } from "./types.ts";

  type Props = Omit<HTMLButtonAttributes, "children"> & {
    /** Required: the accessible name, also shown as the native tooltip. */
    label: string;
    variant?: "ghost" | "secondary" | "danger";
    /** "xs" is 20px, for dense rows (an outline twisty). */
    size?: "xs" | Size;
    /** Toggle buttons: sets aria-pressed. */
    pressed?: boolean;
    children: Snippet;
  };

  let {
    label,
    variant = "ghost",
    size = "md",
    pressed,
    type = "button",
    class: className = "",
    children,
    ...rest
  }: Props = $props();
</script>

<button
  {...rest}
  {type}
  class="ds-icon-btn {variant} {size} {className}"
  aria-label={label}
  title={rest.title ?? label}
  aria-pressed={pressed}
>{@render children()}</button>

<style>
  .ds-icon-btn {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: var(--control-height-md);
    height: var(--control-height-md);
    padding: 0;
    border: var(--border-width) solid transparent;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: background-color var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);
  }
  .xs { width: var(--control-height-xs); height: var(--control-height-xs); border-radius: var(--radius-sm); }
  .sm { width: var(--control-height-sm); height: var(--control-height-sm); border-radius: var(--radius-sm); }
  .lg { width: var(--control-height-lg); height: var(--control-height-lg); }
  .ds-icon-btn:hover:not(:disabled) { background: var(--color-surface-hover); color: var(--color-text); border-color: transparent; }
  .ds-icon-btn:active:not(:disabled) { background: var(--color-surface-active); }
  .ds-icon-btn:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .secondary { background: var(--color-surface); border-color: var(--color-border); }
  .secondary:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .danger:hover:not(:disabled) { color: var(--color-danger-text); background: var(--color-danger-subtle); }
  .ds-icon-btn[aria-pressed="true"] { background: var(--color-accent-subtle); color: var(--color-accent-text); }
  .ds-icon-btn:disabled { opacity: 0.45; cursor: not-allowed; }
</style>
