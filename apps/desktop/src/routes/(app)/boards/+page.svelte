<script lang="ts">
  import { log } from "$lib/log.ts";
  import { onMount, tick } from "svelte";
  import { api, type BoardDoc, type BoardMeta } from "$lib/api.ts";
  import { errorText } from "$lib/errors.ts";
  import { Banner, Button, Dialog, EmptyState, List, ListRow, Select, TextArea, TextField } from "$lib/ds/index.ts";
  import BoardCanvas from "$lib/components/BoardCanvas.svelte";
  import { judgeSave } from "$lib/boards/guard.ts";
  import { createAutosaver, type Autosaver, type SaveStatus } from "$lib/boards/autosave.ts";
  import { TEMPLATES } from "$lib/boards/templates.ts";
  import { STICKY_PRESETS, sticky } from "$lib/boards/stickies.ts";
  import { placeAt, reId, type Skeleton } from "$lib/boards/skeleton.ts";
  import type { ExcalidrawImperativeAPI } from "$lib/boards/excalidraw.ts";

  const TEMPLATE_OPTIONS = TEMPLATES.map((t) => ({ value: t.id, label: t.name, description: t.description }));

  type Engine = typeof import("$lib/boards/excalidraw.ts");
  type Snap = { json: string; liveCount: number };

  let hasVault = $state<boolean | null>(null);
  let boards = $state<BoardMeta[]>([]);
  let doc = $state<BoardDoc | null>(null);
  let scene = $state<unknown>(null);
  let mountKey = $state(0);
  let status = $state<SaveStatus>("idle");
  let conflict = $state<"changed" | "deleted" | null>(null);
  /** An empty scene is being held back from overwriting a non-empty board. */
  let emptyHeld = $state(false);
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
  let saver: Autosaver<Snap | null> | null = null;
  let revision: string | undefined;
  let lastSignature = "";
  /** The board on disk has elements; an empty scene needs an explicit OK to replace it. */
  let diskHasContent = false;
  let allowEmpty = false;
  let pendingTemplate: Skeleton[] | null = null;
  let stickyCount = 0;
  /** Bumped whenever a canvas unmounts, so a late onReady from a dead canvas is ignored. */
  let generation = 0;

  const fail = (e: unknown) => {
    log.warn("ui_error", { kind: "boards" });
    problem = errorText(e);
  };

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
      void saver?.flush(); // no-op once the canvas teardown already flushed
    };
  });

  /**
   * Snapshot of the scene for autosave, or null when there is nothing safe to
   * save. Runs synchronously (see `snapshotScene`): null once the canvas is gone,
   * and null for an empty scene over a non-empty file unless the user said so.
   */
  function snapshot(): Snap | null {
    if (!boardApi || !engine) return null;
    const snap = engine.snapshotScene(boardApi);
    const verdict = judgeSave(snap.liveCount, { diskHasContent, allowEmpty });
    emptyHeld = verdict === "empty-held";
    return verdict === "save" ? snap : null;
  }

  function scheduleSave() {
    saver?.schedule(snapshot);
  }

  function newSaver(name: string): Autosaver<Snap | null> {
    return createAutosaver<Snap | null>({
      delayMs: 800,
      onStatus: (s, e) => {
        status = s;
        if (s === "error") {
          if (String(e).startsWith("conflict:")) {
            conflict = String(e).startsWith("conflict: deleted") ? "deleted" : "changed";
            saver?.cancel();
          } else fail(e);
        }
      },
      save: async (snap) => {
        if (snap === null || conflict) return;
        revision = await api.boardSave(name, snap.json, revision);
        diskHasContent = snap.liveCount > 0;
      },
    });
  }

  async function open(name: string, template: Skeleton[] | null = null): Promise<boolean> {
    problem = "";
    notice = "";
    pendingTemplate = null; // never carry a template over from an earlier, failed open
    try {
      await saver?.flush();
      const opened = await api.boardOpen(name);
      saver = newSaver(opened.name);
      revision = opened.revision;
      conflict = null;
      emptyHeld = false;
      allowEmpty = false;
      confirmDelete = false;
      renaming = false;
      boardApi = null;
      status = "idle";
      const parsed = JSON.parse(opened.content) as { elements?: { version: number; isDeleted?: boolean }[]; appState?: AppBits };
      lastSignature = signature(parsed.elements ?? [], parsed.appState);
      diskHasContent = (parsed.elements ?? []).some((e) => !e.isDeleted);
      pendingTemplate = template;
      scene = parsed;
      doc = opened;
      mountKey += 1;
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    problem = "";
    const template = TEMPLATES.find((t) => t.id === templateId);
    try {
      const created = await api.boardCreate(newName);
      newName = "";
      await refresh();
      await open(created.name, template && template.elements.length > 0 ? template.elements : null);
    } catch (e) {
      fail(e);
    }
  }

  interface AppBits {
    viewBackgroundColor?: string;
    gridModeEnabled?: boolean;
  }

  /** What makes a scene worth saving: elements (count + versions) and the saved appState bits. */
  function signature(elements: readonly { version: number }[], app: AppBits | undefined): string {
    let sum = 0;
    for (const e of elements) sum += e.version;
    return `${elements.length}:${sum}:${app?.viewBackgroundColor ?? "#ffffff"}:${app?.gridModeEnabled ?? false}`;
  }

  function onChange(elements: readonly { version: number }[], app: unknown) {
    const sig = signature(elements, app as AppBits);
    if (sig === lastSignature) return; // scroll/zoom/selection only
    lastSignature = sig;
    if (!conflict) scheduleSave();
  }

  async function onReady(boardApiNext: ExcalidrawImperativeAPI) {
    const gen = generation;
    const e = await loadEngine();
    if (gen !== generation) return; // the board was switched or closed while loading
    boardApi = boardApiNext;
    void e.installLibrary(boardApiNext).catch(fail);
    if (pendingTemplate) {
      const centre = e.viewportCentre(boardApiNext);
      const items = placeAt(reId(pendingTemplate, "t-"), centre.x, centre.y);
      pendingTemplate = null;
      await e.insertSkeletons(boardApiNext, items, true);
      scheduleSave();
    }
  }

  async function addSticky(color: string) {
    if (!boardApi) return;
    try {
      const e = await loadEngine();
      stickyCount += 1;
      const c = e.viewportCentre(boardApi);
      const jitter = ((stickyCount - 1) % 5) * 24;
      await e.insertSkeletons(boardApi, [sticky(color, "Idea", c.x - 90 + jitter, c.y - 90 + jitter)]);
      scheduleSave();
    } catch (e) {
      fail(e);
    }
  }

  async function addFrame() {
    if (!boardApi) return;
    const e = await loadEngine();
    const c = e.viewportCentre(boardApi);
    await e.insertSkeletons(boardApi, [{ type: "frame", x: c.x - 320, y: c.y - 220, width: 640, height: 440, name: "Frame" }]);
    scheduleSave();
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
      scheduleSave();
    } catch (err) {
      // Mermaid errors can quote the diagram, so only the error's type is logged.
      log.warn("ui_error", { kind: "boards", code: err instanceof Error ? err.name : "unknown" });
      problem = `Mermaid import failed: ${err instanceof Error ? err.message : String(err)}`;
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
      diskHasContent = JSON.parse(reopened.content).elements?.some((e: { isDeleted?: boolean }) => !e.isDeleted) ?? false;
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
      await saver?.flush(); // let an in-flight save finish so it cannot recreate the board
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

  /** Canvas is about to unmount: capture any pending save now, then refuse further ones. */
  function teardown() {
    void saver?.flush();
    boardApi = null;
    generation += 1;
  }

  /** The user confirmed saving an empty board over a non-empty one. */
  function saveEmpty() {
    allowEmpty = true;
    emptyHeld = false;
    scheduleSave();
  }

  async function reloadFromDisk() {
    if (doc) await open(doc.name);
  }

  async function overwriteDisk() {
    if (!doc || !boardApi || !engine) return;
    try {
      const { json, liveCount } = engine.snapshotScene(boardApi);
      revision = await api.boardSave(doc.name, json, undefined);
      diskHasContent = liveCount > 0;
      conflict = null;
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
    conflict ? "Changed on disk" : emptyHeld ? "Not saved (board is empty)" : { idle: "", dirty: "Unsaved changes…", saving: "Saving…", saved: "Saved", error: "Save failed" }[status],
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
        <TextField id="board-name" label="New board name" hideLabel placeholder="New board name" bind:value={newName} required />
        <Select id="board-template" label="Template" hideLabel bind:value={templateId} options={TEMPLATE_OPTIONS} />
        <Button type="submit" variant="primary">Create</Button>
      </form>
      <List label="Board list" bordered={false}>
        {#each boards as b (b.name)}
          <ListRow title={b.name} selected={doc?.name === b.name} onclick={() => open(b.name)} />
        {:else}
          <p class="muted">No boards yet</p>
        {/each}
      </List>
    {/if}
  </aside>

  <section class="stage" aria-label="Board editor">
    {#if problem}<Banner tone="danger" ondismiss={() => (problem = "")}>{problem}</Banner>{/if}
    {#if notice}<Banner tone="info">{notice}</Banner>{/if}
    {#if doc}
      <div class="toolbar" role="toolbar" aria-label="Board tools">
        {#if renaming}
          <form onsubmit={rename} class="inline">
            <TextField id="rename" label="Board name" hideLabel size="sm" bind:value={renameTo} required />
            <Button type="submit" size="sm" variant="primary">Rename</Button>
            <Button size="sm" onclick={() => (renaming = false)}>Cancel</Button>
          </form>
        {:else}
          <strong class="title">{doc.name}</strong>
          <Button size="sm" onclick={() => { renaming = true; renameTo = doc?.name ?? ""; }} aria-label="Rename board">Rename</Button>
        {/if}
        <span class="sep" aria-hidden="true"></span>
        <span class="group" role="group" aria-label="Sticky notes">
          {#each STICKY_PRESETS as p (p.id)}
            <button type="button" class="swatch" style:background={p.color} aria-label={`Add ${p.label.toLowerCase()} sticky note`} title={`${p.label} sticky note (Alt+N adds yellow)`} onclick={() => addSticky(p.color)}></button>
          {/each}
        </span>
        <Button size="sm" onclick={addFrame} aria-label="Add frame">Frame</Button>
        <Button size="sm" onclick={() => (mermaidOpen = !mermaidOpen)} aria-expanded={mermaidOpen} aria-label="Import Mermaid diagram" title="Alt+M">Mermaid</Button>
        <span class="sep" aria-hidden="true"></span>
        <Button size="sm" disabled={busy} onclick={() => exportAs("png")} aria-label="Export PNG to vault assets">PNG</Button>
        <Button size="sm" disabled={busy} onclick={() => exportAs("svg")} aria-label="Export SVG to vault assets">SVG</Button>
        <Button size="sm" onclick={() => copy("png")} aria-label="Copy as PNG to clipboard">Copy PNG</Button>
        <Button size="sm" onclick={() => copy("svg")} aria-label="Copy as SVG to clipboard">Copy SVG</Button>
        <span class="sep" aria-hidden="true"></span>
        <Button size="sm" variant="danger" onclick={() => (confirmDelete = true)} aria-label="Delete board (moves to trash)">Delete</Button>
        <span class="status" role="status" aria-live="polite">{statusText}</span>
      </div>
      {#if emptyHeld}
        <Banner tone="warn">
          The board is empty, so autosave is paused to protect what is saved on disk.
          {#snippet actions()}<Button size="sm" variant="danger" onclick={saveEmpty}>Save empty board</Button>{/snippet}
        </Banner>
      {/if}
      {#if conflict}
        <Banner tone="danger">
          {conflict === "deleted" ? "This board was deleted or moved on disk while it was open." : "This board changed on disk while it was open."}
          {#snippet actions()}
            <Button size="sm" onclick={reloadFromDisk}>Reload from disk</Button>
            <Button size="sm" variant="danger" onclick={overwriteDisk}>Keep mine (overwrite)</Button>
          {/snippet}
        </Banner>
      {/if}
      {#if mermaidOpen}
        <form class="mermaid" onsubmit={importMermaid}>
          <TextArea id="mermaid-text" label="Mermaid diagram text" rows={6} mono bind:value={mermaidText} spellcheck={false} />
          <div class="row">
            <Button type="submit" variant="primary" disabled={busy}>Add to board</Button>
            <Button onclick={() => (mermaidOpen = false)}>Close</Button>
          </div>
        </form>
      {/if}
      <div class="canvas">
        {#key mountKey}
          <BoardCanvas {scene} onchange={onChange} onready={onReady} onerror={fail} onteardown={teardown} />
        {/key}
      </div>
    {:else if hasVault}
      <EmptyState
        title="Design board"
        description="Pick a board on the left or create one from a template. Boards are saved in your vault as boards/<name>.excalidraw."
      />
    {/if}
  </section>
</div>

<Dialog
  bind:open={confirmDelete}
  title="Move board to trash?"
  description={doc ? `"${doc.name}" moves to the vault trash.` : ""}
  confirmLabel="Move to trash"
  cancelLabel="Keep"
  tone="danger"
  onconfirm={remove}
/>

<style>
  .page { display: grid; grid-template-columns: 220px 1fr; flex: 1; min-height: 0; }
  .list { border-right: var(--border-width) solid var(--color-border); padding: var(--space-3); overflow: auto; display: flex; flex-direction: column; gap: var(--space-2); }
  h2 { font-size: var(--font-size-xs); text-transform: uppercase; letter-spacing: var(--letter-spacing-caps); color: var(--color-text-secondary); }
  .new { display: grid; gap: var(--space-1-5); }
  .stage { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-1-5); padding: var(--space-1-5) var(--space-3); border-bottom: var(--border-width) solid var(--color-border); }
  .title { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sep { width: var(--border-width); align-self: stretch; background: var(--color-border); margin: 0 var(--space-1); }
  .group { display: inline-flex; gap: var(--space-1); }
  .swatch { width: var(--control-height-sm); height: var(--control-height-sm); padding: 0; border-radius: var(--radius-sm); border: var(--border-width) solid var(--color-border-strong); }
  .swatch:focus-visible { box-shadow: var(--focus-ring); outline: none; }
  .inline { display: flex; gap: var(--space-1-5); align-items: center; }
  .status { margin-left: auto; color: var(--color-text-secondary); font-size: var(--font-size-sm); min-width: 90px; text-align: right; }
  .canvas { flex: 1; min-height: 0; position: relative; }
  .mermaid { display: grid; gap: var(--space-1-5); padding: var(--space-3); border-bottom: var(--border-width) solid var(--color-border); }
  .row { display: flex; gap: var(--space-1-5); }
  @media (max-width: 640px) {
    .page { grid-template-columns: 1fr; grid-template-rows: auto 1fr; }
    .list { border-right: 0; border-bottom: var(--border-width) solid var(--color-border); max-height: 40vh; }
    .status { margin-left: 0; text-align: left; }
  }
</style>
