// Typed shape for the worktree diff API (Step 4) plus pure helpers for the
// side-by-side reviewer.

export interface DiffLine {
  kind: "context" | "add" | "remove";
  text: string;
}

export interface DiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
}

export type DiffFileStatus = "added" | "modified" | "deleted" | "renamed";

export interface DiffFile {
  path: string;
  oldPath?: string;
  status: DiffFileStatus;
  binary?: boolean;
  hunks: DiffHunk[];
}

export type DiffAction = "merge" | "discard" | "keep";

export interface ReviewDecision {
  action: DiffAction;
  label: string;
}

export interface DiffViewerProps {
  files: DiffFile[];
  /** Branch under review, e.g. `next-agent/auto-3f2a`. */
  branch?: string;
  onAction?: (action: DiffAction) => void | Promise<void>;
  decision?: ReviewDecision;
  busy?: boolean;
}

export interface SideCell {
  line: number;
  text: string;
  kind: DiffLine["kind"];
}
/** One row of the side-by-side view; a missing side renders as blank. */
export interface SideRow {
  left?: SideCell;
  right?: SideCell;
}

/** Pair a hunk's lines into left (old) / right (new) rows. Removed and added
 * runs are zipped so a modified line sits next to its replacement. */
export function sideBySide(hunk: DiffHunk): SideRow[] {
  const rows: SideRow[] = [];
  let oldLine = hunk.oldStart;
  let newLine = hunk.newStart;
  let removed: SideCell[] = [];
  let added: SideCell[] = [];
  const flush = () => {
    const n = Math.max(removed.length, added.length);
    for (let i = 0; i < n; i += 1) {
      const row: SideRow = {};
      if (removed[i]) row.left = removed[i];
      if (added[i]) row.right = added[i];
      rows.push(row);
    }
    removed = [];
    added = [];
  };
  for (const line of hunk.lines) {
    if (line.kind === "remove") {
      removed.push({ line: oldLine++, text: line.text, kind: "remove" });
    } else if (line.kind === "add") {
      added.push({ line: newLine++, text: line.text, kind: "add" });
    } else {
      flush();
      rows.push({
        left: { line: oldLine++, text: line.text, kind: "context" },
        right: { line: newLine++, text: line.text, kind: "context" },
      });
    }
  }
  flush();
  return rows;
}

export function diffStats(files: DiffFile[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const file of files)
    for (const hunk of file.hunks)
      for (const line of hunk.lines) {
        if (line.kind === "add") added += 1;
        else if (line.kind === "remove") removed += 1;
      }
  return { added, removed };
}
