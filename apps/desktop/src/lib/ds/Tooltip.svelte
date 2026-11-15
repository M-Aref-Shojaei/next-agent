<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    text,
    placement = "top",
    /** The trigger. Put `describedby` on the focusable element as aria-describedby. */
    children,
  }: {
    text: string;
    placement?: "top" | "bottom";
    children: Snippet<[{ describedby: string }]>;
  } = $props();

  const id = $props.id();
  let open = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function show(delay = 350) {
    clearTimeout(timer);
    timer = setTimeout(() => (open = true), delay);
  }
  function hide() {
    clearTimeout(timer);
    open = false;
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") hide(); }} />

<span
  class="ds-tooltip"
  role="presentation"
  onpointerenter={() => show()}
  onpointerleave={hide}
  onfocusin={() => show(0)}
  onfocusout={hide}
>
  {@render children({ describedby: id })}
  <span class="tip {placement}" class:open role="tooltip" {id}>{text}</span>
</span>

<style>
  .ds-tooltip { position: relative; display: inline-flex; }
  .tip {
    position: absolute;
    left: 50%;
    z-index: var(--z-tooltip);
    width: max-content;
    max-width: 260px;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-inverse);
    color: var(--color-text-inverse);
    font-size: var(--font-size-xs);
    line-height: var(--line-height-normal);
    box-shadow: var(--shadow-md);
    pointer-events: none;
    opacity: 0;
    visibility: hidden;
    transform: translate(-50%, 2px);
    transition: opacity var(--duration-fast) var(--ease-out), transform var(--duration-fast) var(--ease-out), visibility var(--duration-fast);
  }
  .top { bottom: calc(100% + var(--space-1-5)); }
  .bottom { top: calc(100% + var(--space-1-5)); }
  .open { opacity: 1; visibility: visible; transform: translate(-50%, 0); }
</style>
