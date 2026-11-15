<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import type { StatusTone } from "./types.ts";

  let {
    tone = "info",
    title,
    /** Shows a close button; the caller removes the banner in ondismiss. */
    ondismiss,
    actions,
    children,
  }: {
    tone?: StatusTone;
    title?: string;
    ondismiss?: () => void;
    actions?: Snippet;
    children?: Snippet;
  } = $props();

  // Danger interrupts (alert); the others are polite status updates.
  const role = $derived(tone === "danger" ? "alert" : "status");
</script>

<div class="ds-banner {tone}" {role}>
  <span class="icon"><Icon name={tone} size={16} /></span>
  <div class="text">
    {#if title}<p class="title">{title}</p>{/if}
    {#if children}<div class="body">{@render children()}</div>{/if}
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </div>
  {#if ondismiss}
    <IconButton label="Dismiss" size="sm" onclick={ondismiss}><Icon name="x" size={14} /></IconButton>
  {/if}
</div>

<style>
  .ds-banner {
    --t-bg: var(--color-info-subtle);
    --t-fg: var(--color-info-text);
    --t-line: var(--color-info-border);
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: var(--border-width) solid var(--t-line);
    border-radius: var(--radius-md);
    background: var(--t-bg);
    color: var(--color-text);
    line-height: var(--line-height-normal);
  }
  .ok { --t-bg: var(--color-ok-subtle); --t-fg: var(--color-ok-text); --t-line: var(--color-ok-border); }
  .warn { --t-bg: var(--color-warn-subtle); --t-fg: var(--color-warn-text); --t-line: var(--color-warn-border); }
  .danger { --t-bg: var(--color-danger-subtle); --t-fg: var(--color-danger-text); --t-line: var(--color-danger-border); }
  .icon { display: inline-flex; padding-top: 2px; color: var(--t-fg); }
  .text { flex: 1; min-width: 0; display: grid; gap: var(--space-1); }
  .title { margin: 0; font-weight: var(--font-weight-semibold); color: var(--t-fg); }
  .body { font-size: var(--font-size-md); }
  .body :global(p) { margin: 0; }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; margin-top: var(--space-1); }
</style>
