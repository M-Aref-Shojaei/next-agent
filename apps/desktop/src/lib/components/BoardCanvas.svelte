<script lang="ts">
  // Svelte wrapper around the Excalidraw React island: mounts on onMount,
  // unmounts on destroy, follows the light/dark tokens.
  import { onMount } from "svelte";
  import type { BoardProps, ExcalidrawImperativeAPI } from "$lib/boards/excalidraw.ts";
  import type { Island } from "$lib/boards/island.ts";

  interface Props {
    scene: unknown;
    onchange: BoardProps["onChange"];
    onready: (api: ExcalidrawImperativeAPI) => void;
    onerror?: (error: unknown) => void;
  }
  let { scene, onchange, onready, onerror }: Props = $props();

  let host: HTMLDivElement;

  onMount(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const theme = () => (query.matches ? ("dark" as const) : ("light" as const));
    let island: Island<BoardProps> | null = null;
    let cancelled = false;
    import("$lib/boards/excalidraw.ts")
      .then(({ mountBoard }) => {
        if (cancelled) return;
        island = mountBoard(host, { initialScene: scene, theme: theme(), onChange: onchange, onReady: onready });
        island.ready.catch((e) => onerror?.(e));
      })
      .catch((e) => onerror?.(e));
    const onTheme = () => island?.update({ theme: theme() });
    query.addEventListener("change", onTheme);
    return () => {
      cancelled = true;
      query.removeEventListener("change", onTheme);
      island?.destroy();
    };
  });
</script>

<div class="host" bind:this={host} role="application" aria-label="Design board canvas"></div>

<style>
  .host { width: 100%; height: 100%; min-height: 0; }
</style>
