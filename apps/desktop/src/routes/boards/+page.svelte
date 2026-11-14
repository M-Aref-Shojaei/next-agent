<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, type BoardDoc, type BoardMeta } from "$lib/api.ts";
  import BoardCanvas from "$lib/components/BoardCanvas.svelte";
  import { createAutosaver, type Autosaver, type SaveStatus } from "$lib/boards/autosave.ts";
  import { TEMPLATES } from "$lib/boards/templates.ts";
  import { STICKY_PRESETS, sticky } from "$lib/boards/stickies.ts";
  import { placeAt, reId, type Skeleton } from "$lib/boards/skeleton.ts";
  import type { ExcalidrawImperativeAPI } from "$lib/boards/excalidraw.ts";

  type Engine = typeof import("$lib/boards/excalidraw.ts");

  let hasVault = $state<boolean | null>(null);
  let boards = $state<BoardMeta[]>([]);
  let doc = $state<BoardDoc | null>(null);
  let scene = $state<unknown>(null);
  let mountKey = $state(0);
  let status = $state<SaveStatus>("idle");
  let conflict = $state(false);
  let notice = $state("");
  let problem = $state("");
  let newName = $state("");
  let templateId = $state("blank");
  let renaming = $state(false);
  let renameTo = $state("");
  let confirmDelete = $state(false);
  let mermaidOpen = $state(false);
  let mermaidText = $state("graph TD\n  A[Start] --> B{Ready?}\n  B -->|yes| C[Ship]\n  B -->|no| D[Fix]\n  D --> B");
  let busy = $state(false);

  let boardApi: ExcalidrawImperativeAPI | null = null;
  let engine: Engine | null = null;
  let saver: Autosaver<true> | null = null;
  let revision: string | undefined;
  let lastSignature = "";
  let pendingTemplate: Skeleton[] | null = null;
  let stickyCount = 0;

  const fail = (e: unknown) => (problem = String(e instanceof Error ? e.message : e));

  async function loadEngine(): Promise<Engine> {
    engine ??= await import("$lib/boards/excalidraw.ts");
    return engine;
  }

  async function refresh() {
    try {
      boards = await api.boardList();
    } catch (e) {
      fail(e);
    }
  }

  onMount(() => {
    void (async () => {
      hasVault = (await api.vaultGet()) !== null;
      if (hasVault) await refresh();
    })();
    const leave = () => void saver?.flush();
    window.addEventListener("beforeunload", leave);
    return () => {
      window.removeEventListener("beforeunload", leave);
      void saver?.flush();
    };
  });

  function newSaver(name: string): Autosaver<true> {
    return createAutosaver<true>({
      delayMs: 800,
      onStatus: (s, e) => {
        status = s;
        if (s === "error") {
          if (String(e).startsWith("conflict:")) {
            conflict = true;
            saver?.cancel();
          } else fail(e);
        }
      },
      save: async () => {
        if (!boardApi || !engine || conflict) return;
        const content = await engine.serializeScene(boardApi);
        revision = await api.boardSave(name, content, revision);
      },
    });
  }

  async function open(name: string) {
    problem = "";
    notice = "";
    try {
      await saver?.flush();
      const opened = await api.boardOpen(name);
      saver = newSaver(opened.name);
      revision = opened.revision;
      conflict = false;
      confirmDelete = false;
      renaming = false;
      boardApi = null;
      lastSignature = "";
      status = "idle";
      scene = JSON.parse(opened.content);
      doc = opened;
      mountKey += 1;
    } catch (e) {
      fail(e);
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    problem = "";
    const template = TEMPLATES.find((t) => t.id === templateId);
    try {
      const created = await api.boardCreate(newName);
      newName = "";
      pendingTemplate = template && template.elements.length > 0 ? template.elements : null;
      await refresh();
      await open(created.name);
    } catch (e) {
      fail(e);
    }
  }

  function signature(elements: readonly { version: number; id: string }[]): string {
    let sum = 0;
    for (const e of elements) sum += e.version;
    return `${elements.length}:${sum}`;
  }

  function onChange(elements: readonly { version: number; id: string }[]) {
    const sig = signature(elements);
    if (sig === lastSignature) return; // scroll/zoom/selection only
    const first = lastSignature === "";
    lastSignature = sig;
    if (first || conflict) return;
    saver?.schedule(true);
  }

  async function onReady(boardApiNext: ExcalidrawImperativeAPI) {
    boardApi = boardApiNext;
    const e = await loadEngine();
    void e.installLibrary(boardApiNext).catch(fail);
    if (pendingTemplate) {
      const centre = e.viewportCentre(boardApiNext);
      const items = placeAt(reId(pendingTemplate, "t-"), centre.x, centre.y);
      pendingTemplate = null;
      await e.insertSkeletons(boardApiNext, items, true);
      saver?.schedule(true);
    }
  }

  async function addSticky(color: string) {
    if (!boardApi) return;
    const e = await loadEngine();
    stickyCount += 1;
    const c = e.viewportCentre(boardApi);
    const jitter = ((stickyCount - 1) % 5) * 24;
    await e.insertSkeletons(boardApi, [sticky(color, "Idea", c.x - 90 + jitter, c.y - 90 + jitter)]);
    saver?.schedule(true);
  }

  async function addFrame() {
    if (!boardApi) return;
    const e = await loadEngine();
    const c = e.viewportCentre(boardApi);
    await e.insertSkeletons(boardApi, [{ type: "frame", x: c.x - 320, y: c.y - 220, width: 640, height: 440, name: "Frame" }]);
    saver?.schedule(true);
  }

  async function importMermaid(event: SubmitEvent) {
    event.preventDefault();
    if (!boardApi) return;
    problem = "";
    busy = true;
    try {
      const e = await loadEngine();
      const n = await e.importMermaid(boardApi, mermaidText);
      notice = `Imported ${n} elements from Mermaid.`;
      mermaidOpen = false;
      saver?.schedule(true);
    } catch (err) {
      fail(`Mermaid import failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      busy = false;
    }
  }

  async function exportAs(kind: "png" | "svg") {
    if (!boardApi || !doc) return;
    problem = "";
    busy = true;
    try {
      const e = await loadEngine();
      const data = kind === "png" ? await e.blobToBase64(await e.exportPngBlob(boardApi)) : await e.exportSvgText(boardApi);
      const path = await api.boardExport(doc.name, kind, data);
      notice = `Exported ${kind.toUpperCase()} to ${path}`;
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  async function copy(kind: "png" | "svg") {
    if (!boardApi) return;
    problem = "";
    try {
      const e = await loadEngine();
      await e.copyToClipboard(boardApi, kind);
      notice = `Copied ${kind.toUpperCase()} to the clipboard.`;
    } catch (err) {
      fail(`Copy failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    if (!doc) return;
    problem = "";
    try {
      await saver?.flush();
      const meta = await api.boardRename(doc.name, renameTo);
      const reopened = await api.boardOpen(meta.name);
      saver = newSaver(reopened.name);
      revision = reopened.revision;
      doc = { ...doc, name: reopened.name };
      renaming = false;
      await refresh();
    } catch (e) {
      fail(e);
    }
  }

  async function remove() {
    if (!doc) return;
    problem = "";
    try {
      saver?.cancel();
      const trashed = await api.boardDelete(doc.name);
      notice = `Moved "${doc.name}" to ${trashed}`;
      doc = null;
      scene = null;
      boardApi = null;
      saver = null;
      confirmDelete = false;
      await refresh();
    } catch (e) {
      fail(e);
    }
  }

  async function reloadFromDisk() {
    if (doc) await open(doc.name);
  }

  async function overwriteDisk() {
    if (!doc || !boardApi || !engine) return;
    try {
      revision = await api.boardSave(doc.name, await engine.serializeScene(boardApi), undefined);
      conflict = false;
      status = "saved";
    } catch (e) {
      fail(e);
    }
  }

  async function onkeydown(event: KeyboardEvent) {
    if (!doc || !event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.code === "KeyN") {
      event.preventDefault();
      await addSticky(STICKY_PRESETS[0]!.color);
    } else if (event.code === "KeyM") {
      event.preventDefault();
      mermaidOpen = true;
      await tick();
      document.getElementById("mermaid-text")?.focus();
    }
  }

  const statusText = $derived(
    conflict ? "Changed on disk" : { idle: "", dirty: "Unsaved changes…", saving: "Saving…", saved: "Saved", error: "Save failed" }[status],
  );
</script>

<svelte:window {onkeydown} />

<div class="page">
  <aside class="list" aria-label="Boards">
    <h2>Boards</h2>
    {#if hasVault === false}
      <p class="muted">Set a vault folder in <a href="/settings">Settings</a> to keep boards in your vault.</p>
    {:else if hasVault}
      <form onsubmit={create} class="new">
        <label class="sr-only" for="board-name">New board name</label>
        <input id="board-name" placeholder="New board name" bind:value={newName} required />
        <label class="sr-only" for="board-template">Template</label>
        <select id="board-template" bind:value={templateId}>
          {#each TEMPLATES as t (t.id)}<option value={t.id} title={t.description}>{t.name}</option>{/each}
        </select>
        <button type="submit" class="primary">Create</button>
      </form>
      <ul aria-label="Board list">
        {#each boards as b (b.name)}
          <li>
            <button aria-current={doc?.name === b.name} onclick={() => open(b.name)} title={b.name}>{b.name}</button>
          </li>
        {:else}
          <li class="muted">No boards yet</li>
        {/each}
      </ul>
    {/if}
  </aside>

  <section class="stage" aria-label="Board editor">
    {#if problem}<div class="problem" role="alert">{problem}</div>{/if}
    {#if notice}<div class="notice" role="status">{notice}</div>{/if}
    {#if doc}
      <div class="toolbar" role="toolbar" aria-label="Board tools">
        {#if renaming}
          <form onsubmit={rename} class="inline">
            <label class="sr-only" for="rename">Board name</label>
            <input id="rename" bind:value={renameTo} required />
            <button type="submit" class="primary">Rename</button>
            <button type="button" onclick={() => (renaming = false)}>Cancel</button>
          </form>
        {:else}
          <strong class="title">{doc.name}</strong>
          <button onclick={() => { renaming = true; renameTo = doc?.name ?? ""; }} aria-label="Rename board">Rename</button>
        {/if}
        <span class="sep" aria-hidden="true"></span>
        <span class="group" role="group" aria-label="Sticky notes">
          {#each STICKY_PRESETS as p (p.id)}
            <button class="swatch" style:background={p.color} aria-label={`Add ${p.label.toLowerCase()} sticky note`} title={`${p.label} sticky note (Alt+N adds yellow)`} onclick={() => addSticky(p.color)}></button>
          {/each}
        </span>
        <button onclick={addFrame} aria-label="Add frame">Frame</button>
        <button onclick={() => (mermaidOpen = !mermaidOpen)} aria-expanded={mermaidOpen} aria-label="Import Mermaid diagram" title="Alt+M">Mermaid</button>
        <span class="sep" aria-hidden="true"></span>
        <button disabled={busy} onclick={() => exportAs("png")} aria-label="Export PNG to vault assets">PNG</button>
        <button disabled={busy} onclick={() => exportAs("svg")} aria-label="Export SVG to vault assets">SVG</button>
        <button onclick={() => copy("png")} aria-label="Copy as PNG to clipboard">Copy PNG</button>
        <button onclick={() => copy("svg")} aria-label="Copy as SVG to clipboard">Copy SVG</button>
        <span class="sep" aria-hidden="true"></span>
        {#if confirmDelete}
          <button class="danger" onclick={remove} aria-label="Confirm: move board to trash">Move to trash?</button>
          <button onclick={() => (confirmDelete = false)}>Keep</button>
        {:else}
          <button class="danger" onclick={() => (confirmDelete = true)} aria-label="Delete board (moves to trash)">Delete</button>
        {/if}
        <span class="status" role="status" aria-live="polite">{statusText}</span>
      </div>
      {#if conflict}
        <div class="problem" role="alert">
          This board changed on disk while it was open.
          <button onclick={reloadFromDisk}>Reload from disk</button>
          <button class="danger" onclick={overwriteDisk}>Keep mine (overwrite)</button>
        </div>
      {/if}
      {#if mermaidOpen}
        <form class="mermaid" onsubmit={importMermaid}>
          <label for="mermaid-text">Mermaid diagram text</label>
          <textarea id="mermaid-text" rows="6" bind:value={mermaidText} spellcheck="false"></textarea>
          <div class="row">
            <button type="submit" class="primary" disabled={busy}>Add to board</button>
            <button type="button" onclick={() => (mermaidOpen = false)}>Close</button>
          </div>
        </form>
      {/if}
      <div class="canvas">
        {#key mountKey}
          <BoardCanvas {scene} onchange={onChange} onready={onReady} onerror={fail} />
        {/key}
      </div>
    {:else if hasVault}
      <div class="empty">
        <h1>Design board</h1>
        <p class="muted">Pick a board on the left or create one from a template. Boards are saved in your vault as <code>boards/&lt;name&gt;.excalidraw</code>.</p>
      </div>
    {/if}
  </section>
</div>

<style>
  .page { display: grid; grid-template-columns: 220px 1fr; flex: 1; min-height: 0; }
  .list { border-right: 1px solid var(--border); padding: 12px; overflow: auto; display: flex; flex-direction: column; gap: 8px; }
  h2 { font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); }
  .new { display: grid; gap: 6px; }
  select { font: inherit; color: inherit; background: var(--panel); border: 1px solid var(--border); border-radius: 6px; padding: 5px 8px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  li button { width: 100%; text-align: left; border-color: transparent; background: transparent; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li button[aria-current="true"] { background: var(--panel); border-color: var(--border); font-weight: 600; }
  .stage { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 6px 10px; border-bottom: 1px solid var(--border); }
  .title { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sep { width: 1px; align-self: stretch; background: var(--border); margin: 0 4px; }
  .group { display: inline-flex; gap: 4px; }
  .swatch { width: 24px; height: 24px; padding: 0; border-radius: 4px; border: 1px solid var(--border); }
  .inline { display: flex; gap: 6px; align-items: center; }
  .inline input { width: 180px; }
  .status { margin-left: auto; color: var(--muted); font-size: 12px; min-width: 90px; text-align: right; }
  .canvas { flex: 1; min-height: 0; position: relative; }
  .problem { background: var(--danger); color: #fff; padding: 6px 12px; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .problem button { color: var(--text); }
  .notice { background: var(--panel); border-bottom: 1px solid var(--border); padding: 6px 12px; }
  .mermaid { display: grid; gap: 6px; padding: 10px; border-bottom: 1px solid var(--border); }
  .mermaid textarea { font-family: ui-monospace, Menlo, monospace; }
  .row { display: flex; gap: 6px; }
  .empty { padding: 32px; max-width: 520px; display: grid; gap: 8px; }
</style>
