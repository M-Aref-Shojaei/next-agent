<script lang="ts">
  // Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandRootView.swift (IslandContainer, IslandHeader, BotPlacement, CompactMiniGrid), NotchBuddy/Sources/App/IslandViewContent.swift (ApprovalView, EmptyStateView), NotchBuddy/Sources/App/IslandWindowController.swift (wireFSM, setMode, expand, collapse, pollFrame hover feed) and windows/src/island/island.ts, windows/src/views/views.ts, windows/src/style.css @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
  //
  // The island, in its own transparent window (label `overlay`). Differences
  // from Coucou: the data comes from our `task-stream` events (not Claude
  // hooks), a decision goes through `approval_respond` and our permission
  // engine, the character is a neutral placeholder (IslandMascot), glyphs are
  // Lucide, and there are no sounds.
  import { onMount, tick } from "svelte";
  import { api, inTauri } from "$lib/api.ts";
  import Glyph from "$lib/components/Glyph.svelte";
  import IslandMascot from "$lib/components/IslandMascot.svelte";
  import { Tracked } from "$lib/island/anim.ts";
  import { IslandStateMachine } from "$lib/island/fsm.ts";
  import { GLYPHS } from "$lib/island/icons.ts";
  import {
    FALLBACK_INFO,
    botPosition,
    islandCorner,
    islandFillOpacity,
    islandRect,
    islandSize,
    islandTopRadius,
    miniGrid,
    type IslandInfo,
    type IslandMode,
    type IslandView,
  } from "$lib/island/layout.ts";
  import { islandPath } from "$lib/island/shape.ts";
  import {
    activeTask,
    emptyModel,
    ingest,
    ingestStarted,
    markAnswered,
    nextTimeout,
    pendingQueue,
  } from "$lib/overlay-model.ts";
  import type { ApprovalChoice } from "$lib/timeline.ts";

  /** How long a fresh permission card ignores clicks on its buttons (views.ts CLICK_GUARD_MS). */
  const CLICK_GUARD_MS = 600;
  /** PC: once folded, the window shrinks to the wake strip (island.ts updateWindowCollapsed). */
  const COLLAPSE_WINDOW_MS = 420;
  const STRIP: [number, number] = [240, 6];

  let model = $state(emptyModel());
  let now = $state(Date.now());
  let info = $state<IslandInfo>(FALLBACK_INFO);
  let mode = $state<IslandMode>("hidden");
  let view = $state<IslandView>("empty");
  let error = $state("");

  // Animated geometry (IslandContainer @State + IslandShape.animatableData).
  const width = new Tracked(FALLBACK_INFO.width);
  const height = new Tracked(FALLBACK_INFO.height);
  const corner = new Tracked(14);
  const botX = new Tracked(46);
  const botY = new Tracked(16);
  const botD = new Tracked(6);
  let geo = $state({ w: FALLBACK_INFO.width, h: FALLBACK_INFO.height, r: 14, bx: 46, by: 16, bd: 6 });

  const queue = $derived(pendingQueue(model, undefined, now));
  const head = $derived(queue.visible[0]);
  const live = $derived(model.tasks.filter((t) => t.status === "running" || t.status === "pending"));
  const task = $derived(activeTask(model));
  const fill = $derived(islandFillOpacity(info, mode));
  const top = $derived(islandTopRadius(info, mode));
  const rect = $derived(islandRect(info, geo.w, geo.h));
  const path = $derived(islandPath(geo.w, geo.h, geo.r, top));
  const grid = $derived(miniGrid(geo.w, geo.h));
  const corner_ = $derived(info.position !== "top-center");
  const tone = $derived(
    head ? "approval" : task?.status === "failed" ? "error" : task?.status === "completed" ? "finished" : live.length ? "working" : "idle",
  );

  const fsm = new IslandStateMachine();
  let wasInIsland = false;
  let shownFor: string | null = null;
  let shownAt = 0;
  let collapsed = false;
  let collapseTimer: ReturnType<typeof setTimeout> | undefined;
  let frame = 0;
  let last = 0;

  // ── Mode transitions (IslandWindowController.setMode / expand / collapse) ──

  const order = (m: IslandMode) => (m === "hidden" ? 0 : m === "compact" ? 1 : 2);

  function defaultView(): IslandView {
    if (head) return "approval";
    return live.length > 0 ? "overview" : "empty";
  }

  function animateTo(shrinking: boolean) {
    const { w, h } = islandSize(info, mode, view);
    const r = islandCorner(info, mode);
    const bot = botPosition(mode, view, w, h);
    const t = performance.now();
    for (const [tracked, target] of [
      [width, w],
      [height, h],
      [corner, r],
      [botX, bot.cx],
      [botY, bot.cy],
      [botD, bot.diameter],
    ] as [Tracked, number][]) {
      if (shrinking) tracked.curveTowards(target, t);
      else tracked.springTo(target);
    }
    run();
  }

  function setMode(next: IslandMode) {
    const prev = mode;
    if (next === prev) return;
    const shrinking = order(next) < order(prev);
    if (next !== "hidden") unfoldWindow();
    mode = next;
    animateTo(shrinking);
    void api.overlaySetExpanded(next === "expanded").catch(() => {});
    scheduleWindowFold();
  }

  function expand(to: IslandView) {
    view = to;
    if (mode === "expanded") animateTo(false);
    else setMode("expanded");
  }

  function collapse(allowPendingApproval = false) {
    const keepsApprovalPending = allowPendingApproval && head !== undefined;
    if (fsm.isHeldOpen?.() === true && !keepsApprovalPending) return;
    fsm.collapse();
    setMode("compact");
  }

  fsm.isHeldOpen = () => head !== undefined;
  fsm.onTransition = (_from, to) => {
    switch (to) {
      case "hidden":
        setMode(live.length > 0 ? "compact" : "hidden");
        break;
      case "petit":
        setMode("compact");
        if (!wasInIsland) fsm.mouseLeft();
        break;
      case "home":
        expand(defaultView());
        if (!wasInIsland) fsm.mouseLeft();
        break;
    }
  };

  // ── PC window fold (windows/src/island/island.ts updateWindowCollapsed) ──

  function scheduleWindowFold() {
    clearTimeout(collapseTimer);
    if (info.platform === "macos" || mode !== "hidden") return;
    collapseTimer = setTimeout(() => {
      if (mode !== "hidden") return;
      collapsed = true;
      void api.overlaySetCollapsed(STRIP).catch(() => {});
      report();
    }, COLLAPSE_WINDOW_MS);
  }

  function unfoldWindow() {
    clearTimeout(collapseTimer);
    if (!collapsed) return;
    collapsed = false;
    void api.overlaySetCollapsed(null).catch(() => {});
  }

  // ── Frame loop (island.ts applyGeometry) ─────────────────────────────────

  let pushed = { x: -1, y: -1, w: -1, h: -1 };
  function report() {
    const r = collapsed ? { x: 0, y: 0, w: STRIP[0], h: STRIP[1] } : rect;
    if (Math.abs(r.x - pushed.x) < 0.5 && Math.abs(r.w - pushed.w) < 0.5 && Math.abs(r.h - pushed.h) < 0.5) return;
    pushed = r;
    void api.overlaySetRect(r).catch(() => {});
  }

  function step(t: number) {
    frame = 0;
    const dt = last ? Math.min(0.05, (t - last) / 1000) : 1 / 60;
    last = t;
    const all = [width, height, corner, botX, botY, botD];
    for (const tracked of all) tracked.step(dt, t);
    geo = { w: width.value, h: height.value, r: corner.value, bx: botX.value, by: botY.value, bd: botD.value };
    report();
    if (all.some((x) => x.animating)) frame = requestAnimationFrame(step);
    else last = 0;
  }

  function run() {
    if (!frame) frame = requestAnimationFrame(step);
  }

  /** New screen, new resting size (notch ↔ bar): snap without animation. */
  function snap() {
    const { w, h } = islandSize(info, mode, view);
    const bot = botPosition(mode, view, w, h);
    width.jump(w);
    height.jump(h);
    corner.jump(islandCorner(info, mode));
    botX.jump(bot.cx);
    botY.jump(bot.cy);
    botD.jump(bot.diameter);
    geo = { w, h, r: corner.value, bx: bot.cx, by: bot.cy, bd: bot.diameter };
    pushed = { x: -1, y: -1, w: -1, h: -1 };
    report();
  }

  // ── Inputs ────────────────────────────────────────────────────────────────

  /** pollFrame's hover feed: the hit test is updated before the FSM hears of it. */
  function onPointer(inside: boolean) {
    const previously = wasInIsland;
    wasInIsland = inside;
    if (inside && !previously) fsm.mouseEntered();
    if (!inside && previously) fsm.mouseLeft();
  }

  function onIslandDown() {
    fsm.userInteracted();
    if (mode !== "expanded") fsm.click();
  }

  async function decide(choice: ApprovalChoice) {
    if (!head || performance.now() - shownAt < CLICK_GUARD_MS) return;
    const item = head;
    try {
      await api.approvalRespond(item.taskId, item.approvalId, choice);
      model = markAnswered(model, item.taskId, item.approvalId, choice);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  // AppState.syncMode: no live task folds the compact island away; a task brings it back.
  $effect(() => {
    const count = live.length;
    if (count === 0 && mode === "compact" && !wasInIsland) {
      fsm.hiddenExternally();
      setMode("hidden");
    } else if (count > 0 && mode === "hidden") {
      setMode("compact");
    }
  });

  // hookExpand for an approval; the card leaves once it is answered.
  let lastHead: string | null = null;
  $effect(() => {
    const id = head?.approvalId ?? null;
    if (id === lastHead) return;
    const had = lastHead;
    lastHead = id;
    if (id) {
      shownFor = id;
      shownAt = performance.now();
      fsm.openedExternally();
      expand("approval");
    } else if (had && mode === "expanded") {
      view = defaultView();
      collapse();
    }
  });

  // Requests time out of the island (HookServer's 115 s wait).
  $effect(() => {
    const due = nextTimeout(model, now);
    if (due === null) return;
    const timer = setTimeout(() => (now = Date.now()), Math.max(0, due - Date.now()) + 50);
    return () => clearTimeout(timer);
  });

  // syncView: home without sessions is the empty view, and back.
  $effect(() => {
    if (mode !== "expanded" || view === "approval") return;
    const want = live.length > 0 ? "overview" : "empty";
    if (view !== want) view = want;
  });

  /**
   * Outside Tauri (a plain browser, `vite dev`) drive the pointer from the
   * page's own mouse events so the island can be inspected (island.ts
   * `followPageCursor`); same hit margin as the Mac poll.
   */
  function followPageCursor(): () => void {
    const move = (e: MouseEvent) => {
      const r = rect;
      const m = 6;
      onPointer(e.clientX >= r.x - m && e.clientX <= r.x + r.w + m && e.clientY >= r.y - m && e.clientY <= r.y + r.h + m);
    };
    const out = (e: MouseEvent) => {
      if (e.relatedTarget == null) onPointer(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseout", out);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseout", out);
    };
  }

  onMount(() => {
    const unlisten: (() => void)[] = [];
    if (!inTauri()) unlisten.push(followPageCursor());
    void (async () => {
      const got = await api.overlayInfo().catch(() => null);
      if (got) info = got.island;
      snap();
      unlisten.push(
        await api.onTaskStream((m) => {
          now = Date.now();
          model = ingest(model, m, now);
          // Non-alert work event: compact reveal (hookReveal).
          fsm.reveal();
        }),
        await api.onTaskStarted((s) => {
          now = Date.now();
          model = ingestStarted(model, s, now);
          fsm.reveal();
        }),
        await api.onOverlayPointer(onPointer),
        await api.onOverlayGeometry((island) => {
          info = island;
          snap();
        }),
      );
      await tick();
      report();
      scheduleWindowFold();
    })();
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(collapseTimer);
      for (const fn of unlisten) fn();
    };
  });
</script>

<svelte:head>
  <style>
    html, body { background: transparent !important; overflow: hidden; margin: 0; }
  </style>
</svelte:head>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="island"
  class:corner={corner_}
  style="left:{rect.x}px;width:{geo.w}px;height:{geo.h}px"
  onmousedown={onIslandDown}
  role="region"
  aria-label="Next Agent island"
>
  <svg class="shape" width={geo.w} height={geo.h} aria-hidden="true">
    <path d={path} fill="#000" fill-opacity={fill} stroke="#fff" stroke-opacity={corner_ ? 0.09 : 0} stroke-width="1" />
  </svg>

  {#if mode === "expanded"}
    <div class="content" style="clip-path: path('{path}')">
      <div id="header">
        <div class="tabs">
          <button class="tab" class:on={view !== "approval"} title="Overview" onclick={() => (view = defaultView())}>
            <Glyph glyph={GLYPHS.house} size={15} />
          </button>
        </div>
        <div class="header-actions">
          {#if !head}
            <button title="Close" aria-label="Close" onclick={() => collapse()}><Glyph glyph={GLYPHS.xmark} size={15} /></button>
          {/if}
        </div>
      </div>
      <div id="views">
        {#if view === "approval" && head}
          <div class="view on">
            <div class="card wash" style="--wash: rgba(245,165,36,0.42)">
              <div class="stack" style="padding:4px 16px 4px 116px">
                <div class="who-row">
                  <span class="dot" style="width:8px;height:8px;background:#f5a524"></span>
                  <span class="n">{head.taskTitle}</span>
                  <span>needs permission</span>
                  {#if queue.visible.length > 1}<span class="count">1/{queue.visible.length}</span>{/if}
                </div>
                <div class="code" title={head.summary || head.tool}>{head.summary || head.tool}</div>
                <div class="actions">
                  <button class="btn secondary" onclick={() => decide("deny")}>Deny</button>
                  <button class="btn primary" onclick={() => decide("allow")}>Allow</button>
                  <button class="btn secondary" onclick={() => decide("always")}>Always</button>
                </div>
                {#if error}<p class="err" role="alert">{error}</p>{/if}
              </div>
              <button class="icon-btn fold" title="Later — keep it waiting" aria-label="Later" onclick={() => collapse(true)}>
                <Glyph glyph={GLYPHS.chevronUp} size={8} stroke={2.4} />
              </button>
            </div>
          </div>
        {:else if view === "overview" && task}
          <div class="view on">
            <div class="card">
              <div class="stack" style="padding:4px 16px 4px 116px">
                <div class="who-row">
                  <span class="dot" style="width:8px;height:8px;background:#3b9eff"></span>
                  <span class="n">{task.title}</span>
                  <span>{live.length > 1 ? `and ${live.length - 1} more` : "is working"}</span>
                </div>
                <div class="actions">
                  <button class="btn primary" onclick={() => api.overlayOpenTask(task.id)}>Open in Next Agent</button>
                </div>
              </div>
            </div>
          </div>
        {:else}
          <div class="view on">
            <div class="card">
              <div class="stack" style="padding:0 18px 0 118px;flex-direction:row;align-items:center;gap:16px">
                <div style="display:flex;flex-direction:column;gap:5px">
                  <div class="title">Nothing running right now.</div>
                  <div class="sub">Start a task in Next Agent.</div>
                </div>
              </div>
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <IslandMascot cx={geo.bx} cy={geo.by} diameter={geo.bd} opacity={1} tone={mode === "hidden" && tone === "idle" ? "rest" : tone} />

  {#if mode === "compact" && !corner_}
    <div
      id="mini-grid"
      style="left:{grid.cx - 14.5}px;top:{grid.cy - 14.5}px;transform:scale({grid.scale})"
      aria-label="{live.length} running, {queue.visible.length} waiting"
    >
      {#each live.slice(0, live.length > 4 ? 3 : 4) as t (t.id)}
        <span class="mini" class:waiting={t.approvals.some((a) => a.choice === undefined)}></span>
      {/each}
      {#if live.length > 4}<span class="more">+{live.length - 3}</span>{/if}
    </div>
  {/if}
</div>

<style>
  /* Palette and type from windows/src/style.css (itself copied from the Swift views). */
  :global(:root) {
    --font: system-ui, -apple-system, "Segoe UI Variable Text", "Segoe UI", "Noto Sans", sans-serif;
    --mono: ui-monospace, "SF Mono", "Cascadia Mono", Consolas, monospace;
    --ink: #f5f6f8;
    --ink-2: #f1f2f4;
    --dim: #9398a1;
    --dim-2: #8e939c;
    --dim-3: #6b7079;
    --dim-4: #5f646d;
    --card: #141518;
    --tab-on: #1d1f23;
    --hairline: rgba(255, 255, 255, 0.035);
  }
  .island {
    position: absolute;
    top: 0;
    font-family: var(--font);
    color: var(--ink);
    -webkit-font-smoothing: antialiased;
    user-select: none;
    cursor: default;
  }
  .island.corner .shape { filter: drop-shadow(0 6px 14px rgba(0, 0, 0, 0.45)); }
  .shape { position: absolute; inset: 0; overflow: visible; }
  .content {
    position: absolute;
    inset: 0;
    padding: 8px 10px 10px;
    display: flex;
    flex-direction: column;
  }

  #header { height: 34px; flex: 0 0 34px; display: flex; align-items: center; margin: 0 -10px; }
  .tabs { display: flex; gap: 5px; padding-left: 14px; }
  .tab {
    width: 30px; height: 22px; display: grid; place-items: center; border: 0;
    background: transparent; border-radius: 11px; color: var(--dim-2); cursor: default; padding: 0;
  }
  .tab:hover { background: rgba(255, 255, 255, 0.07); color: #b0b5be; }
  .tab.on { background: var(--tab-on); color: var(--ink); }
  .header-actions { margin-left: auto; display: flex; gap: 14px; padding-right: 16px; }
  .header-actions button { border: 0; background: transparent; color: var(--dim-2); display: grid; place-items: center; cursor: default; padding: 0; }
  .header-actions button:hover { color: var(--ink); }

  #views { position: relative; flex: 1 1 auto; min-height: 0; }
  .view { position: absolute; inset: 0; opacity: 0; transform: scale(0.97); }
  .view.on {
    opacity: 1;
    transform: scale(1);
    animation: view-in 0.3s cubic-bezier(0.3, 1.2, 0.4, 1) 0.16s both;
  }
  @keyframes view-in { from { opacity: 0; transform: scale(0.97); } to { opacity: 1; transform: scale(1); } }

  .card {
    position: relative; height: 100%; border-radius: 20px; background: var(--card);
    border: 1px solid var(--hairline); overflow: hidden; box-sizing: border-box;
  }
  .card.wash::after {
    content: ""; position: absolute; inset: 0; pointer-events: none;
    background: radial-gradient(280px 280px at 50% 130%, var(--wash) 0%, rgba(0, 0, 0, 0) 70%);
  }
  .stack {
    position: relative; z-index: 1; height: 100%; display: flex; flex-direction: column;
    justify-content: center; gap: 5px; box-sizing: border-box;
  }
  .who-row { display: flex; align-items: center; gap: 7px; font: 400 12px var(--font); color: var(--dim-2); min-width: 0; }
  .who-row .n { font: 600 12px var(--font); color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .who-row .count { font-weight: 600; color: #8e939c; font-variant-numeric: tabular-nums; }
  .dot { display: inline-block; border-radius: 50%; flex: 0 0 auto; }
  .code {
    font: 400 12px var(--mono); color: #e8e9ec; background: rgba(255, 255, 255, 0.07);
    border: 1px solid rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 5px 10px;
    max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; user-select: text; box-sizing: border-box;
  }
  .actions { display: flex; gap: 8px; }
  .btn {
    font: 500 12.5px var(--font); padding: 7px 13px; border: 0; border-radius: 999px; cursor: default;
    display: inline-flex; align-items: center; gap: 7px; transition: transform 0.08s ease-out, background 0.12s ease-out;
  }
  .btn:active { transform: scale(0.94); }
  .btn.primary { background: var(--ink); color: #0b0c0e; }
  .btn.secondary { background: rgba(255, 255, 255, 0.09); color: var(--ink-2); }
  .btn.secondary:hover { background: rgba(255, 255, 255, 0.15); }
  .icon-btn {
    width: 16px; height: 16px; border: 0; border-radius: 50%; background: rgba(255, 255, 255, 0.07);
    color: var(--dim-4); display: grid; place-items: center; cursor: default; padding: 0;
  }
  .fold { position: absolute; top: 8px; right: 10px; z-index: 2; }
  .title { font: 600 15px var(--font); }
  .sub { font: 400 13px var(--font); color: var(--dim); }
  .err { margin: 0; font: 400 11px var(--font); color: #ff8d97; }

  #mini-grid {
    position: absolute; width: 29px; height: 29px; display: grid;
    grid-template-columns: 13px 13px; grid-auto-rows: 13px; gap: 3px; pointer-events: none;
    transform-origin: center; transition: opacity 0.25s ease-in-out;
  }
  .mini { width: 12px; height: 12px; margin: 0.5px; border-radius: 50%; background: #3b9eff; }
  .mini.waiting { background: #f5a524; }
  .more { font: 700 7px var(--font); color: var(--ink); display: grid; place-items: center; }
</style>
