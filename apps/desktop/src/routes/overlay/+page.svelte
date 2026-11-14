<script lang="ts">
  // Always-on-top island. Shown in its own transparent window (label `overlay`).
  // Design adapted from Coucou (MIT, © 2026 Louis Raillé): windows/ island UI
  // and NotchBuddy/ApprovalQueue.swift — the look here is our own, built on the
  // app's CSS tokens; no Coucou assets are used.
  import { onMount, tick } from "svelte";
  import { api, type OverlayInfo } from "$lib/api.ts";
  import { MODE_LABELS, MODE_TONE } from "$lib/modes.ts";
  import {
    activeTask,
    emptyModel,
    ingest,
    ingestStarted,
    markAnswered,
    pendingQueue,
  } from "$lib/overlay-model.ts";
  import type { ApprovalChoice } from "$lib/timeline.ts";
  import type { ModeName } from "@next-agent/contracts";

  const COLLAPSE_DELAY_MS = 450;

  let model = $state(emptyModel());
  let health = $state<"ok" | "degraded" | "down" | "unknown">("unknown");
  let info = $state<OverlayInfo | null>(null);
  let pointerInside = $state(false);
  let lingering = $state(false);
  let island = $state<HTMLElement | null>(null);
  let error = $state("");
  let linger: ReturnType<typeof setTimeout> | undefined;

  const queue = $derived(pendingQueue(model));
  const task = $derived(activeTask(model));
  const mode = $derived((task ? model.modes[task.id] : undefined) as ModeName | undefined);
  // Interim (until the Coucou port lands): hovering no longer opens the full
  // panel, which covered browser tabs. Only pending approvals expand it.
  const expanded = $derived(queue.total > 0);
  const notchWidth = $derived(info?.notch ? info.notch[0] : 0);

  const STATUS: Record<string, string> = {
    pending: "starting",
    running: "running",
    completed: "done",
    failed: "failed",
    cancelled: "cancelled",
  };

  // Keep the window size in step with the panel state.
  let lastExpanded: boolean | null = null;
  $effect(() => {
    const want = expanded;
    if (want === lastExpanded) return;
    lastExpanded = want;
    void api.overlaySetExpanded(want).then(reportRect, () => {});
  });

  function reportRect() {
    if (!island) return;
    const r = island.getBoundingClientRect();
    void api.overlaySetRect({ x: r.x, y: r.y, w: r.width, h: r.height }).catch(() => {});
  }

  function onPointer(inside: boolean) {
    clearTimeout(linger);
    if (inside) {
      pointerInside = true;
      lingering = false;
    } else {
      pointerInside = false;
      lingering = true;
      linger = setTimeout(() => (lingering = false), COLLAPSE_DELAY_MS);
    }
  }

  async function respond(taskId: string, approvalId: string, choice: ApprovalChoice) {
    try {
      await api.approvalRespond(taskId, approvalId, choice);
      model = markAnswered(model, taskId, approvalId, choice);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    const unlisten: (() => void)[] = [];
    const watch = new ResizeObserver(reportRect);
    void (async () => {
      info = await api.overlayInfo().catch(() => null);
      const health0 = await api.routerHealth().catch(() => null);
      if (health0) health = health0.status;
      unlisten.push(
        await api.onTaskStream((m) => (model = ingest(model, m))),
        await api.onTaskStarted((s) => (model = ingestStarted(model, s))),
        await api.onOverlayPointer(onPointer),
        await api.onRouterHealth((r) => (health = r.status)),
      );
      await tick();
      if (island) watch.observe(island);
      reportRect();
    })();
    return () => {
      clearTimeout(linger);
      watch.disconnect();
      for (const fn of unlisten) fn();
    };
  });
</script>

<svelte:head>
  <style>
    html, body { background: transparent !important; overflow: hidden; }
  </style>
</svelte:head>

<div class="frame" class:notched={notchWidth > 0}>
<div
  class="island"
  class:expanded
  class:notched={notchWidth > 0}
  class:opaque={info ? !info.transparent : false}
  bind:this={island}
  role="region"
  aria-label="Next Agent overlay"
