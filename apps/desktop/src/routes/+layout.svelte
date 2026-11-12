<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { app } from "$lib/app.svelte.ts";
  import { shouldCycleMode } from "$lib/shortcuts.ts";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";

  let { children } = $props();

  onMount(() => void app.init());

  function onkeydown(event: KeyboardEvent) {
    const focused = document.activeElement instanceof HTMLElement && document.activeElement.hasAttribute("data-chat-input");
    if (shouldCycleMode(event, focused)) {
      event.preventDefault();
      app.cycleMode();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Sidebar />
  <main>
    {#if app.error}<div class="error" role="alert">{app.error}</div>{/if}
    {@render children()}
  </main>
  <StatusBar />
</div>

<style>
  .shell { display: grid; grid-template-columns: 230px 1fr; grid-template-rows: 1fr auto; height: 100vh; }
  main { min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .error { background: var(--danger); color: #fff; padding: 6px 12px; }
</style>
