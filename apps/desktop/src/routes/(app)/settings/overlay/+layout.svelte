<script lang="ts">
  // Settings → Overlay: the island's settings (vendored Coucou, ADR-0011),
  // moved here from Coucou's own window (owner request, ADR-0012). One page
  // per section; lib/overlay-settings.ts makes the calls.
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { SettingsLayout, Spinner } from "$lib/ds/index.ts";
  import { OVERLAY_ROOT, OVERLAY_SECTIONS } from "$lib/overlay-settings.ts";
  import { overlayStore } from "$lib/overlay-store.svelte.ts";

  let { children } = $props();

  const nav = OVERLAY_SECTIONS.map((s) => ({ id: s.id, label: s.label, href: `${OVERLAY_ROOT}/${s.id}` }));
  const current = $derived(page.url.pathname.slice(OVERLAY_ROOT.length + 1).split("/")[0] || "general");
  const title = $derived(OVERLAY_SECTIONS.find((s) => s.id === current)?.label ?? "Overlay");

  onMount(() => void overlayStore.load());
</script>

<svelte:head><title>{title} · Overlay · Next Agent</title></svelte:head>

<SettingsLayout {title} {nav} {current}>
  {#snippet top()}<a href="/settings">Settings</a> › Overlay{/snippet}
  {#if overlayStore.loaded}
    {@render children()}
  {:else}
    <Spinner label="Loading the overlay's settings" />
  {/if}
</SettingsLayout>
