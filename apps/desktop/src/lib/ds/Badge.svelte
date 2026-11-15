<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Tone } from "./types.ts";

  let {
    tone = "neutral",
    /** "subtle" (tinted, default) or "solid" (counts, "new"). */
    variant = "subtle",
    size = "md",
    children,
  }: {
    tone?: Tone;
    variant?: "subtle" | "solid" | "outline";
    size?: "sm" | "md";
    children: Snippet;
  } = $props();
</script>

<span class="ds-badge {tone} {variant} {size}">{@render children()}</span>

<style>
  .ds-badge {
    --t-bg: var(--color-neutral-subtle);
    --t-fg: var(--color-neutral-text);
    --t-solid: var(--color-neutral);
    --t-on: var(--color-text-inverse);
    --t-line: var(--color-neutral-border);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: 20px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-full);
    border: var(--border-width) solid transparent;
    background: var(--t-bg);
    color: var(--t-fg);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    line-height: 1;
    white-space: nowrap;
    vertical-align: middle;
  }
  .sm { height: 16px; padding: 0 var(--space-1-5); }
  .accent { --t-bg: var(--color-accent-subtle); --t-fg: var(--color-accent-text); --t-solid: var(--color-accent); --t-on: var(--color-on-accent); --t-line: var(--color-accent); }
  .info { --t-bg: var(--color-info-subtle); --t-fg: var(--color-info-text); --t-solid: var(--color-info); --t-on: var(--color-on-info); --t-line: var(--color-info-border); }
  .ok { --t-bg: var(--color-ok-subtle); --t-fg: var(--color-ok-text); --t-solid: var(--color-ok); --t-on: var(--color-on-ok); --t-line: var(--color-ok-border); }
  .warn { --t-bg: var(--color-warn-subtle); --t-fg: var(--color-warn-text); --t-solid: var(--color-warn); --t-on: var(--color-on-warn); --t-line: var(--color-warn-border); }
  .danger { --t-bg: var(--color-danger-subtle); --t-fg: var(--color-danger-text); --t-solid: var(--color-danger); --t-on: var(--color-on-danger); --t-line: var(--color-danger-border); }
  .solid { background: var(--t-solid); color: var(--t-on); }
  .outline { background: transparent; border-color: var(--t-line); }
</style>
