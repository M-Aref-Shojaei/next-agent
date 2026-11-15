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
    --bg: var(--color-neutral-subtle);
    --fg: var(--color-neutral-text);
    --solid: var(--color-neutral);
    --on: var(--color-text-inverse);
    --line: var(--color-neutral-border);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: 20px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-full);
    border: var(--border-width) solid transparent;
    background: var(--bg);
    color: var(--fg);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    line-height: 1;
    white-space: nowrap;
    vertical-align: middle;
  }
  .sm { height: 16px; padding: 0 var(--space-1-5); }
  .accent { --bg: var(--color-accent-subtle); --fg: var(--color-accent-text); --solid: var(--color-accent); --on: var(--color-on-accent); --line: var(--color-accent); }
  .info { --bg: var(--color-info-subtle); --fg: var(--color-info-text); --solid: var(--color-info); --on: var(--color-on-info); --line: var(--color-info-border); }
  .ok { --bg: var(--color-ok-subtle); --fg: var(--color-ok-text); --solid: var(--color-ok); --on: var(--color-on-ok); --line: var(--color-ok-border); }
  .warn { --bg: var(--color-warn-subtle); --fg: var(--color-warn-text); --solid: var(--color-warn); --on: var(--color-on-warn); --line: var(--color-warn-border); }
  .danger { --bg: var(--color-danger-subtle); --fg: var(--color-danger-text); --solid: var(--color-danger); --on: var(--color-on-danger); --line: var(--color-danger-border); }
  .solid { background: var(--solid); color: var(--on); }
  .outline { background: transparent; border-color: var(--line); }
</style>
