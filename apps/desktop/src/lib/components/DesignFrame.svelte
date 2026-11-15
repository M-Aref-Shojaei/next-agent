<script lang="ts">
  // A device frame (web, mobile, desktop) around a sandboxed iframe, scaled
  // to fit its container. Directions pass `srcdoc` (sandbox ""), the live
  // app passes `src` (the loopback proxy, APP_SANDBOX).
  import { FRAMES, fitScale, type FrameKind } from "$lib/design.ts";
  import { PREVIEW_CANVAS } from "$lib/design-preview-data.ts";

  let {
    frame,
    title,
    sandbox,
    srcdoc = undefined,
    src = undefined,
    thumb = false,
    iframe = $bindable(null),
    onload = undefined,
  }: {
    frame: FrameKind;
    title: string;
    sandbox: string;
    srcdoc?: string;
    src?: string;
    /** A small, non-interactive thumbnail (skipped by keyboard and screen readers). */
    thumb?: boolean;
    iframe?: HTMLIFrameElement | null;
    onload?: () => void;
  } = $props();

  let width = $state(0);
  const size = $derived(FRAMES[frame]);
  const chrome = $derived(frame === "mobile" ? 24 : 0);
  const scale = $derived(fitScale(frame, width - chrome));
</script>

<div class="wrap" bind:clientWidth={width} inert={thumb}>
  <div class="device {frame}" class:thumb style="width: {size.width * scale + chrome}px">
    {#if !thumb}
      <div class="bar" aria-hidden="true">
        {#if frame === "desktop"}
          <span class="dots"><i></i><i></i><i></i></span><span class="label">{title}</span>
        {:else if frame === "web"}
          <span class="url">{src ? src.replace(/^https?:\/\//, "") : "preview — no network"}</span>
        {:else}
          <span class="notch"></span>
        {/if}
      </div>
    {/if}
    <div class="screen" style="width: {size.width * scale}px; height: {size.height * scale}px" style:background={PREVIEW_CANVAS}>
      <iframe
        bind:this={iframe}
        {title}
        {srcdoc}
        {src}
        sandbox={sandbox}
        referrerpolicy="no-referrer"
        style="width: {size.width}px; height: {size.height}px; transform: scale({scale})"
        style:background={PREVIEW_CANVAS}
        {onload}
      ></iframe>
    </div>
  </div>
</div>

<style>
  .wrap { width: 100%; display: flex; justify-content: center; min-width: 0; }
  .device { border: var(--border-width) solid var(--color-border); border-radius: var(--radius-lg); background: var(--color-surface); overflow: hidden; box-shadow: var(--shadow-sm); }
  .device.mobile { border-radius: var(--radius-xl); padding: 0 var(--space-3) var(--space-3); background: var(--color-surface-sunken); }
  .device.thumb { border-radius: var(--radius-md); box-shadow: none; }
  .device.thumb.mobile { border-radius: var(--radius-lg); padding: var(--space-1-5); }
  .bar { height: 26px; display: flex; align-items: center; gap: var(--space-2); padding: 0 var(--space-3); border-bottom: var(--border-width) solid var(--color-border); background: var(--color-surface-sunken); font-size: var(--font-size-xs); color: var(--color-text-secondary); }
  .mobile .bar { border: 0; justify-content: center; background: transparent; }
  .notch { width: 72px; height: var(--space-1-5); border-radius: var(--radius-xs); background: var(--color-border); }
  .dots { display: inline-flex; gap: var(--space-1-5); }
  .dots i { width: 9px; height: 9px; border-radius: var(--radius-full); background: var(--color-border-strong); display: block; }
  .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .url { flex: 1; background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-full); padding: var(--space-0-5) var(--space-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .screen { overflow: hidden; position: relative; }
  .mobile .screen { border-radius: var(--radius-xl); }
  .thumb.mobile .screen { border-radius: var(--radius-lg); }
  iframe { border: 0; transform-origin: 0 0; display: block; }
  .thumb iframe { pointer-events: none; }
</style>
