<script lang="ts">
  // A device frame (web, mobile, desktop) around a sandboxed iframe, scaled
  // to fit its container. Directions pass `srcdoc` (sandbox ""), the live
  // app passes `src` (the loopback proxy, APP_SANDBOX).
  import { FRAMES, fitScale, type FrameKind } from "$lib/design.ts";

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
    <div class="screen" style="width: {size.width * scale}px; height: {size.height * scale}px">
      <iframe
        bind:this={iframe}
        {title}
        {srcdoc}
        {src}
        sandbox={sandbox}
        referrerpolicy="no-referrer"
        style="width: {size.width}px; height: {size.height}px; transform: scale({scale})"
        {onload}
      ></iframe>
    </div>
  </div>
</div>

<style>
  .wrap { width: 100%; display: flex; justify-content: center; min-width: 0; }
  .device { border: 1px solid var(--border); border-radius: 8px; background: var(--panel); overflow: hidden; box-shadow: 0 1px 2px rgb(0 0 0 / 0.06); }
  .device.mobile { border-radius: 28px; padding: 0 12px 12px; background: var(--sidebar); }
  .device.thumb { border-radius: 6px; box-shadow: none; }
  .device.thumb.mobile { border-radius: 14px; padding: 6px; }
  .bar { height: 26px; display: flex; align-items: center; gap: 8px; padding: 0 10px; border-bottom: 1px solid var(--border); background: var(--sidebar); font-size: 11px; color: var(--muted); }
  .mobile .bar { border: 0; justify-content: center; background: transparent; }
  .notch { width: 72px; height: 6px; border-radius: 3px; background: var(--border); }
  .dots { display: inline-flex; gap: 5px; }
  .dots i { width: 9px; height: 9px; border-radius: 50%; background: var(--border); display: block; }
  .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .url { flex: 1; background: var(--panel); border: 1px solid var(--border); border-radius: 999px; padding: 1px 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .screen { overflow: hidden; position: relative; background: #fff; }
  .mobile .screen { border-radius: 18px; }
  .thumb.mobile .screen { border-radius: 10px; }
  iframe { border: 0; transform-origin: 0 0; display: block; background: #fff; }
  .thumb iframe { pointer-events: none; }
</style>
