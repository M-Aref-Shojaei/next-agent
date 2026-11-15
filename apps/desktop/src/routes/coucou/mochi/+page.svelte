<script lang="ts">
  // Ported from Coucou (MIT, © 2026 Louis Raillé): windows/mochi.html @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
  //
  // Mount point for Coucou's desktop window, loading
  // apps/desktop/coucou/src/desktop/main.ts as it is. Next Agent draws its
  // gradient orb there instead of Coucou's character (mochi/engine.ts).
  import { onMount } from "svelte";

  onMount(() => {
    document.title = "Next Agent";
    void import("$coucou/desktop/main.ts");
  });
</script>

<canvas id="mochi"></canvas>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
    user-select: none;
    -webkit-user-select: none;
    cursor: default;
  }
  #mochi {
    position: absolute;
    left: 0;
    top: 0;
    width: 120px;
    height: 120px;
    transition: opacity 0.45s ease-in-out;
  }
</style>
