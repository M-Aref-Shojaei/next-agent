<script lang="ts">
  import "../../app.css";
  import { onMount } from "svelte";
  import { app } from "$lib/app.svelte.ts";
  import { shouldCycleMode } from "$lib/shortcuts.ts";
  import { Banner, Button, Icon } from "$lib/ds/index.ts";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";

  let { children } = $props();
  /** Narrow windows: the sidebar is a drawer under the top bar. */
  let navOpen = $state(false);

  onMount(() => {
    void app.init();
  });

  function onkeydown(event: KeyboardEvent) {
    const focused = document.activeElement instanceof HTMLElement && document.activeElement.hasAttribute("data-chat-input");
    if (shouldCycleMode(event, focused)) {
      event.preventDefault();
      app.cycleMode();
    }
  }

  /** Choosing a workspace, task or page closes the drawer. */
  function onnavclick(event: MouseEvent) {
    if ((event.target as Element).closest("a, li button")) navOpen = false;
  }
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <div class="topbar">
    <Button variant="ghost" size="sm" aria-expanded={navOpen} aria-controls="app-sidebar" onclick={() => (navOpen = !navOpen)}>
      {#snippet icon()}<Icon name={navOpen ? "x" : "more"} size={16} />{/snippet}
      Menu
    </Button>
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="side" id="app-sidebar" class:open={navOpen} onclick={onnavclick}>
    <Sidebar />
  </div>
  <main>
    {#if app.error}<Banner tone="danger">{app.error}</Banner>{/if}
    {@render children()}
  </main>
  <StatusBar />
</div>

<style>
  .shell {
    --sidebar-width: calc(var(--settings-nav-width) + var(--space-8));
    display: grid;
    grid-template-columns: var(--sidebar-width) minmax(0, 1fr);
    grid-template-rows: 1fr auto;
    height: 100vh;
  }
  .side { display: flex; min-height: 0; min-width: 0; }
  .topbar { display: none; }
  main { min-width: 0; min-height: 0; display: flex; flex-direction: column; }

  /* Narrow: one column; the sidebar becomes a drawer under the top bar. */
  @media (max-width: 640px) {
    .shell { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto auto minmax(0, 1fr) auto; }
    .topbar { display: block; padding: var(--space-1) var(--space-2); border-bottom: var(--border-width) solid var(--color-border); background: var(--color-surface-sunken); }
    .side { display: none; max-height: 60vh; }
    .side.open { display: flex; }
    .side :global(.sidebar) { width: 100%; border-right: 0; border-bottom: var(--border-width) solid var(--color-border); }
  }
</style>
