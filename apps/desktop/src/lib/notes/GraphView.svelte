<script lang="ts">
  // Behaviour follows Logseq (AGPL-3.0): the page graph (frontend/extensions/graph.cljs) —
  // pages as nodes sized by links, journals dimmed, click a node to open the page.
  // Layout by d3-force (ISC), drawn on a canvas.
  import { log } from "../log.ts";
  import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, type Simulation, type SimulationLinkDatum, type SimulationNodeDatum } from "d3-force";
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import { Banner, Button, Checkbox, TextField } from "$lib/ds/index.ts";
  import type { GraphData, GraphNode } from "./types.ts";
  import { useNotes } from "./ctx.ts";

  interface N extends GraphNode, SimulationNodeDatum {}
  type L = SimulationLinkDatum<N>;

  const ctx = useNotes();
  let canvas = $state<HTMLCanvasElement | null>(null);
  let box = $state<HTMLDivElement | null>(null);
  let data = $state.raw<GraphData | null>(null);
  let problem = $state("");
  let showJournals = $state(false);
  let showMissing = $state(true);
  let find = $state("");
  let counts = $state({ pages: 0, links: 0 });

  let hover: N | null = null;
  let sim: Simulation<N, L> | null = null;
  let nodes: N[] = [];
  let links: L[] = [];
  let width = 600;
  let height = 400;
  let dpr = 1;
  // View transform: screen = world * k + (width/2 + x, height/2 + y).
  let view = { k: 1, x: 0, y: 0 };
  let drag: { node: N | null; moved: boolean; sx: number; sy: number; vx: number; vy: number } | null = null;
  let raf = 0;
  let palette = { text: "", muted: "", accent: "", warn: "", face: "", pt: 11 };

  const radius = (n: N) => 4 + Math.min(10, Math.sqrt(n.degree) * 2.2);
  const lower = (s: string) => s.toLowerCase();

  // The canvas cannot use var(); read the design tokens from the document and re-read them
  // whenever the theme changes.
  function readPalette() {
    const css = getComputedStyle(document.documentElement);
    const get = (name: string) => css.getPropertyValue(name).trim();
    palette = {
      text: get("--color-text"),
      muted: get("--color-text-secondary"),
      accent: get("--color-accent"),
      warn: get("--color-warn"),
      face: get("--font-sans"),
      pt: parseFloat(get("--font-size-xs")),
    };
  }

  function rebuild() {
    sim?.stop();
    if (!data) return;
    const keep = new Map(nodes.map((n) => [n.id, n]));
    nodes = data.nodes
      .filter((n) => (showJournals || !n.journal) && (showMissing || !n.missing))
      .map((n) => Object.assign(keep.get(n.id) ?? { x: (Math.random() - 0.5) * 200, y: (Math.random() - 0.5) * 200 }, n) as N);
    const ids = new Set(nodes.map((n) => n.id));
    links = data.edges.filter((e) => ids.has(e.source) && ids.has(e.target)).map((e) => ({ source: e.source, target: e.target }));
    counts = { pages: nodes.length, links: links.length };
    sim = forceSimulation<N>(nodes)
      .force("link", forceLink<N, L>(links).id((n) => n.id).distance(46).strength(0.5))
      .force("charge", forceManyBody<N>().strength(-90).distanceMax(320))
      .force("center", forceCenter(0, 0))
      .force("collide", forceCollide<N>().radius((n) => radius(n) + 3))
      .alphaDecay(0.04)
      .on("tick", schedule);
  }

  function schedule() {
    if (!raf) raf = requestAnimationFrame(() => ((raf = 0), draw()));
  }

  const toWorld = (px: number, py: number) => ({ x: (px - width / 2 - view.x) / view.k, y: (py - height / 2 - view.y) / view.k });

  function neighbours(n: N | null): Set<string> {
    const s = new Set<string>();
    if (!n) return s;
    s.add(n.id);
    for (const l of links) {
      const a = (l.source as N).id;
      const b = (l.target as N).id;
      if (a === n.id) s.add(b);
      if (b === n.id) s.add(a);
    }
    return s;
  }

  function draw() {
    const g = canvas?.getContext("2d");
    if (!g || !canvas) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, height);
    g.translate(width / 2 + view.x, height / 2 + view.y);
    g.scale(view.k, view.k);
    const near = neighbours(hover);
    const q = lower(find.trim());
    const dim = (n: N) => (hover !== null && !near.has(n.id)) || (q !== "" && !lower(n.name).includes(q));
    g.lineWidth = 1 / view.k;
    for (const l of links) {
      const a = l.source as N;
      const b = l.target as N;
      const lit = hover !== null && (a.id === hover.id || b.id === hover.id);
      g.strokeStyle = lit ? palette.accent : palette.muted;
      g.globalAlpha = hover ? (lit ? 1 : 0.12) : 0.5;
      g.beginPath();
      g.moveTo(a.x ?? 0, a.y ?? 0);
      g.lineTo(b.x ?? 0, b.y ?? 0);
      g.stroke();
    }
    for (const n of nodes) {
      g.globalAlpha = dim(n) ? 0.25 : 1;
      g.fillStyle = n.missing ? palette.muted : n.journal ? palette.warn : palette.accent;
      g.beginPath();
      g.arc(n.x ?? 0, n.y ?? 0, radius(n), 0, Math.PI * 2);
      g.fill();
      if (q && lower(n.name).includes(q)) {
        g.strokeStyle = palette.text;
        g.lineWidth = 2 / view.k;
        g.stroke();
      }
    }
    const scale = Math.max(view.k, 0.6);
    g.fillStyle = palette.text;
    g.textAlign = "center";
    g.font = `${palette.pt / scale}px ${palette.face}`;
    for (const n of nodes) {
      if (view.k < 0.55 && n.degree < 3 && n !== hover) continue;
      g.globalAlpha = dim(n) ? 0.25 : 0.9;
      g.fillText(n.name, n.x ?? 0, (n.y ?? 0) + radius(n) + 12 / scale);
    }
    g.globalAlpha = 1;
  }

  function resize() {
    if (!box || !canvas) return;
    width = box.clientWidth;
    height = box.clientHeight;
    dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    readPalette();
    schedule();
  }

  function pick(px: number, py: number): N | null {
    const w = toWorld(px, py);
    let best: N | null = null;
    let bestD = Infinity;
    for (const n of nodes) {
      const d = Math.hypot((n.x ?? 0) - w.x, (n.y ?? 0) - w.y);
      if (d <= radius(n) + 3 / view.k && d < bestD) {
        best = n;
        bestD = d;
      }
    }
    return best;
  }

  const local = (e: PointerEvent | WheelEvent) => {
    const r = canvas!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  function onpointerdown(e: PointerEvent) {
    const p = local(e);
    drag = { node: pick(p.x, p.y), moved: false, sx: p.x, sy: p.y, vx: view.x, vy: view.y };
    canvas?.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    const p = local(e);
    if (!drag) {
      const h = pick(p.x, p.y);
      if (h !== hover) {
        hover = h;
        schedule();
      }
      if (canvas) canvas.style.cursor = h ? "pointer" : "grab";
      return;
    }
    if (Math.hypot(p.x - drag.sx, p.y - drag.sy) > 3) drag.moved = true;
    if (!drag.moved) return;
    if (drag.node) {
      const w = toWorld(p.x, p.y);
      drag.node.fx = w.x;
      drag.node.fy = w.y;
      sim?.alphaTarget(0.25).restart();
    } else {
      view.x = drag.vx + (p.x - drag.sx);
      view.y = drag.vy + (p.y - drag.sy);
      schedule();
    }
  }

  function onpointerup() {
    if (!drag) return;
    const { node, moved } = drag;
    drag = null;
    if (node) {
      node.fx = node.fy = null;
      sim?.alphaTarget(0);
      if (!moved) ctx.open(node.name);
    }
  }

  function onwheel(e: WheelEvent) {
    e.preventDefault();
    const p = local(e);
    const before = toWorld(p.x, p.y);
    view.k = Math.min(4, Math.max(0.15, view.k * Math.exp(-e.deltaY * 0.0015)));
    view.x = p.x - width / 2 - before.x * view.k;
    view.y = p.y - height / 2 - before.y * view.k;
    schedule();
  }

  async function load() {
    try {
      data = await api.graphGraph();
      problem = "";
      rebuild();
    } catch (e) {
      log.warn("ui_error", { kind: "graph" });
      problem = String(e instanceof Error ? e.message : e);
    }
  }

  onMount(() => {
    resize();
    void load();
    const ro = new ResizeObserver(resize);
    if (box) ro.observe(box);
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const recolor = () => (readPalette(), schedule());
    scheme.addEventListener("change", recolor);
    const themed = new MutationObserver(recolor); // <html data-theme="light|dark"> forces a theme
    themed.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      ro.disconnect();
      scheme.removeEventListener("change", recolor);
      themed.disconnect();
      sim?.stop();
      cancelAnimationFrame(raf);
    };
  });

  // Links changed (own saves, external edits): reload the graph.
  $effect(() => {
    if (ctx.version() > 0) void load();
  });
  $effect(() => {
    void showJournals;
    void showMissing;
    rebuild();
  });
  $effect(() => {
    void find;
    schedule();
  });
