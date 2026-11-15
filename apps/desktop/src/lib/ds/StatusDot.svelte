<script lang="ts">
  import type { Tone } from "./types.ts";

  let {
    tone = "neutral",
    /** Text next to the dot. Without it, `label` is read by screen readers only. */
    text,
    /** Screen-reader name when there is no visible text (e.g. "Connected"). */
    label,
    /** Gentle pulse for "live / working". Off under reduced motion. */
    pulse = false,
  }: {
    tone?: Tone;
    text?: string;
    label?: string;
    pulse?: boolean;
  } = $props();
</script>

<span class="ds-status {tone}" class:pulse>
  <span class="dot" aria-hidden="true"></span>
  {#if text}<span class="text">{text}</span>{:else if label}<span class="sr-only">{label}</span>{/if}
</span>

<style>
  .ds-status { --c: var(--color-neutral); display: inline-flex; align-items: center; gap: var(--space-1-5); font-size: var(--font-size-sm); }
  .accent { --c: var(--color-accent); }
  .info { --c: var(--color-info); }
  .ok { --c: var(--color-ok); }
  .warn { --c: var(--color-warn); }
  .danger { --c: var(--color-danger); }
  .dot { position: relative; flex: none; width: 8px; height: 8px; border-radius: var(--radius-full); background: var(--c); }
  .pulse .dot::after {
    content: "";
    position: absolute;
    inset: -3px;
    border-radius: inherit;
    border: var(--border-width-strong) solid var(--c);
    opacity: 0;
    animation: ping 1.6s var(--ease-out) infinite;
  }
  .text { color: var(--color-text-secondary); }
  @keyframes ping { 0% { opacity: 0.6; transform: scale(0.6); } 100% { opacity: 0; transform: scale(1.5); } }
  @media (prefers-reduced-motion: reduce) { .pulse .dot::after { animation: none; } }
</style>
