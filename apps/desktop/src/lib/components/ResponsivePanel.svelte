<script lang="ts">
  import { Icon, IconButton } from "$lib/ds/index.ts";

  let { title, side = "bottom", open = true, onclose, children }: {
    title: string;
    side?: "right" | "bottom";
    open?: boolean;
    onclose?: () => void;
    children: import("svelte").Snippet;
  } = $props();
</script>

{#if open}
  <section class:side-right={side === "right"} class:side-bottom={side === "bottom"} class="panel" aria-label={title}>
    <header><strong>{title}</strong>{#if onclose}<IconButton label="Close panel" size="sm" onclick={onclose}>{#snippet children()}<Icon name="x" size={14} />{/snippet}</IconButton>{/if}</header>
    <div class="content">{@render children()}</div>
  </section>
{/if}

<style>
  .panel { min-width: 0; min-height: 0; display: flex; flex-direction: column; background: var(--color-bg-surface); border-color: var(--color-border-default); }
  .side-right { width: var(--shell-inspector-width); border-left: var(--border-width) solid var(--color-border-default); }
  .side-bottom { height: var(--shell-bottom-panel-height); border-top: var(--border-width) solid var(--color-border-default); }
  header { min-height: var(--shell-titlebar-height); display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); padding: 0 var(--space-3); }
  .content { min-height: 0; overflow: auto; padding: var(--space-3); }
  @media (max-width: 1439px) {
    .side-right { width: min(var(--shell-inspector-width), 35vw); }
  }
  @media (max-width: 900px) {
    .side-right { width: 100%; max-height: var(--shell-bottom-panel-height); border-left: 0; border-top: var(--border-width) solid var(--color-border-default); }
  }
</style>
