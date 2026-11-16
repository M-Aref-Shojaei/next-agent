<script lang="ts">
  // Coucou's windows (island, settings, desktop Mochi, dictation pill): no
  // Next Agent chrome. Each page mounts the element Coucou's HTML page had and
  // loads its entry module as it is. Their styles are Coucou's, written on
  // Next Agent's design tokens (ADR-0012), which load here.
  import "$lib/ds/tokens.css";
  import { invoke } from "@tauri-apps/api/core";
  import { installGlobalHandlers, setLogSink } from "$lib/log.ts";

  // These windows log into the same diagnostics log as the main window
  // (ADR-0013): `diag_log`, the one Next Agent command their capabilities
  // grant, with the same rate limit, allowlist and redaction in Rust (target
  // `ui`, the window's label as `window`). Outside Tauri there is no sink.
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    setLogSink((level, event, fields) => invoke("diag_log", { level, event, fields }));
  }
  // Window errors and unhandled rejections in Coucou's windows, as in the main window.
  installGlobalHandlers();

  let { children } = $props();

  // These windows are fixed dark surfaces: the island and the pill sit on the
  // notch and the screen edge, which is black, and the hub is a dark panel. So
  // they use the dark tokens whatever the OS theme is. Set before the first
  // paint, so no light frame shows.
  $effect.pre(() => {
    const root = document.documentElement;
    root.dataset.theme = "dark";
    return () => delete root.dataset.theme;
  });
</script>

{@render children()}
