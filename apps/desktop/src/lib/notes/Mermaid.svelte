<script lang="ts">
  // Mermaid (MIT) is loaded on first use. `securityLevel: "strict"` makes it sanitise the
  // diagram text and the SVG; the CSP (`default-src 'self'`) keeps anything remote out.
  let { code }: { code: string } = $props();
  let svg = $state("");
  let error = $state("");
  let seq = 0;

  const dark = () => typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches;

  $effect(() => {
    const source = code;
    let live = true;
    const id = `mmd-${Date.now().toString(36)}-${++seq}`;
    void (async () => {
      try {
        const { default: mermaid } = await import("mermaid");
        mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark() ? "dark" : "default" });
        const out = await mermaid.render(id, source);
        if (live) {
          svg = out.svg;
          error = "";
        }
      } catch (e) {
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
  <div class="err" role="alert">Mermaid: {error}</div>
  <pre>{code}</pre>
{:else if svg}
  <div class="mermaid">{@html svg}</div>
{:else}
  <div class="muted">Rendering diagram…</div>
{/if}

<style>
  .mermaid { overflow-x: auto; padding: 6px 0; }
  .mermaid :global(svg) { max-width: 100%; height: auto; }
  .err { color: var(--danger); font-size: 12px; }
  pre { margin: 4px 0; padding: 8px; background: var(--sidebar); border-radius: 6px; overflow: auto; font-size: 12px; }
</style>