</script>

<div class="graph">
  <div class="bar">
    <div class="find"><TextField placeholder="Highlight page" aria-label="Highlight page" bind:value={find} /></div>
    <Checkbox label="Journals" bind:checked={showJournals} />
    <Checkbox label="Unwritten pages" bind:checked={showMissing} />
    <Button onclick={() => { view = { k: 1, x: 0, y: 0 }; sim?.alpha(0.6).restart(); schedule(); }}>Reset view</Button>
    <span class="muted">{counts.pages} pages · {counts.links} links</span>
  </div>
  {#if problem}<Banner tone="danger">{problem}</Banner>{/if}
  <div class="stage" bind:this={box}>
    <canvas bind:this={canvas} {onpointerdown} {onpointermove} {onpointerup} onpointercancel={onpointerup} {onwheel} aria-label="Page graph. The All pages view lists the same pages for keyboard use."></canvas>
  </div>
</div>

<style>
  .graph { display: flex; flex-direction: column; height: 100%; min-height: 360px; }
  .bar { display: flex; gap: var(--space-4); align-items: center; padding-bottom: var(--space-2); flex-wrap: wrap; }
  .find { flex: 1; max-width: 220px; min-width: 140px; }
  .stage { flex: 1; min-height: 0; border: var(--border-width) solid var(--color-border); border-radius: var(--radius-lg); overflow: hidden; background: var(--color-surface); }
  canvas { width: 100%; height: 100%; display: block; touch-action: none; cursor: grab; }
</style>