>
  <header>
    <span class="dot {health}" title="Router {health}" aria-label="Router {health}"></span>
    {#if mode}
      <span class="badge tone-{MODE_TONE[mode] ?? 'neutral'}">{(MODE_LABELS[mode] ?? mode).toUpperCase()}</span>
    {/if}
    {#if notchWidth > 0}<span class="notch-gap" style="width:{notchWidth}px" aria-hidden="true"></span>{/if}
    {#if task}
      <button class="title" onclick={() => api.overlayOpenTask(task.id)} title="Open in Next Agent">
        {task.title}
      </button>
      <span class="status {task.status}">{STATUS[task.status] ?? task.status}</span>
    {:else}
      <span class="idle">Idle</span>
    {/if}
    {#if queue.total > 0}<span class="count" aria-label="{queue.total} pending approvals">{queue.total}</span>{/if}
  </header>

  {#if expanded}
    <div class="body">
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      {#if queue.total === 0}
        <p class="muted">No approvals waiting.</p>
      {/if}
      <ul class="queue" aria-label="Pending approvals">
        {#each queue.visible as item (item.approvalId)}
          <li class="card">
            <div class="what">
              <code>{item.tool}</code>
              <span class="summary">{item.summary}</span>
              {#if item.taskTitle}<span class="muted of">{item.taskTitle}</span>{/if}
            </div>
            <div class="row">
              <button class="primary" onclick={() => respond(item.taskId, item.approvalId, "allow")}>Allow</button>
              <button class="danger" onclick={() => respond(item.taskId, item.approvalId, "deny")}>Deny</button>
              <button onclick={() => respond(item.taskId, item.approvalId, "always")}>Always</button>
            </div>
          </li>
        {/each}
      </ul>
      {#if queue.hidden > 0}<p class="muted">+{queue.hidden} more waiting</p>{/if}
    </div>
  {/if}
</div>
</div>

<style>
  .island {
    width: 100%; height: 100%; overflow: hidden; display: flex; flex-direction: column;
    background: color-mix(in srgb, var(--panel) 94%, transparent); color: var(--text);
    border: 1px solid var(--border); border-top: 0; border-radius: 0 0 16px 16px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
    transform-origin: top center;
    transition: border-radius 0.34s cubic-bezier(0.45, 0, 0.2, 1);
    font-size: 13px;
  }
  /* Fallback if the webview cannot be transparent. */
  .island.opaque { background: #0b0c0f; color: #e6e8ec; border-radius: 0; border: 0; box-shadow: none; }

  /* Notch screens: a pure-black island that merges with the notch, flush with
     the top edge, rounded below, with concave "shoulders" curving into the
     menu bar on both sides (design adapted from Coucou, MIT; no assets). */
  .frame { --shoulder: 10px; position: relative; width: 100%; height: 100%; }
  .frame.notched { padding: 0 var(--shoulder); box-sizing: border-box; }
  .frame.notched::before, .frame.notched::after {
    content: ""; position: absolute; top: 0; width: var(--shoulder); height: var(--shoulder);
  }
  .frame.notched::before { left: 0; background: radial-gradient(circle at 0 100%, transparent calc(var(--shoulder) - 0.5px), #000 var(--shoulder)); }
  .frame.notched::after { right: 0; background: radial-gradient(circle at 100% 100%, transparent calc(var(--shoulder) - 0.5px), #000 var(--shoulder)); }
  .island.notched {
    background: #000; color: #e6e8ec; border: 0; box-shadow: none;
    border-radius: 0 0 12px 12px;
  }
  .island.notched.expanded { border-radius: 0 0 22px 22px; }
  .island.expanded { height: auto; max-height: 100vh; animation: spring-in 0.5s cubic-bezier(0.34, 1.56, 0.64, 1); }
  .island:not(.expanded) { animation: ease-out 0.34s cubic-bezier(0.45, 0, 0.2, 1); }
  @keyframes spring-in { from { transform: scaleY(0.6) scaleX(0.9); opacity: 0.6; } to { transform: none; opacity: 1; } }
  @keyframes ease-out { from { transform: scaleY(1.25); opacity: 0.8; } to { transform: none; opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .island, .island.expanded { animation: none; transition: none; } }

  header { display: flex; align-items: center; gap: 8px; padding: 4px 12px; min-height: 28px; }
  .notch-gap { flex: none; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--muted); flex: none; }
  .dot.ok { background: var(--ok); } .dot.degraded { background: var(--warn); } .dot.down { background: var(--danger); }
  .badge { color: var(--tone); border: 1px solid var(--tone); border-radius: 4px; padding: 0 6px; font-size: 11px; font-weight: 700; letter-spacing: 0.06em; flex: none; }
  .title { border: 0; background: none; padding: 0; min-width: 0; flex: 1; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: inherit; }
  .title:hover { text-decoration: underline; }
  .idle { color: var(--muted); flex: 1; }
  .status { font-size: 11px; color: var(--muted); flex: none; }
  .status.running { color: var(--info); } .status.completed { color: var(--ok); } .status.failed { color: var(--danger); }
  .count { background: var(--warn); color: #000; border-radius: 999px; padding: 0 7px; font-weight: 700; font-size: 11px; flex: none; }

  .body { padding: 4px 12px 12px; overflow-y: auto; display: grid; gap: 8px; }
  .queue { list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }
  .card { border: 1px solid var(--warn); border-left-width: 4px; border-radius: 8px; padding: 8px 10px; display: flex; gap: 10px; align-items: center; justify-content: space-between; }
  .what { display: grid; gap: 2px; min-width: 0; }
  .summary { font-family: ui-monospace, monospace; word-break: break-all; }
  .of { font-size: 11px; }
  .row { display: flex; gap: 6px; flex: none; }
  p { margin: 0; }
  .err { color: var(--danger); }
</style>
