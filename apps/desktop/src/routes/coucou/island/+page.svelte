<script lang="ts">
  // Ported from Coucou (MIT, © 2026 Louis Raillé): windows/index.html @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
  //
  // Mount point for Coucou's island: the element its page had, and its own
  // TypeScript (apps/desktop/coucou/src/main.ts), loaded as it is.
  import { onMount } from "svelte";

  onMount(() => {
    document.documentElement.lang = "en";
    document.title = "Next Agent";
    void import("$coucou/main.ts");
  });
</script>

<div id="root"></div>
