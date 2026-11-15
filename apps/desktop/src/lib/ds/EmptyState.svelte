<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    description,
    icon,
    actions,
    compact = false,
  }: {
    title: string;
    description?: string;
    icon?: Snippet;
    /** The one next step (usually a primary Button). */
    actions?: Snippet;
    /** Less padding, for empty lists inside cards. */
    compact?: boolean;
  } = $props();
</script>

<div class="ds-empty" class:compact>
  {#if icon}<div class="icon" aria-hidden="true">{@render icon()}</div>{/if}
  <p class="title">{title}</p>
  {#if description}<p class="desc">{description}</p>{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</div>

<style>
  .ds-empty {
    display: grid;
    justify-items: center;
    gap: var(--space-1-5);
    padding: var(--space-10) var(--space-4);
    text-align: center;
    color: var(--color-text-secondary);
  }
  .compact { padding: var(--space-5) var(--space-3); }
  .icon {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin-bottom: var(--space-1);
    border-radius: var(--radius-full);
    background: var(--color-surface-sunken);
    color: var(--color-text-tertiary);
  }
  p { margin: 0; max-width: 46ch; }
  .title { color: var(--color-text); font-weight: var(--font-weight-semibold); }
  .desc { font-size: var(--font-size-sm); line-height: var(--line-height-relaxed); }
  .actions { display: flex; gap: var(--space-2); margin-top: var(--space-2); }
</style>
