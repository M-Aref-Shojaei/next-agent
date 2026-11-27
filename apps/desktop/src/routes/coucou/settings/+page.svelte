<script lang="ts">
  // Ported from Coucou (MIT, © 2026 Louis Raillé): windows/settings.html @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
  //
  // Mount point for Coucou's own window (Flow's dictation history, the
  // Notetaker, statistics, dictionary, snippets and mail). Next Agent: it loads
  // coucou/src/hub/main.ts; the settings moved into Next Agent's Settings.
  import { onMount } from "svelte";

  onMount(() => {
    document.documentElement.lang = "en";
    document.title = "Next Agent";
    document.body.className = "settings-window hub-window";
    document.body.style.background = "#000";
    document.body.style.color = "#f5f6f8";
    void import("$coucou/hub/main.ts");
  });
</script>

<div id="hub"><p style="padding: var(--space-6)">Loading…</p></div>
