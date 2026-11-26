<script lang="ts">
  import "../../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { afterNavigate } from "$app/navigation";
  import { app } from "$lib/app.svelte.ts";
  import { shouldCycleMode } from "$lib/shortcuts.ts";
  import { Banner, Button, Icon } from "$lib/ds/index.ts";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ActivityRail from "$lib/components/ActivityRail.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";

  let { children } = $props();
  /** Narrow windows: the sidebar is a drawer under the top bar. */
  let navOpen = $state(false);
  let commandPaletteOpen = $state(false);

  onMount(() => {
    void app.init();
  });

  afterNavigate(({ from, to }) => {
    if (from?.url?.pathname !== to?.url?.pathname) navOpen = false;
  });

  function onkeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing) return;
    if (event.key === "Escape" && navOpen && !commandPaletteOpen) {
      const menu = document.getElementById("sidebar-toggle");
      if (menu?.getClientRects().length) {
        event.preventDefault();
        navOpen = false;
        menu.focus();
        return;
      }
    }
    const opensPalette = (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k";
    // Notes reserves this shortcut for its own search palette.
    if (opensPalette && page.route.id !== "/(app)/notes") {
      event.preventDefault();
      commandPaletteOpen = true;
      return;
    }
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

{#if commandPaletteOpen}<CommandPalette onclose={() => (commandPaletteOpen = false)} />{/if}

<div class="shell" class:menu-open={navOpen}>
  <ActivityRail onactivate={() => (navOpen = false)} />
  <div class="topbar">
    <Button id="sidebar-toggle" variant="ghost" size="sm" aria-expanded={navOpen} aria-controls="app-sidebar" onclick={() => (navOpen = !navOpen)}>
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
    grid-template-columns: var(--shell-rail, 56px) var(--sidebar-width) minmax(0, 1fr);
    grid-template-rows: 1fr auto;
    height: 100vh;
  }
  .side { display: flex; min-height: 0; min-width: 0; }
  .topbar { display: none; }
  main { min-width: 0; min-height: 0; display: flex; flex-direction: column; }

  /* Narrow windows give either the sidebar or the route the full content row. */
  @media (max-width: 640px) {
    .shell { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto auto minmax(0, 1fr) auto; height: 100dvh; }
    .shell :global(.rail) { grid-column: 1; grid-row: 2; }
    .topbar { display: block; grid-row: 1; padding: var(--space-1) var(--space-2); border-bottom: var(--border-width) solid var(--color-border); background: var(--color-surface-sunken); }
    .side, main { grid-row: 3; }
    .side, .menu-open main { display: none; }
    .side.open { display: flex; }
    .shell :global(.status) { grid-row: 4; }
    .side :global(.sidebar) { width: 100%; border-right: 0; border-bottom: var(--border-width) solid var(--color-border); }
  }
</style>
