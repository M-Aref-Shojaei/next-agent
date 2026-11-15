<script lang="ts">
  // Ported from Coucou (MIT, © 2026 Louis Raillé): windows/settings.html @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
  //
  // Mount point for Coucou's own window (Flow, the Notetaker and every
  // setting of the island), loading apps/desktop/coucou/src/settings/main.ts
  // as it is.
  import { onMount } from "svelte";

  onMount(() => {
    document.documentElement.lang = "en";
    document.title = "Coucou";
    document.body.className = "settings-window hub-window";
    document.body.style.background = "#000";
    document.body.style.color = "#f5f6f8";
    void import("$coucou/settings/main.ts");
  });
</script>

<div id="hub"><p style="padding: 24px">Loading…</p></div>
