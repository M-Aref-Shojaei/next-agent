<script lang="ts">
  import { goto } from "$app/navigation";
  import { onDestroy, untrack } from "svelte";
  import { app } from "$lib/app.svelte.ts";
  import { api, type BoardMeta } from "$lib/api.ts";
  import { errorText } from "$lib/errors.ts";
  import DesignFrame from "$lib/components/DesignFrame.svelte";
  import DirectionCard from "$lib/components/design/DirectionCard.svelte";
  import SystemPanel from "$lib/components/design/SystemPanel.svelte";
  import { Banner, Button, Checkbox, Dialog, EmptyState, List, ListRow, SegmentedControl, Select, TextArea, TextField, type Option } from "$lib/ds/index.ts";
  import {
    DIRECTION_SANDBOX,
    FRAMES,
    FRAMEWORKS,
    STYLE_FIELDS,
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

  const MODES: Option<"explore" | "edit">[] = [
    { value: "explore", label: "Explore" },
    { value: "edit", label: "Edit app" },
  ];
  const EDITOR_TABS: Option<"md" | "css">[] = [
    { value: "md", label: "DESIGN.md" },
    { value: "css", label: "tokens.css" },
  ];
  const FRAME_OPTIONS: Option<FrameKind>[] = (Object.entries(FRAMES) as [FrameKind, { label: string }][]).map(([value, f]) => ({ value, label: f.label }));
  const COUNT_OPTIONS: Option[] = ["3", "4", "5"].map((c) => ({ value: c, label: c }));
  const FRAMEWORK_OPTIONS: Option<Framework | "">[] = [{ value: "", label: "Project's framework" }, ...FRAMEWORKS.map((f) => ({ value: f.id, label: f.label }))];
  /** A style field's choices, plus its current value when that is not one of them. */
  const fieldOptions = (options: readonly string[], current: string | undefined): Option[] =>
    [...(current !== undefined && !options.includes(current) ? [current] : []), ...options].map((o) => ({ value: o, label: o || "—" }));

  const ws = $derived(app.activeWorkspace);
  let tab = $state<"explore" | "edit">("explore");
  let problem = $state("");
  let notices = $state<string[]>([]);
  const fail = (e: unknown) => (problem = errorText(e));

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
  let count = $state("3");
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

  // ---- edit app ----
  let previewUrl = $state("");
  /** Workspace the running preview belongs to. */
  let previewWs = "";
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
      const batch = await api.designGenerate(ws.id, prompt, frame, Number(count), board || null, critique, (p) => (progress = p));
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
  /** Running the project's own `dev` script runs repository code: ask first. */
  let confirmDev = $state(false);
  async function startPreview(event?: SubmitEvent) {
    event?.preventDefault();
    if (!ws) return;
    if (!previewUrl.trim() && !confirmDev) {
      confirmDev = true;
      return;
    }
    confirmDev = false;
    starting = true;
    problem = "";
    try {
      preview = await api.designPreviewStart(ws.id, previewUrl.trim() || null, window.location.origin);
      previewWs = ws.id;
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
    if (preview && previewWs) {
      try {
        await api.designPreviewStop(previewWs);
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
    // Keep the bridge's mode in sync with the toggle; leaving edit mode drops
    // the selection and its unwritten style previews.
    const edit = tab === "edit" && selecting;
    post("mode", { edit });
    if (!edit && untrack(() => selected)) {
      post("deselect");
      clearSelection();
    }
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
</script>

<svelte:window onmessage={onMessage} />

<div class="studio">
  <header class="bar">
    <h1>Design</h1>
    {#if ws}<span class="muted ws" title={ws.path}>{ws.name}</span>{/if}
    <SegmentedControl label="Studio mode" size="sm" options={MODES} bind:value={tab} />
    <span class="model" title={model?.reason ?? "The design model role (Settings → Providers)"}>
      Model: {model ? model.label : "design role"}
    </span>
  </header>

  {#if problem}<Banner tone="danger" ondismiss={() => (problem = "")}>{problem}</Banner>{/if}
  {#each notices as n (n)}<Banner tone="info">{n}</Banner>{/each}

  {#if !ws}
    <EmptyState title="No workspace open" description="Open a workspace in the sidebar to design for it." />
  {:else}
    <div class="cols">
      <SystemPanel {files} busy={systemBusy} bind:confirmReplace ongenerate={generateSystem} onedit={startEditing} />

      <!-- Main column -->
      <div class="main" id="studio-main">
        {#if editing}
          <div class="editor">
            <div class="row between">
              <SegmentedControl label="Design file" size="sm" options={EDITOR_TABS} bind:value={editorTab} />
              <div class="row">
                <Button onclick={() => (editing = false)}>Cancel</Button>
                <Button variant="primary" onclick={saveSystem} disabled={systemBusy}>Save both</Button>
              </div>
            </div>
            {#if editorTab === "md"}
              <TextArea id="ds-md" label="DESIGN.md" hideLabel class="code" mono spellcheck={false} bind:value={draftMd} />
            {:else}
              <TextArea id="ds-css" label="tokens.css" hideLabel class="code" mono spellcheck={false} bind:value={draftCss} />
            {/if}
          </div>
        {:else if tab === "explore"}
          <form class="composer" onsubmit={generate} aria-label="Generate directions">
            <TextArea id="design-prompt" label="What should we design?" rows={3} bind:value={prompt} onkeydown={promptKeys} placeholder="A settings screen for team members and roles… (⌘/Ctrl+Enter to generate)" />
            <div class="row wrap">
              <SegmentedControl label="Frame" size="sm" options={FRAME_OPTIONS} bind:value={frame} />
              <Select label="Directions" size="sm" bind:value={count} options={COUNT_OPTIONS} />
              <Select
                label="Sketch"
                size="sm"
                bind:value={board}
                options={[{ value: "", label: "None" }, ...boards.map((b) => ({ value: b.name, label: b.name }))]}
                disabled={!boards.length}
                title={boards.length ? "Follow a Design board's layout" : "No boards (set a vault and draw one in Boards)"}
              />
              <Checkbox label="Self-critique" bind:checked={critique} />
              <Button variant="primary" type="submit" disabled={!prompt.trim() || generating}>{generating ? "Generating…" : "Generate"}</Button>
            </div>
            <p class="muted small" aria-live="polite">{progressLabel(progress)}</p>
          </form>

          {#if batches.length > 1}
            <Select
              label="Earlier requests"
              size="sm"
              value={batchId ?? ""}
              options={batches.map((b) => ({ value: b.id, label: `${b.prompt.slice(0, 60)} · ${FRAMES[b.frame].label}` }))}
              onchange={(e) => { batchId = e.currentTarget.value; openId = null; }}
            />
          {/if}

          {#if open}
            <div class="focus">
              <div class="row between">
                <Button onclick={() => (openId = null)}>← All directions</Button>
                <SegmentedControl label="Versions" size="sm" options={versions.map((v, i) => ({ value: v.id, label: `v${i + 1}` }))} value={open.id} onchange={(id) => (openId = id)} />
              </div>
              <h2>{open.title}</h2>
              {#if open.rationale}<p class="muted">{open.rationale}</p>{/if}
              <DesignFrame frame={open.frame} title="Direction: {open.title}" sandbox={DIRECTION_SANDBOX} srcdoc={open.srcdoc} />
            </div>
          {:else if heads.length}
            <ul class="grid" aria-label="Directions">
              {#each heads as d (d.id)}<DirectionCard {d} onpick={pick} />{/each}
            </ul>
          {:else}
            <EmptyState
              title="Describe a screen"
              description="Generate 3–5 on-brand directions. Each one renders in a sandbox with no network or scripts, and is self-critiqued before you see it."
            />
          {/if}
        {:else}
          <form class="toolbar" onsubmit={startPreview} aria-label="Live preview">
            <TextField id="preview-url" label="Dev server URL" hideLabel mono bind:value={previewUrl} placeholder="http://localhost:5173 (leave empty to start the workspace's dev server)" />
            <Button variant="primary" type="submit" disabled={starting}>{starting ? "Starting…" : preview ? "Restart" : "Start"}</Button>
            {#if preview}
              <Button onclick={reload}>Reload</Button>
              <Button onclick={stopPreview}>Stop</Button>
              <Button variant={selecting ? "primary" : "secondary"} aria-pressed={selecting} onclick={() => (selecting = !selecting)} title="Click elements in the preview to inspect them">Select elements</Button>
              <Select id="preview-frame" label="Frame" hideLabel bind:value={previewFrame} options={FRAME_OPTIONS} />
            {/if}
          </form>
          {#if preview}
            <p class="muted small">Proxying {preview.target}{#if preview.command} · started <code>{preview.command}</code>{/if}. Enter selects the focused element, Escape clears.</p>
            <DesignFrame frame={previewFrame} title="Live app preview" sandbox={preview.sandbox} src={preview.url} bind:iframe />
            {#if log.length}
              <details class="log"><summary>Dev server output</summary><pre>{log.join("\n")}</pre></details>
            {/if}
          {:else}
            <EmptyState
              title="Edit the running app"
              description="Run the real app here, click an element to jump to its source, and change its style or layout. Changes preview instantly, then land in the code as a task in a worktree that you review and merge in Tasks."
            />
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
              <TextArea id="refine" label="Ask for a change" hideLabel rows={2} bind:value={chat} placeholder="Tighter header, larger type…" onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); (e.currentTarget as HTMLTextAreaElement).form?.requestSubmit(); } }} />
              <Button variant="primary" type="submit" disabled={!chat.trim() || refining}>{refining ? "Updating…" : "Send"}</Button>
            </form>
            <h2>Hand off</h2>
            <div class="row wrap">
              <Select id="framework" label="Framework" hideLabel bind:value={framework} options={FRAMEWORK_OPTIONS} />
              <Button onclick={() => handOff("convert")} disabled={handing}>Convert to code</Button>
            </div>
            <Button variant="primary" block onclick={() => handOff("implement")} disabled={handing}>Implement this</Button>
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
          <div class="row"><Button size="sm" onclick={() => post("parent")}>Select parent</Button><Button size="sm" onclick={() => { post("deselect"); clearSelection(); }}>Clear</Button></div>
          {#each ["Layout", "Spacing", "Type", "Fill"] as group (group)}
            <fieldset class="fields">
              <legend>{group}</legend>
              {#each STYLE_FIELDS.filter((f) => f.group === group) as f (f.key)}
                <label for="f-{f.key}">{f.label}</label>
                {#if f.options}
                  <Select id="f-{f.key}" size="sm" bind:value={values[f.key]} options={fieldOptions(f.options, values[f.key])} onchange={previewStyles} />
                {:else}
                  <TextField id="f-{f.key}" size="sm" list="tok-{f.key}" bind:value={values[f.key]} oninput={previewStyles} error={safeValue(values[f.key] ?? "") ? undefined : "Not allowed in a style value"} />
                  {#if files}<datalist id="tok-{f.key}">{#each tokenOptions(f, files.tokens) as o (o)}<option value={o}></option>{/each}</datalist>{/if}
                {/if}
              {/each}
            </fieldset>
          {/each}
          {#if unsafe.length}<Banner tone="danger">Not allowed in a style value: quotes, ; {"{}"} &lt;&gt; \ or url().</Banner>{/if}
          <div class="row">
            <Button size="sm" onclick={resetStyles} disabled={!changes.length}>Reset preview</Button>
            <Button size="sm" variant="primary" onclick={writeToCode} disabled={!changes.length || unsafe.length > 0 || writing || !source}>{writing ? "Starting task…" : `Write ${changes.length || ""} change${changes.length === 1 ? "" : "s"} to code`}</Button>
          </div>
          {#if editNote}<Banner tone="warn">{editNote}</Banner>{/if}
        {:else}
          <h2>Inspector</h2>
          <p class="muted">{preview ? "Click an element in the preview." : "Start the preview first."}</p>
          {#if editNote}<Banner tone="warn">{editNote}</Banner>{/if}
        {/if}

        {#if tasks.length}
          <h2>Tasks from the studio</h2>
          <List label="Tasks from the studio" bordered={false}>
            {#each tasks as t (t.taskId)}
              <ListRow title={t.title}>
                {#snippet actions()}<Button size="sm" onclick={() => review(t.taskId)}>Review diff</Button>{/snippet}
              </ListRow>
            {/each}
          </List>
        {/if}
      </aside>
    </div>
  {/if}
</div>

<Dialog
  bind:open={confirmDev}
  title="Run this workspace's dev script?"
  description="This runs the dev script from the workspace's package.json: the project's own code, on this computer, outside the agent's permission checks. Only do this for projects you trust; otherwise start the dev server yourself and enter its URL."
  confirmLabel="Run dev script"
  onconfirm={() => startPreview()}
/>

<style>
  .studio { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .bar { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-3); padding: var(--space-2) var(--space-3); border-bottom: var(--border-width) solid var(--color-border); background: var(--color-surface); }
  .bar h1 { font-size: var(--font-size-lg); }
  .ws { font-size: var(--font-size-sm); }
  .model { margin-left: auto; font-size: var(--font-size-sm); color: var(--color-text-secondary); }
  .cols { display: grid; grid-template-columns: 240px minmax(0, 1fr) 300px; flex: 1; min-height: 0; }
  .side { overflow: auto; padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); background: var(--color-surface); border-left: var(--border-width) solid var(--color-border); }
  .main { overflow: auto; padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-3); min-width: 0; }
  h2 { font-size: var(--font-size-md); }
  .small { font-size: var(--font-size-sm); margin: 0; }
  .row { display: flex; gap: var(--space-1-5); align-items: center; }
  .row.wrap { flex-wrap: wrap; }
  .between { justify-content: space-between; }
  .composer { display: grid; gap: var(--space-2); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-lg); padding: var(--space-3); }
  .grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: var(--space-3); }
  .focus { display: grid; gap: var(--space-2); }
  .editor { display: flex; flex-direction: column; gap: var(--space-2); flex: 1; min-height: 0; }
  .editor :global(.code) { flex: 1; min-height: 420px; }
  .toolbar { display: flex; flex-wrap: wrap; gap: var(--space-1-5); align-items: center; }
  .toolbar > :global(:first-child) { flex: 1; min-width: 200px; }
  .log pre { max-height: 180px; overflow: auto; font-size: var(--font-size-xs); background: var(--color-surface-sunken); padding: var(--space-2); border-radius: var(--radius-md); }
  .chat { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1-5); }
  .chat .you { justify-self: end; background: var(--color-accent); color: var(--color-on-accent); border-radius: var(--radius-lg) var(--radius-lg) var(--radius-xs) var(--radius-lg); padding: var(--space-1) var(--space-3); max-width: 90%; }
  .chat .model-reply { background: var(--color-surface-sunken); border-radius: var(--radius-lg) var(--radius-lg) var(--radius-lg) var(--radius-xs); padding: var(--space-1) var(--space-3); max-width: 90%; font-size: var(--font-size-md); }
  .ask { display: grid; gap: var(--space-1-5); }
  .source { font-size: var(--font-size-xs); background: var(--color-surface-sunken); border-radius: var(--radius-md); padding: var(--space-1-5) 0; overflow: auto; max-height: 220px; margin: 0; }
  .source span { display: block; padding: 0 var(--space-2); white-space: pre; }
  .source span.hit { background: var(--color-accent-subtle); }
  .source b { display: inline-block; width: 3ch; color: var(--color-text-secondary); font-weight: var(--font-weight-regular); margin-right: var(--space-2); text-align: right; }
  .fields { border: var(--border-width) solid var(--color-border); border-radius: var(--radius-md); padding: var(--space-1-5) var(--space-2); margin: 0; display: grid; grid-template-columns: 90px 1fr; gap: var(--space-1) var(--space-2); align-items: center; font-size: var(--font-size-sm); }
  .fields legend { font-size: var(--font-size-xs); text-transform: uppercase; letter-spacing: var(--letter-spacing-caps); color: var(--color-text-secondary); padding: 0 var(--space-1); }
  @media (max-width: 1100px) { .cols { grid-template-columns: 200px minmax(0, 1fr) 260px; } }
  @media (max-width: 760px) {
    .cols { grid-template-columns: 1fr; flex: none; }
    .side { border-left: 0; border-top: var(--border-width) solid var(--color-border); }
    .model { margin-left: 0; }
  }
</style>
