// The only module that imports Excalidraw (and React). Everything is loaded
// with dynamic import() so the main bundle and other routes never pay for it.
import type { ExcalidrawImperativeAPI } from "@excalidraw/excalidraw/types";
import type { ExcalidrawElement } from "@excalidraw/excalidraw/element/types";
import { mountIsland, type Island, type IslandRenderer } from "./island.ts";
import { WIREFRAME_LIBRARY } from "./library.ts";
import type { Skeleton } from "./skeleton.ts";

export type { ExcalidrawImperativeAPI };

/** Where Excalidraw finds its fonts at runtime (bundled copy, never a CDN). */
export const ASSET_PATH = "/excalidraw-assets/";

type Excalidraw = typeof import("@excalidraw/excalidraw");
let loaded: Promise<Excalidraw> | null = null;
let lib: Excalidraw | null = null;

/** Load Excalidraw once (code, CSS, fonts path). */
export function loadExcalidraw(): Promise<Excalidraw> {
  loaded ??= (async () => {
    (window as unknown as { EXCALIDRAW_ASSET_PATH: string }).EXCALIDRAW_ASSET_PATH = ASSET_PATH;
    const [l] = await Promise.all([import("@excalidraw/excalidraw"), import("@excalidraw/excalidraw/index.css")]);
    return l;
  })().then((l) => (lib = l));
  return loaded;
}

export interface BoardProps {
  initialScene: unknown;
  theme: "light" | "dark";
  onChange: (elements: readonly ExcalidrawElement[], appState: unknown, files: unknown) => void;
  onReady: (api: ExcalidrawImperativeAPI) => void;
}

/** Mount Excalidraw into `el` as a React island. */
export function mountBoard(el: HTMLElement, initial: BoardProps): Island<BoardProps> {
  return mountIsland<BoardProps>(async () => {
    const [{ Excalidraw }, React, { createRoot }] = await Promise.all([
      loadExcalidraw(),
      import("react"),
      import("react-dom/client"),
    ]);
    const root = createRoot(el);
    const renderer: IslandRenderer<BoardProps> = {
      render(props) {
        const scene = (props.initialScene ?? {}) as { elements?: unknown; appState?: object; files?: unknown };
        root.render(
          React.createElement(Excalidraw, {
            initialData: { elements: scene.elements ?? [], appState: scene.appState ?? {}, files: scene.files ?? {} } as never,
            theme: props.theme,
            excalidrawAPI: props.onReady,
            onChange: props.onChange as never,
            UIOptions: { canvasActions: { loadScene: false, saveToActiveFile: false, export: false, saveAsImage: false } },
          }),
        );
      },
      unmount() {
        root.unmount();
      },
    };
    return renderer;
  }, initial);
}

export interface SceneSnapshot {
  json: string;
  /** Elements that are not soft-deleted. */
  liveCount: number;
}

/**
 * Serialise the current scene as standard `.excalidraw` JSON, synchronously.
 * Sync on purpose: autosave must capture the scene before the canvas is torn
 * down, and an `await` in between lets React unmount and the scene read empty.
 * Only callable once a board has mounted (which implies Excalidraw is loaded).
 */
export function snapshotScene(api: ExcalidrawImperativeAPI): SceneSnapshot {
  if (!lib) throw new Error("Excalidraw is not loaded yet");
  const elements = api.getSceneElements();
  return {
    json: lib.serializeAsJSON(elements, api.getAppState(), api.getFiles(), "local"),
    liveCount: elements.filter((e) => !e.isDeleted).length,
  };
}

/** Centre of the visible canvas in scene coordinates. */
export function viewportCentre(api: ExcalidrawImperativeAPI): { x: number; y: number } {
  const s = api.getAppState();
  return { x: s.width / 2 / s.zoom.value - s.scrollX, y: s.height / 2 / s.zoom.value - s.scrollY };
}

/** Add skeleton content to the board, select it, and scroll it into view. */
export async function insertSkeletons(api: ExcalidrawImperativeAPI, skeletons: Skeleton[], fit = false): Promise<void> {
  if (skeletons.length === 0) return;
  const { convertToExcalidrawElements } = await loadExcalidraw();
  const added = convertToExcalidrawElements(skeletons as never, { regenerateIds: false });
  api.updateScene({
    elements: [...api.getSceneElements(), ...added],
    appState: { selectedElementIds: Object.fromEntries(added.map((e) => [e.id, true])) },
  });
  api.scrollToContent(added, { fitToViewport: fit, viewportZoomFactor: 0.9 });
}

/** Install our wireframe library into the library panel (replaces earlier copies of ours). */
export async function installLibrary(api: ExcalidrawImperativeAPI): Promise<void> {
  const { convertToExcalidrawElements } = await loadExcalidraw();
  const libraryItems = WIREFRAME_LIBRARY.map((item) => ({
    id: `next-agent:${item.id}`,
    status: "published" as const,
    created: 0,
    name: item.name,
    elements: convertToExcalidrawElements(item.elements as never) as never,
  }));
  await api.updateLibrary({ libraryItems: libraryItems as never, merge: true, openLibraryMenu: false });
}

/** Parse Mermaid text and add the diagram to the board. Throws on invalid Mermaid. */
export async function importMermaid(api: ExcalidrawImperativeAPI, source: string): Promise<number> {
  const text = source.trim();
  if (!text) throw new Error("Paste some Mermaid text first.");
  const [{ parseMermaidToExcalidraw }, { convertToExcalidrawElements }] = await Promise.all([
    import("@excalidraw/mermaid-to-excalidraw"),
    loadExcalidraw(),
  ]);
  const parsed = await parseMermaidToExcalidraw(text);
  const elements = convertToExcalidrawElements(parsed.elements as never, { regenerateIds: true });
  if (parsed.files) api.addFiles(Object.values(parsed.files) as never);
  const c = viewportCentre(api);
  // Centre the diagram on the current view.
  const xs = elements.map((e) => e.x);
  const ys = elements.map((e) => e.y);
  const mx = (Math.min(...xs) + Math.max(...elements.map((e) => e.x + e.width))) / 2;
  const my = (Math.min(...ys) + Math.max(...elements.map((e) => e.y + e.height))) / 2;
  const moved = elements.map((e) => ({ ...e, x: e.x + c.x - mx, y: e.y + c.y - my }));
  api.updateScene({ elements: [...api.getSceneElements(), ...moved] });
  api.scrollToContent(moved as never, { fitToViewport: true });
  return moved.length;
}

function exportInput(api: ExcalidrawImperativeAPI) {
  const elements = api.getSceneElements().filter((e) => !e.isDeleted);
  return { elements, appState: { ...api.getAppState(), exportBackground: true }, files: api.getFiles() };
}

export async function exportPngBlob(api: ExcalidrawImperativeAPI): Promise<Blob> {
  const { exportToBlob } = await loadExcalidraw();
  return exportToBlob({ ...exportInput(api), mimeType: "image/png" });
}

export async function exportSvgText(api: ExcalidrawImperativeAPI): Promise<string> {
  const { exportToSvg } = await loadExcalidraw();
  const svg = await exportToSvg(exportInput(api));
  return new XMLSerializer().serializeToString(svg);
}

export async function copyToClipboard(api: ExcalidrawImperativeAPI, type: "png" | "svg"): Promise<void> {
  const { exportToClipboard } = await loadExcalidraw();
  await exportToClipboard({ ...exportInput(api), type });
}

export function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.readAsDataURL(blob);
  });
}
