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
    width: 16px;
    height: 16px;
    border-radius: var(--radius-full);
    border: var(--border-width-strong) solid currentColor;
    border-right-color: transparent;
    opacity: 0.8;
    animation: spin 0.7s linear infinite;
    vertical-align: middle;
  }
  .sm { width: 12px; height: 12px; }
  .lg { width: 24px; height: 24px; border-width: 3px; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    .spinner { animation-duration: 2.4s; }
  }
</style>
