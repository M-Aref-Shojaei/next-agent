<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    description,
    /** Small text before the actions (a date, a count). */
    meta,
    /** A native tooltip for the row (the full path behind a short name). */
    tooltip,
    selected = false,
    /** How a selected button row is announced: "pressed" (a toggle, default) or "current" (aria-current, for navigation lists). */
    selectedAs = "pressed",
    disabled = false,
    /** Makes the row's main area a button. Actions stay separate buttons. */
    onclick,
    /** Makes the row's main area a link. */
    href,
    leading,
    actions,
    children,
  }: {
    title: string;
    description?: string;
    meta?: string;
    tooltip?: string;
    selected?: boolean;
    selectedAs?: "pressed" | "current";
    disabled?: boolean;
    onclick?: () => void;
    href?: string;
    /** An icon, StatusDot or avatar. */
    leading?: Snippet;
    /** IconButtons / small Buttons. Shown on the row's right. */
    actions?: Snippet;
    /** Extra content under the description (badges, a path). */
    children?: Snippet;
  } = $props();
</script>

{#snippet body()}
  {#if leading}<span class="leading">{@render leading()}</span>{/if}
  <span class="text">
    <span class="title">{title}</span>
    {#if description}<span class="desc">{description}</span>{/if}
    {#if children}<span class="extra">{@render children()}</span>{/if}
  </span>
{/snippet}

<li class="ds-row" class:selected class:disabled>
  {#if href && !disabled}
    <a class="main interactive" {href} title={tooltip} aria-current={selected ? "page" : undefined}>{@render body()}</a>
  {:else if onclick}
    <button
      type="button"
      class="main interactive"
      {disabled}
      title={tooltip}
      aria-pressed={selectedAs === "pressed" ? selected : undefined}
      aria-current={selectedAs === "current" && selected ? "true" : undefined}
      {onclick}
    >{@render body()}</button>
  {:else}
    <div class="main" title={tooltip}>{@render body()}</div>
  {/if}
  {#if meta || actions}
    <span class="end">
      {#if meta}<span class="meta">{meta}</span>{/if}
      {#if actions}<span class="actions">{@render actions()}</span>{/if}
    </span>
  {/if}
</li>

<style>
  .ds-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    padding-right: var(--space-2);
    border-bottom: var(--border-width) solid var(--color-border-subtle);
    transition: background-color var(--duration-fast) var(--ease-standard);
  }
  .ds-row:last-child { border-bottom: 0; }
  .ds-row:hover { background: var(--color-surface-hover); }
  .selected { background: var(--color-accent-subtle); }
  .selected:hover { background: var(--color-accent-subtle); }
  .disabled { opacity: 0.55; }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 0;
    border-radius: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    text-decoration: none;
  }
  .interactive { cursor: pointer; }
  .interactive:hover { color: inherit; border-color: transparent; }
  .interactive:focus-visible { outline: none; box-shadow: var(--focus-ring-inset); }
  .leading { flex: none; display: inline-flex; color: var(--color-text-secondary); }
  .text { flex: 1; min-width: 0; display: grid; gap: var(--space-0-5); }
  .title { font-weight: var(--font-weight-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .desc { color: var(--color-text-secondary); font-size: var(--font-size-sm); overflow: hidden; text-overflow: ellipsis; }
  .extra { display: flex; flex-wrap: wrap; gap: var(--space-1); margin-top: var(--space-0-5); }
  .end { flex: none; display: flex; align-items: center; gap: var(--space-2); }
  .meta { color: var(--color-text-tertiary); font-size: var(--font-size-sm); font-variant-numeric: tabular-nums; }
  .actions { display: flex; align-items: center; gap: var(--space-0-5); }
</style>
