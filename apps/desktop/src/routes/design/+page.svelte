<script lang="ts">
  import { goto } from "$app/navigation";
  import { onDestroy } from "svelte";
  import { app } from "$lib/app.svelte.ts";
  import { api, type BoardMeta } from "$lib/api.ts";
  import DesignFrame from "$lib/components/DesignFrame.svelte";
  import {
    DIRECTION_SANDBOX,
    FRAMES,
    FRAMEWORKS,
    STYLE_FIELDS,
    averageScore,
    batchHeads,
    fieldValues,
    lineage,
    progressLabel,
    readBridgeMessage,
    safeValue,
    styleChanges,
    tokenOptions,
    type DesignDirection,
    type DesignFiles,
    type DesignProgress,
    type DesignTask,
    type FrameKind,
    type Framework,
    type ModelInfo,
    type PreviewInfo,
    type RawLocation,
    type SourceView,
  } from "$lib/design.ts";

  const ws = $derived(app.activeWorkspace);
  let tab = $state<"explore" | "edit">("explore");
  let problem = $state("");
  let notices = $state<string[]>([]);
  const fail = (e: unknown) => (problem = String(e instanceof Error ? e.message : e));

  // ---- design system ----
  let files = $state<DesignFiles | null>(null);
  let editing = $state(false);
  let draftMd = $state("");
  let draftCss = $state("");
  let editorTab = $state<"md" | "css">("md");
  let confirmReplace = $state(false);
  let systemBusy = $state(false);

  // ---- explore ----
  let prompt = $state("");
  let frame = $state<FrameKind>("web");
  let count = $state(3);
  let board = $state("");
  let boards = $state<BoardMeta[]>([]);
  let critique = $state(true);
  let progress = $state<DesignProgress | null>(null);
  let generating = $state(false);
  let history = $state<DesignDirection[]>([]);
  let batchId = $state<string | null>(null);
  let openId = $state<string | null>(null);
  let model = $state<ModelInfo | null>(null);
  let chat = $state("");
  let refining = $state(false);
  let framework = $state<Framework | "">("");
  let tasks = $state<DesignTask[]>([]);
  let handing = $state(false);

  const heads = $derived(batchId ? batchHeads(history, batchId) : []);
  const open = $derived(history.find((d) => d.id === openId) ?? null);
  const versions = $derived(open ? lineage(history, open.id) : []);
  const batches = $derived(
    [...new Map(history.map((d) => [d.batchId, d])).values()].map((d) => ({ id: d.batchId, prompt: d.prompt, frame: d.frame })),
  );
  const colorTokens = $derived(files?.tokens.filter((t) => t.kind === "color") ?? []);
  const otherTokens = $derived(files?.tokens.filter((t) => t.kind !== "color" && t.kind !== "other") ?? []);

  // ---- edit app ----
  let previewUrl = $state("");
  let preview = $state<PreviewInfo | null>(null);
  let previewFrame = $state<FrameKind>("web");
  let iframe = $state<HTMLIFrameElement | null>(null);
  let selecting = $state(true);
  let starting = $state(false);
  let log = $state<string[]>([]);
  let selected = $state<{ location: RawLocation; text: string } | null>(null);
  let source = $state<SourceView | null>(null);
  let before = $state<Record<string, string>>({});
  let values = $state<Record<string, string>>({});
  let editNote = $state("");
  let writing = $state(false);
  const changes = $derived(styleChanges(before, values));
  const unsafe = $derived(changes.filter((c) => c.value && !safeValue(c.value)));

  let loadedFor = "";
  $effect(() => {
    const id = ws?.id;
    if (!id || id === loadedFor) return;
    loadedFor = id;
    void loadWorkspace(id);
  });

  async function loadWorkspace(id: string) {
    problem = "";
    files = null;
    history = [];
    batchId = null;
    openId = null;
    await stopPreview();
    try {
      files = await api.designSystemGet(id);
      history = await api.designHistory(id);
      batchId = history[0]?.batchId ?? null;
    } catch (e) {
      fail(e);
    }
    try {
      boards = await api.boardList();
    } catch {
      boards = []; // no vault: sketches are optional
    }
  }

  // ---- design system actions ----
  async function generateSystem(overwrite: boolean) {
    if (!ws) return;
    systemBusy = true;
    try {
      files = await api.designSystemGenerate(ws.id, overwrite);
      confirmReplace = false;
      notices = [`Wrote ${files.designPath} and ${files.tokensPath} from ${files.sources.length} style file(s).`];
    } catch (e) {
      const text = String(e);
      if (text.startsWith("exists:")) confirmReplace = true;
      else fail(e);
    } finally {
      systemBusy = false;
    }
  }
  function startEditing() {
    if (!files) return;
    draftMd = files.designMd;
    draftCss = files.tokensCss;
    editorTab = "md";
    editing = true;
  }
  async function saveSystem() {
    if (!ws) return;
    systemBusy = true;
    try {
      files = await api.designSystemSave(ws.id, draftMd, draftCss);
      editing = false;
      notices = [`Saved ${files.designPath} and ${files.tokensPath}.`];
    } catch (e) {
      fail(e);
    } finally {
      systemBusy = false;
    }
  }

  // ---- explore actions ----
  async function generate(event?: SubmitEvent) {
    event?.preventDefault();
    if (!ws || !prompt.trim() || generating) return;
    generating = true;
    problem = "";
    progress = null;
    try {
      const batch = await api.designGenerate(ws.id, prompt, frame, count, board || null, critique, (p) => (progress = p));
      model = batch.model;
      notices = batch.notices;
      history = [...batch.directions, ...history];
      batchId = batch.batchId;
      openId = null;
    } catch (e) {
      fail(e);
    } finally {
      generating = false;
      progress = null;
    }
  }
  function promptKeys(event: KeyboardEvent) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      (event.currentTarget as HTMLTextAreaElement).form?.requestSubmit();
    }
  }
  async function pick(d: DesignDirection) {
    openId = d.id;
    try {
      await api.designChoose(d.id);
      history = history.map((x) => (x.batchId === d.batchId ? { ...x, chosen: x.id === d.id } : x));
    } catch (e) {
      fail(e);
    }
  }
  async function refine(event: SubmitEvent) {
    event.preventDefault();
    if (!open || !chat.trim() || refining) return;
    refining = true;
    const message = chat;
    try {
      const next = await api.designIterate(open.id, message);
      chat = "";
      history = [next, ...history.map((x) => (x.batchId === next.batchId ? { ...x, chosen: false } : x))];
      openId = next.id;
    } catch (e) {
      fail(e);
    } finally {
      refining = false;
    }
  }
  async function handOff(kind: "convert" | "implement") {
    if (!open || handing) return;
    handing = true;
    const id = open.id;
    let started: DesignTask | null = null;
    const title = kind === "convert" ? `Design convert: ${open.title}` : `Implement design: ${open.title}`;
    await app.trackTask(title, async (deliver) => {
      started = kind === "convert" ? await api.designConvert(id, framework || null, deliver) : await api.designImplement(id, deliver);
      return started.taskId;
    });
    if (started) tasks = [started, ...tasks];
    else if (app.error) fail(app.error);
    handing = false;
  }
  function review(taskId: string) {
    app.selectTask(taskId);
    void goto("/");
  }

  // ---- edit app actions ----
  async function startPreview(event?: SubmitEvent) {
    event?.preventDefault();
    if (!ws) return;
    starting = true;
    problem = "";
    try {
      preview = await api.designPreviewStart(ws.id, previewUrl.trim() || null, window.location.origin);
      clearSelection();
      pollLog();
    } catch (e) {
      fail(e);
    } finally {
      starting = false;
    }
  }
  async function stopPreview() {
    if (logTimer) clearInterval(logTimer);
    logTimer = null;
    if (preview && loadedFor) {
      try {
        await api.designPreviewStop(loadedFor);
      } catch {
        // already gone
      }
    }
    preview = null;
    clearSelection();
  }
  let logTimer: ReturnType<typeof setInterval> | null = null;
  function pollLog() {
    if (logTimer) clearInterval(logTimer);
    if (!preview?.command || !ws) return;
    const id = ws.id;
    logTimer = setInterval(async () => {
      try {
        log = await api.designPreviewLog(id);
      } catch {
        // ignore
      }
    }, 2000);
  }
  function reload() {
    if (iframe && preview) iframe.src = preview.url;
    clearSelection();
  }
  function post(type: string, data: object = {}) {
    if (!iframe?.contentWindow || !preview) return;
    iframe.contentWindow.postMessage({ ns: "next-agent", type, ...data }, preview.origin);
  }
  function clearSelection() {
    selected = null;
    source = null;
    before = {};
    values = {};
    editNote = "";
  }
  $effect(() => {
    // Keep the bridge's mode in sync with the toggle.
    post("mode", { edit: tab === "edit" && selecting });
  });

  async function onMessage(event: MessageEvent) {
    if (!preview) return;
    const msg = readBridgeMessage(event, preview.origin, iframe?.contentWindow ?? null);
    if (!msg) return;
    if (msg.type === "ready") post("mode", { edit: tab === "edit" && selecting });
    else if (msg.type === "nosource") editNote = `No source location for <${msg.tag}>. Svelte 5 and React 18 dev builds work as is; otherwise add nextAgentSource() from @next-agent/design-instrument to vite.config.`;
    else if (msg.type === "deselected") clearSelection();
    else if (msg.type === "styled") {
      // Preview applied; keep form values as typed.
    } else if (msg.type === "select" && ws) {
      selected = { location: msg.location, text: msg.text };
      before = fieldValues(msg.styles);
      values = { ...before };
      editNote = "";
      try {
        source = await api.designSource(ws.id, msg.location);
      } catch (e) {
        source = null;
        editNote = String(e);
      }
    }
  }
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  function previewStyles() {
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      const safe = changes.filter((c) => !c.value || safeValue(c.value));
      post("reset");
      if (safe.length) post("style", { changes: safe });
    }, 120);
  }
  function resetStyles() {
    values = { ...before };
    post("reset");
  }
  async function writeToCode() {
    if (!ws || !selected || !changes.length || unsafe.length || writing) return;
    writing = true;
    const location = selected.location;
    const id = ws.id;
    let started: DesignTask | null = null;
    await app.trackTask(`Design edit: <${location.tag}>`, async (deliver) => {
      started = await api.designApplyStyle(id, location, changes, deliver);
      return started.taskId;
    });
    if (started) {
      tasks = [started, ...tasks];
      before = { ...values };
    } else if (app.error) fail(app.error);
    writing = false;
  }

  onDestroy(() => {
    void stopPreview();
  });

  const swatch = (v: string) => (/^(#|rgb|hsl|oklch|oklab|lab|lch|hwb)/i.test(v.trim()) ? v : "transparent");
</script>

<svelte:window onmessage={onMessage} />

<div class="studio">
  <header class="bar">
    <h1>Design</h1>
    {#if ws}<span class="muted ws" title={ws.path}>{ws.name}</span>{/if}
    <div role="tablist" aria-label="Studio mode" class="modes">
      <button role="tab" id="tab-explore" aria-selected={tab === "explore"} aria-controls="studio-main" onclick={() => (tab = "explore")}>Explore</button>
      <button role="tab" id="tab-edit" aria-selected={tab === "edit"} aria-controls="studio-main" onclick={() => (tab = "edit")}>Edit app</button>
    </div>
    <span class="model" title={model?.reason ?? "The design model role (Settings → Providers)"}>
      Model: {model ? model.label : "design role"}
    </span>
  </header>

  {#if problem}<div class="problem" role="alert">{problem} <button onclick={() => (problem = "")} aria-label="Dismiss error">×</button></div>{/if}
  {#each notices as n (n)}<p class="notice" role="status">{n}</p>{/each}

  {#if !ws}
    <p class="empty">Open a workspace in the sidebar to design for it.</p>
  {:else}
    <div class="cols">
      <!-- Design system -->
      <aside class="system" aria-labelledby="ds-title">
        <h2 id="ds-title">Design system</h2>
        {#if files}
          <p class="muted small">
            <code>{files.designPath}</code> · <code>{files.tokensPath}</code><br />
            {files.saved ? "Saved in the workspace" : "Draft from the project's CSS (not saved)"}
          </p>
          <div class="row">
            <button onclick={() => generateSystem(false)} disabled={systemBusy}>Generate from project</button>
            <button onclick={startEditing} disabled={systemBusy}>Edit files</button>
          </div>
          {#if confirmReplace}
            <div class="confirm" role="alertdialog" aria-labelledby="replace-q">
              <p id="replace-q">Replace the saved DESIGN.md and tokens.css with a fresh read of the project's CSS?</p>
              <div class="row">
                <button class="danger" onclick={() => generateSystem(true)}>Replace</button>
                <button onclick={() => (confirmReplace = false)}>Cancel</button>
              </div>
            </div>
          {/if}
          <h3>Colour</h3>
          <ul class="swatches">
            {#each colorTokens as t (t.name)}
              <li title="{t.name}: {t.value}{t.dark ? ` / dark ${t.dark}` : ''}">
                <span class="chip" aria-hidden="true"><i style="background: {swatch(t.value)}"></i>{#if t.dark}<i style="background: {swatch(t.dark)}"></i>{/if}</span>
                <code>{t.name}</code>
              </li>
            {:else}
              <li class="muted">No colour tokens</li>
            {/each}
          </ul>
          {#if otherTokens.length}
            <h3>Type, space, shape</h3>
            <ul class="tokens">
              {#each otherTokens as t (t.name)}
                <li><code>{t.name}</code> <span class="muted">{t.value}</span>{#if t.derived}<span class="tag" title="Derived from literal values in the CSS">derived</span>{/if}</li>
              {/each}
            </ul>
          {/if}
        {:else}
          <p class="muted">Reading the project…</p>
        {/if}
      </aside>

      <!-- Main column -->
      <div class="main" id="studio-main" role="tabpanel" aria-labelledby={tab === "explore" ? "tab-explore" : "tab-edit"}>
        {#if editing}
          <div class="editor">
            <div class="row between">
              <div role="tablist" aria-label="Design file" class="modes">
                <button role="tab" aria-selected={editorTab === "md"} onclick={() => (editorTab = "md")}>DESIGN.md</button>
                <button role="tab" aria-selected={editorTab === "css"} onclick={() => (editorTab = "css")}>tokens.css</button>
              </div>
              <div class="row">
                <button onclick={() => (editing = false)}>Cancel</button>
                <button class="primary" onclick={saveSystem} disabled={systemBusy}>Save both</button>
              </div>
            </div>
            {#if editorTab === "md"}
              <label class="sr-only" for="ds-md">DESIGN.md</label>
              <textarea id="ds-md" class="code" spellcheck="false" bind:value={draftMd}></textarea>
            {:else}
              <label class="sr-only" for="ds-css">tokens.css</label>
              <textarea id="ds-css" class="code" spellcheck="false" bind:value={draftCss}></textarea>
            {/if}
          </div>
        {:else if tab === "explore"}
          <form class="composer" onsubmit={generate} aria-label="Generate directions">
            <label for="design-prompt">What should we design?</label>
            <textarea id="design-prompt" rows="3" bind:value={prompt} onkeydown={promptKeys} placeholder="A settings screen for team members and roles… (⌘/Ctrl+Enter to generate)"></textarea>
            <div class="row wrap">
              <fieldset class="seg">
                <legend class="sr-only">Frame</legend>
                {#each Object.entries(FRAMES) as [id, f] (id)}
                  <label class:on={frame === id}><input type="radio" name="frame" value={id} bind:group={frame} />{f.label}</label>
                {/each}
              </fieldset>
              <label class="inline">Directions
                <select bind:value={count}>{#each [3, 4, 5] as c (c)}<option value={c}>{c}</option>{/each}</select>
              </label>
              <label class="inline">Sketch
                <select bind:value={board} disabled={!boards.length} title={boards.length ? "Follow a Design board's layout" : "No boards (set a vault and draw one in Boards)"}>
                  <option value="">None</option>
                  {#each boards as b (b.name)}<option value={b.name}>{b.name}</option>{/each}
                </select>
              </label>
              <label class="inline"><input type="checkbox" bind:checked={critique} /> Self-critique</label>
              <button class="primary" type="submit" disabled={!prompt.trim() || generating}>{generating ? "Generating…" : "Generate"}</button>
            </div>
            <p class="muted small" aria-live="polite">{progressLabel(progress)}</p>
          </form>

          {#if batches.length > 1}
            <label class="inline small">Earlier requests
              <select value={batchId} onchange={(e) => { batchId = (e.currentTarget as HTMLSelectElement).value; openId = null; }}>
                {#each batches as b (b.id)}<option value={b.id}>{b.prompt.slice(0, 60)} · {FRAMES[b.frame].label}</option>{/each}
              </select>
            </label>
          {/if}

          {#if open}
            <div class="focus">
              <div class="row between">
                <button onclick={() => (openId = null)}>← All directions</button>
                <div class="versions" role="group" aria-label="Versions">
                  {#each versions as v, i (v.id)}
                    <button aria-pressed={v.id === open.id} onclick={() => (openId = v.id)} title={v.note || "Original"}>v{i + 1}</button>
                  {/each}
                </div>
              </div>
              <h2>{open.title}</h2>
              {#if open.rationale}<p class="muted">{open.rationale}</p>{/if}
              <DesignFrame frame={open.frame} title="Direction: {open.title}" sandbox={DIRECTION_SANDBOX} srcdoc={open.srcdoc} />
            </div>
          {:else if heads.length}
            <ul class="grid" aria-label="Directions">
              {#each heads as d (d.id)}
                {@const score = averageScore(d.critique)}
                {@const p0 = d.lint.filter((f) => f.severity === "p0").length}
                <li class="card" class:chosen={d.chosen}>
                  <DesignFrame frame={d.frame} title="Thumbnail: {d.title}" sandbox={DIRECTION_SANDBOX} srcdoc={d.srcdoc} thumb />
                  <div class="meta">
                    <h3>{d.title}</h3>
                    {#if d.rationale}<p class="muted small">{d.rationale}</p>{/if}
                    {#if d.critique}
                      <div class="scores" title={d.critique.notes}>
                        <span class="score">{score}/5{#if d.critique.revised} · revised{/if}</span>
                        {#each d.critique.scores as [dim, s] (dim)}
                          <span class="dim" title="{dim}: {s}/5"><span class="sr-only">{dim} {s} of 5</span><i style="height: {s * 20}%" class:low={s < 3}></i></span>
                        {/each}
                      </div>
                    {/if}
                    {#if d.lint.length}
                      <details class="lint"><summary>{p0 ? `${p0} issue${p0 > 1 ? "s" : ""}` : `${d.lint.length} note${d.lint.length > 1 ? "s" : ""}`}</summary>
                        <ul>{#each d.lint as f (f.rule)}<li><b>{f.severity.toUpperCase()}</b> {f.message}</li>{/each}</ul>
                      </details>
                    {/if}
                    <button class="primary" onclick={() => pick(d)}>Pick this direction</button>
                  </div>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="empty">Describe a screen and generate 3–5 on-brand directions. Each one renders in a sandbox with no network or scripts, and is self-critiqued before you see it.</p>
          {/if}
        {:else}
          <form class="toolbar" onsubmit={startPreview} aria-label="Live preview">
            <label class="sr-only" for="preview-url">Dev server URL</label>
            <input id="preview-url" bind:value={previewUrl} placeholder="http://localhost:5173 (leave empty to start the workspace's dev server)" />
            <button class="primary" type="submit" disabled={starting}>{starting ? "Starting…" : preview ? "Restart" : "Start"}</button>
            {#if preview}
              <button type="button" onclick={reload}>Reload</button>
              <button type="button" onclick={stopPreview}>Stop</button>
              <button type="button" aria-pressed={selecting} onclick={() => (selecting = !selecting)} title="Click elements in the preview to inspect them">Select elements</button>
              <label class="sr-only" for="preview-frame">Frame</label>
              <select id="preview-frame" bind:value={previewFrame}>{#each Object.entries(FRAMES) as [id, f] (id)}<option value={id}>{f.label}</option>{/each}</select>
            {/if}
          </form>
          {#if preview}
            <p class="muted small">Proxying {preview.target}{#if preview.command} · started <code>{preview.command}</code>{/if}. Enter selects the focused element, Escape clears.</p>
            <DesignFrame frame={previewFrame} title="Live app preview" sandbox={preview.sandbox} src={preview.url} bind:iframe />
            {#if log.length}
              <details class="log"><summary>Dev server output</summary><pre>{log.join("\n")}</pre></details>
            {/if}
          {:else}
            <p class="empty">Run the real app here, click an element to jump to its source, and change its style or layout. Changes preview instantly, then land in the code as a task in a worktree that you review and merge in Tasks.</p>
          {/if}
        {/if}
      </div>

      <!-- Right column -->
      <aside class="side" aria-label={tab === "explore" ? "Refine and hand off" : "Inspector"}>
        {#if tab === "explore"}
          {#if open}
            <h2>Refine</h2>
            <ol class="chat" aria-label="Changes">
              <li class="muted small">Original: “{open.prompt}”</li>
              {#each versions.filter((v) => v.note) as v (v.id)}
                <li class="you">{v.note}</li>
                <li class="model-reply">{v.reply}</li>
              {/each}
            </ol>
            <form class="ask" onsubmit={refine}>
              <label class="sr-only" for="refine">Ask for a change</label>
              <textarea id="refine" rows="2" bind:value={chat} placeholder="Tighter header, larger type…" onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); (e.currentTarget as HTMLTextAreaElement).form?.requestSubmit(); } }}></textarea>
              <button class="primary" type="submit" disabled={!chat.trim() || refining}>{refining ? "Updating…" : "Send"}</button>
            </form>
            <h2>Hand off</h2>
            <div class="row wrap">
              <label class="sr-only" for="framework">Framework</label>
              <select id="framework" bind:value={framework}>
                <option value="">Project's framework</option>
                {#each FRAMEWORKS as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
              </select>
              <button onclick={() => handOff("convert")} disabled={handing}>Convert to code</button>
            </div>
            <button class="primary block" onclick={() => handOff("implement")} disabled={handing}>Implement this</button>
            <p class="muted small">Both run as tasks in a worktree. Review the diff and merge in Tasks.</p>
          {:else}
            <p class="muted">Pick a direction to refine it by chat, convert it to the project's framework, or hand it to the coding agent.</p>
          {/if}
        {:else if selected}
          <h2>&lt;{selected.location.tag}&gt;</h2>
          {#if source}
            <p class="small"><code>{source.file}:{source.line}</code></p>
            <pre class="source" aria-label="Source around line {source.line}">{#each source.lines as l, i (i)}<span class:hit={source.firstLine + i === source.line}><b>{source.firstLine + i}</b>{l}
</span>{/each}</pre>
          {/if}
          <div class="row"><button onclick={() => post("parent")}>Select parent</button><button onclick={() => { post("deselect"); clearSelection(); }}>Clear</button></div>
          {#each ["Layout", "Spacing", "Type", "Fill"] as group (group)}
            <fieldset class="fields">
              <legend>{group}</legend>
              {#each STYLE_FIELDS.filter((f) => f.group === group) as f (f.key)}
                <label for="f-{f.key}">{f.label}</label>
                {#if f.options}
                  <select id="f-{f.key}" bind:value={values[f.key]} onchange={previewStyles}>
                    {#if !f.options.includes(values[f.key] ?? "")}<option value={values[f.key]}>{values[f.key]}</option>{/if}
                    {#each f.options as o (o)}<option value={o}>{o || "—"}</option>{/each}
                  </select>
                {:else}
                  <input id="f-{f.key}" list="tok-{f.key}" bind:value={values[f.key]} oninput={previewStyles} aria-invalid={!safeValue(values[f.key] ?? "")} />
                  {#if files}<datalist id="tok-{f.key}">{#each tokenOptions(f, files.tokens) as o (o)}<option value={o}></option>{/each}</datalist>{/if}
                {/if}
              {/each}
            </fieldset>
          {/each}
          {#if unsafe.length}<p class="warn small" role="alert">Not allowed in a style value: quotes, ; {"{}"} &lt;&gt; \ or url().</p>{/if}
          <div class="row">
            <button onclick={resetStyles} disabled={!changes.length}>Reset preview</button>
            <button class="primary" onclick={writeToCode} disabled={!changes.length || unsafe.length > 0 || writing || !source}>{writing ? "Starting task…" : `Write ${changes.length || ""} change${changes.length === 1 ? "" : "s"} to code`}</button>
          </div>
          {#if editNote}<p class="warn small">{editNote}</p>{/if}
        {:else}
          <h2>Inspector</h2>
          <p class="muted">{preview ? "Click an element in the preview." : "Start the preview first."}</p>
          {#if editNote}<p class="warn small">{editNote}</p>{/if}
        {/if}

        {#if tasks.length}
          <h2>Tasks from the studio</h2>
          <ul class="tasks">
            {#each tasks as t (t.taskId)}
              <li><span>{t.title}</span><button onclick={() => review(t.taskId)}>Review diff</button></li>
            {/each}
          </ul>
        {/if}
      </aside>
    </div>
  {/if}
</div>

<style>
  .studio { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .bar { display: flex; align-items: center; gap: 12px; padding: 8px 12px; border-bottom: 1px solid var(--border); background: var(--panel); }
  .bar h1 { font-size: 16px; }
  .ws { font-size: 12px; }
  .modes { display: inline-flex; border: 1px solid var(--border); border-radius: 6px; overflow: hidden; }
  .modes button { border: 0; border-radius: 0; background: transparent; }
  .modes button[aria-selected="true"] { background: var(--accent); color: #fff; }
  .model { margin-left: auto; font-size: 12px; color: var(--muted); }
  .problem { background: var(--danger); color: #fff; padding: 6px 12px; display: flex; justify-content: space-between; gap: 8px; }
  .problem button { background: transparent; color: #fff; border-color: transparent; }
  .notice { margin: 0; padding: 6px 12px; background: var(--sidebar); border-bottom: 1px solid var(--border); font-size: 12px; }
  .empty { color: var(--muted); padding: 24px; max-width: 60ch; }
  .cols { display: grid; grid-template-columns: 240px minmax(0, 1fr) 300px; flex: 1; min-height: 0; }
  .system, .side { overflow: auto; padding: 12px; display: flex; flex-direction: column; gap: 8px; background: var(--panel); }
  .system { border-right: 1px solid var(--border); }
  .side { border-left: 1px solid var(--border); }
  .main { overflow: auto; padding: 12px; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  h2 { font-size: 13px; } h3 { font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); margin-top: 6px; }
  .small { font-size: 12px; margin: 0; }
  .row { display: flex; gap: 6px; align-items: center; }
  .row.wrap { flex-wrap: wrap; }
  .between { justify-content: space-between; }
  .inline { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; white-space: nowrap; }
  .block { width: 100%; }
  select { font: inherit; color: inherit; background: var(--panel); border: 1px solid var(--border); border-radius: 6px; padding: 4px 6px; }
  .swatches { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .swatches li { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .chip { display: inline-flex; border: 1px solid var(--border); border-radius: 4px; overflow: hidden; }
  .chip i { width: 14px; height: 18px; display: block; }
  .tokens { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; font-size: 12px; }
  .tag { margin-left: 4px; font-size: 10px; border: 1px solid var(--border); border-radius: 999px; padding: 0 5px; color: var(--muted); }
  .confirm { border: 1px solid var(--warn); border-radius: 6px; padding: 8px; font-size: 12px; }
  .confirm p { margin: 0 0 6px; }
  .composer { display: grid; gap: 8px; background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 10px; }
  .composer > label { font-weight: 600; }
  textarea { resize: vertical; }
  .seg { display: inline-flex; border: 1px solid var(--border); border-radius: 6px; padding: 0; margin: 0; overflow: hidden; }
  .seg label { padding: 4px 10px; cursor: pointer; font-size: 12px; position: relative; }
  .seg label.on { background: var(--accent); color: #fff; }
  .seg input { position: absolute; opacity: 0; inset: 0; cursor: pointer; }
  .seg label:has(input:focus-visible) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px; }
  .card { border: 1px solid var(--border); border-radius: 8px; background: var(--panel); padding: 8px; display: flex; flex-direction: column; gap: 8px; }
  .card.chosen { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
  .meta { display: grid; gap: 6px; }
  .meta h3 { font-size: 14px; text-transform: none; letter-spacing: 0; color: var(--text); margin: 0; }
  .scores { display: flex; align-items: flex-end; gap: 3px; height: 22px; }
  .score { font-size: 12px; margin-right: 6px; align-self: center; }
  .dim { width: 6px; height: 100%; background: var(--sidebar); border-radius: 2px; display: flex; align-items: flex-end; overflow: hidden; }
  .dim i { width: 100%; background: var(--ok); display: block; }
  .dim i.low { background: var(--warn); }
  .lint { font-size: 12px; } .lint ul { margin: 4px 0 0; padding-left: 16px; }
  .focus { display: grid; gap: 8px; }
  .versions { display: inline-flex; gap: 4px; }
  .versions button[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
  .editor { display: flex; flex-direction: column; gap: 8px; flex: 1; min-height: 0; }
  .code { flex: 1; min-height: 420px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
  .toolbar { display: flex; gap: 6px; align-items: center; }
  .toolbar input { flex: 1; }
  .toolbar button[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
  .log pre { max-height: 180px; overflow: auto; font-size: 11px; background: var(--sidebar); padding: 8px; border-radius: 6px; }
  .chat { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .chat .you { justify-self: end; background: var(--accent); color: #fff; border-radius: 10px 10px 2px 10px; padding: 4px 10px; max-width: 90%; }
  .chat .model-reply { background: var(--sidebar); border-radius: 10px 10px 10px 2px; padding: 4px 10px; max-width: 90%; font-size: 13px; }
  .ask { display: grid; gap: 6px; }
  .source { font-size: 11px; background: var(--sidebar); border-radius: 6px; padding: 6px 0; overflow: auto; max-height: 220px; margin: 0; }
  .source span { display: block; padding: 0 8px; white-space: pre; }
  .source span.hit { background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .source b { display: inline-block; width: 3ch; color: var(--muted); font-weight: 400; margin-right: 8px; text-align: right; }
  .fields { border: 1px solid var(--border); border-radius: 6px; padding: 6px 8px; margin: 0; display: grid; grid-template-columns: 90px 1fr; gap: 4px 8px; align-items: center; font-size: 12px; }
  .fields legend { font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); padding: 0 4px; }
  .fields input, .fields select { padding: 2px 6px; font-size: 12px; }
  .fields input[aria-invalid="true"] { border-color: var(--danger); }
  .warn { color: var(--warn); }
  .tasks { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; font-size: 12px; }
  .tasks li { display: flex; justify-content: space-between; gap: 6px; align-items: center; }
  .tasks span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 1100px) { .cols { grid-template-columns: 200px minmax(0, 1fr) 260px; } }
</style>
