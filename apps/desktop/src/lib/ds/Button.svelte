<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Spinner from "./Spinner.svelte";
  import type { Size } from "./types.ts";

  type Props = Omit<HTMLButtonAttributes, "children"> & {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: Size;
    /** Shows a spinner, sets aria-busy and blocks clicks; the label stays for layout. */
    loading?: boolean;
    /** Toggle button: sets aria-pressed and shows the on state (a tinted accent). */
    pressed?: boolean;
    /** Stretches to the container's width. */
    block?: boolean;
    /** Leading icon (e.g. <Icon name="plus" />). */
    icon?: Snippet;
    /** Trailing icon. */
    iconEnd?: Snippet;
    children?: Snippet;
  };

  let {
    variant = "secondary",
    size = "md",
    loading = false,
    block = false,
    pressed,
    disabled = false,
    type = "button",
    icon,
    iconEnd,
    children,
    class: className = "",
    onclick,
    ...rest
  }: Props = $props();
</script>

<button
  {...rest}
  {type}
  class="ds-btn {variant} {size} {className}"
  class:block
  class:loading
  disabled={disabled || undefined}
  aria-disabled={loading || undefined}
  aria-busy={loading || undefined}
  aria-pressed={pressed}
  onclick={(e) => {
    if (loading) { e.preventDefault(); return; }
    onclick?.(e);
  }}
>
  {#if loading}<span class="spin"><Spinner size="sm" label="" /></span>{/if}
  {#if icon}<span class="ico">{@render icon()}</span>{/if}
  {#if children}<span class="label">{@render children()}</span>{/if}
  {#if iconEnd}<span class="ico">{@render iconEnd()}</span>{/if}
</button>

<style>
  .ds-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1-5);
    height: var(--control-height-md);
    padding: 0 var(--space-3);
    border: var(--border-width) solid transparent;
    border-radius: var(--radius-md);
    font: inherit;
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    line-height: 1;
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
    transition:
      background-color var(--duration-fast) var(--ease-standard),
      border-color var(--duration-fast) var(--ease-standard),
      color var(--duration-fast) var(--ease-standard),
      box-shadow var(--duration-fast) var(--ease-standard);
  }
  .ds-btn:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .sm { height: var(--control-height-sm); padding: 0 var(--space-2); font-size: var(--font-size-sm); gap: var(--space-1); }
  .lg { height: var(--control-height-lg); padding: 0 var(--space-4); }
  .block { display: flex; width: 100%; }

  .primary { background: var(--color-accent); border-color: var(--color-accent); color: var(--color-on-accent); }
  .primary:hover:not(:disabled) { background: var(--color-accent-hover); border-color: var(--color-accent-hover); }
  .primary:active:not(:disabled) { background: var(--color-accent-active); }

  .secondary { background: var(--color-surface); border-color: var(--color-border); color: var(--color-text); box-shadow: var(--shadow-sm); }
  .secondary:hover:not(:disabled) { background: var(--color-surface-hover); border-color: var(--color-border-strong); }
  .secondary:active:not(:disabled) { background: var(--color-surface-active); }

  .ghost { background: transparent; color: var(--color-text); }
  .ghost:hover:not(:disabled) { background: var(--color-surface-hover); border-color: transparent; }
  .ghost:active:not(:disabled) { background: var(--color-surface-active); }

  .danger { background: var(--color-danger); border-color: var(--color-danger); color: var(--color-on-danger); }
  .danger:hover:not(:disabled) { filter: brightness(1.06); border-color: var(--color-danger); }

  .ds-btn[aria-pressed="true"]:not(.primary):not(.danger) {
    background: var(--color-accent-subtle);
    border-color: var(--color-accent);
    color: var(--color-accent-text);
  }
  .ds-btn:disabled { opacity: 0.5; cursor: not-allowed; box-shadow: none; }
  .loading { cursor: progress; }
  .loading .label, .loading .ico { visibility: hidden; }
  .spin { position: absolute; inset: 0; display: grid; place-items: center; }
  .ico { display: inline-flex; }
</style>
