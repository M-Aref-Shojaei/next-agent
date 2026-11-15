<script lang="ts">
  // Ported from Coucou (MIT, © 2026 Louis Raillé): windows/pill.html @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
  //
  // Mount point for Flow dictation's pill, loading
  // apps/desktop/coucou/src/pill/main.ts as it is.
  import { onMount } from "svelte";

  onMount(() => {
    document.documentElement.lang = "en";
    document.title = "Coucou";
    void import("$coucou/pill/main.ts");
  });
</script>

<div id="pill"></div>
