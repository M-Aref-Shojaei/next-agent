// Design Studio input: a board's live elements plus a PNG snapshot.
// Used by the future AI Design Studio ("board sketch → studio").
import { api } from "../api.ts";
import { blobToBase64, loadExcalidraw } from "./excalidraw.ts";

export interface BoardSketch {
  name: string;
  /** Live (non-deleted) Excalidraw elements, in z-order. */
  elements: unknown[];
  /** Embedded images, keyed by file id. */
  files: unknown;
  /** Base64 PNG of the whole board on a white background; null for an empty board. */
  pngBase64: string | null;
}

/** Read a board from the vault and render it. Works without the board being open. */
export async function loadBoardSketch(name: string, scale = 1): Promise<BoardSketch> {
  const data = await api.boardSketch(name);
  if (data.elements.length === 0) return { ...data, pngBase64: null };
  const { exportToBlob } = await loadExcalidraw();
  const files = (data.files ?? {}) as Record<string, never>;
  const blob = await exportToBlob({
    elements: data.elements as never,
    appState: { exportBackground: true, exportScale: scale, viewBackgroundColor: "#ffffff" } as never,
    files: files as never,
    mimeType: "image/png",
  });
  return { ...data, pngBase64: await blobToBase64(blob) };
}
