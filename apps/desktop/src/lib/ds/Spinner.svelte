<script lang="ts">
  import type { Size } from "./types.ts";

  let {
    size = "md",
    /** Announced to screen readers. Pass "" when a parent already announces (e.g. a loading Button). */
    label = "Loading",
  }: { size?: Size; label?: string } = $props();
</script>

{#if label}
  <span class="spinner {size}" role="status"><span class="sr-only">{label}</span></span>
{:else}
  <span class="spinner {size}" aria-hidden="true"></span>
{/if}

<style>
  .spinner {
    display: inline-block;
    flex: none;
    width: var(--space-4);
    height: var(--space-4);
    border-radius: var(--radius-full);
    border: var(--border-width-strong) solid currentColor;
    border-right-color: transparent;
    opacity: 0.8;
    animation: spin 0.7s linear infinite;
    vertical-align: middle;
  }
  .sm { width: var(--space-3); height: var(--space-3); }
  .lg { width: var(--space-6); height: var(--space-6); border-width: calc(var(--space-0-5) + var(--border-width)); }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    .spinner { animation-duration: 2.4s; }
  }
</style>
