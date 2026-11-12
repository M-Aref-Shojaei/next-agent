// Typed shape for the worktree diff API (Step 4). The CodeMirror merge view is
// wired to this once the worktree crate lands; until then it is a stub.

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
  hunks: DiffHunk[];
}

export type DiffAction = "merge" | "discard" | "keep";

export interface DiffViewerProps {
  files: DiffFile[];
  /** Branch under review, e.g. `next-agent/auto-3f2a`. */
  branch?: string;
  onAction?: (action: DiffAction) => void;
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
