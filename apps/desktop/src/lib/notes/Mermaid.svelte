<script lang="ts">
  import { Banner, currentTheme, watchTheme } from "$lib/ds/index.ts";
  import { log } from "$lib/log.ts";
  // Mermaid (MIT) is loaded on first use. `securityLevel: "strict"` makes it sanitise the
  // diagram text and the SVG; the CSP (`default-src 'self'`) keeps anything remote out.
  let { code }: { code: string } = $props();
  let svg = $state("");
  let error = $state("");
  let seq = 0;

  // Follows a forced <html data-theme>, else the OS setting; a change re-renders the diagram.
  let theme = $state(currentTheme());
  $effect(() => watchTheme((next) => (theme = next)));

  $effect(() => {
    const source = code;
    const mermaidTheme = theme === "dark" ? "dark" : "default";
    let live = true;
    const id = `mmd-${Date.now().toString(36)}-${++seq}`;
    void (async () => {
      try {
        const { default: mermaid } = await import("mermaid");
        mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: mermaidTheme });
        const out = await mermaid.render(id, source);
        if (live) {
          svg = out.svg;
          error = "";
        }
      } catch (e) {
        // Mermaid errors can quote the diagram, so only the error's type is logged.
        log.warn("ui_error", { kind: "mermaid", code: e instanceof Error ? e.name : "unknown" });
        if (live) {
          svg = "";
          error = String(e instanceof Error ? e.message : e).split("\n")[0] ?? "";
        }
        document.getElementById(`d${id}`)?.remove(); // mermaid leaves its scratch node on errors
      }
    })();
    return () => (live = false);
  });
</script>

{#if error}
  <Banner tone="danger">Mermaid: {error}</Banner>
  <pre>{code}</pre>
{:else if svg}
  <div class="mermaid">{@html svg}</div>
{:else}
  <div class="muted">Rendering diagram…</div>
{/if}

<style>
  .mermaid { overflow-x: auto; padding: var(--space-1-5) 0; }
  .mermaid :global(svg) { max-width: 100%; height: auto; }
  pre { margin: var(--space-1) 0; padding: var(--space-2); background: var(--color-surface-sunken); border-radius: var(--radius-md); overflow: auto; font-size: var(--font-size-sm); }
</style>
