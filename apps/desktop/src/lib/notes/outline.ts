// Behaviour follows Logseq (AGPL-3.0): outliner operations — frontend/modules/outliner/
// (indent-outdent, move up/down, merge on backspace, new block as first child of an
// expanded parent, outdent adopts the following siblings).
//
// The tree is immutable: every operation returns a new tree (unchanged subtrees are
// shared) or `null` when it does nothing. Blocks have stable ids that never reach disk.

import { editableText, isCollapsed, setCollapsed, withEditedText } from "./content.ts";
import type { BlockNode } from "./types.ts";

export interface OBlock {
  id: string;
  content: string;
  children: OBlock[];
}
export type Tree = readonly OBlock[];

let counter = 0;
export const newId = (): string => `b${++counter}`;

export function fromNodes(nodes: readonly BlockNode[]): OBlock[] {
  return nodes.map((n) => ({ id: newId(), content: n.content, children: fromNodes(n.children) }));
}

/** What `graph_save_page` reads: only `content` and `children`. */
export function toNodes(tree: Tree): BlockNode[] {
  return tree.map((b) => ({ content: b.content, children: toNodes(b.children) }));
}

export const emptyBlock = (content = ""): OBlock => ({ id: newId(), content, children: [] });

export interface Location {
  /** Path of indexes from the root. */
  path: number[];
  block: OBlock;
}

export function locate(tree: Tree, id: string): Location | null {
  const walk = (blocks: Tree, prefix: number[]): Location | null => {
    for (const [i, block] of blocks.entries()) {
      if (block.id === id) return { path: [...prefix, i], block };
      const hit = walk(block.children, [...prefix, i]);
      if (hit) return hit;
    }
    return null;
  };
  return walk(tree, []);
}

/** Replace the list at `path` (the parent's children; `[]` is the root) using `fn`. */
function editList(tree: Tree, parentPath: number[], fn: (list: OBlock[]) => OBlock[]): OBlock[] {
  if (parentPath.length === 0) return fn([...tree]);
  const [head, ...rest] = parentPath as [number, ...number[]];
  return tree.map((b, i) => (i === head ? { ...b, children: editList(b.children, rest, fn) } : b));
}

function mapBlock(tree: Tree, id: string, fn: (b: OBlock) => OBlock): OBlock[] {
  return tree.map((b) => (b.id === id ? fn(b) : b.children.length ? { ...b, children: mapBlock(b.children, id, fn) } : b));
}

/** Blocks in reading order that are on screen (children of collapsed blocks are not). */
export function visibleIds(tree: Tree): string[] {
  const out: string[] = [];
  const walk = (blocks: Tree) => {
    for (const b of blocks) {
      out.push(b.id);
      if (!isCollapsed(b.content)) walk(b.children);
    }
  };
  walk(tree);
  return out;
}

export function setText(tree: Tree, id: string, text: string): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc) return null;
  const content = withEditedText(loc.block.content, text);
  return content === loc.block.content ? null : mapBlock(tree, id, (b) => ({ ...b, content }));
}

export function setContent(tree: Tree, id: string, content: string): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc || loc.block.content === content) return null;
  return mapBlock(tree, id, (b) => ({ ...b, content }));
}

export function toggleCollapsed(tree: Tree, id: string, collapsed?: boolean): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc || loc.block.children.length === 0) return null;
  const next = collapsed ?? !isCollapsed(loc.block.content);
  return setContent(tree, id, setCollapsed(loc.block.content, next));
}

/**
 * Enter: the text before the caret stays, the text after it starts a new block —
 * the first child when the block is expanded and has children, else the next sibling.
 */
export function split(tree: Tree, id: string, caret: number): { tree: OBlock[]; focus: string } | null {
  const loc = locate(tree, id);
  if (!loc) return null;
  const text = editableText(loc.block.content);
  const created = emptyBlock(text.slice(caret));
  const kept = withEditedText(loc.block.content, text.slice(0, caret));
  const asChild = loc.block.children.length > 0 && !isCollapsed(loc.block.content);
  const parentPath = loc.path.slice(0, -1);
  const index = loc.path[loc.path.length - 1]!;
  const next = editList(tree, parentPath, (list) => {
    if (asChild) {
      list[index] = { ...loc.block, content: kept, children: [created, ...loc.block.children] };
    } else {
      list[index] = { ...loc.block, content: kept };
      list.splice(index + 1, 0, created);
    }
    return list;
  });
  return { tree: next, focus: created.id };
}

/** Tab: become the last child of the previous sibling. */
export function indent(tree: Tree, id: string): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc) return null;
  const index = loc.path[loc.path.length - 1]!;
  if (index === 0) return null;
  return editList(tree, loc.path.slice(0, -1), (list) => {
    const prev = list[index - 1]!;
    const content = isCollapsed(prev.content) ? setCollapsed(prev.content, false) : prev.content;
    list[index - 1] = { ...prev, content, children: [...prev.children, loc.block] };
    list.splice(index, 1);
    return list;
  });
}

/** Shift+Tab: become the next sibling of the parent; later siblings become this block's children. */
export function outdent(tree: Tree, id: string): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc || loc.path.length < 2) return null;
  const parentPath = loc.path.slice(0, -1);
  const index = loc.path[loc.path.length - 1]!;
  const grandPath = parentPath.slice(0, -1);
  const parentIndex = parentPath[parentPath.length - 1]!;
  return editList(tree, grandPath, (list) => {
    const parent = list[parentIndex]!;
    const after = parent.children.slice(index + 1);
    const moved: OBlock = { ...loc.block, children: [...loc.block.children, ...after] };
    list[parentIndex] = { ...parent, children: parent.children.slice(0, index) };
    list.splice(parentIndex + 1, 0, moved);
    return list;
  });
}

/** Alt+Up: swap with the previous sibling, or hop before the parent. */
export function moveUp(tree: Tree, id: string): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc) return null;
  const parentPath = loc.path.slice(0, -1);
  const index = loc.path[loc.path.length - 1]!;
  if (index > 0) {
    return editList(tree, parentPath, (list) => {
      [list[index - 1], list[index]] = [list[index]!, list[index - 1]!];
      return list;
    });
  }
  if (parentPath.length === 0) return null;
  const grandPath = parentPath.slice(0, -1);
  const parentIndex = parentPath[parentPath.length - 1]!;
  return editList(tree, grandPath, (list) => {
    const parent = list[parentIndex]!;
    list[parentIndex] = { ...parent, children: parent.children.slice(1) };
    list.splice(parentIndex, 0, loc.block);
    return list;
  });
}

/** Alt+Down: swap with the next sibling, or hop after the parent. */
export function moveDown(tree: Tree, id: string): OBlock[] | null {
  const loc = locate(tree, id);
  if (!loc) return null;
  const parentPath = loc.path.slice(0, -1);
  const index = loc.path[loc.path.length - 1]!;
  const siblings = parentPath.length === 0 ? tree : locateByPath(tree, parentPath)!.children;
  if (index < siblings.length - 1) {
    return editList(tree, parentPath, (list) => {
      [list[index], list[index + 1]] = [list[index + 1]!, list[index]!];
      return list;
    });
  }
  if (parentPath.length === 0) return null;
  const grandPath = parentPath.slice(0, -1);
  const parentIndex = parentPath[parentPath.length - 1]!;
  return editList(tree, grandPath, (list) => {
    const parent = list[parentIndex]!;
    list[parentIndex] = { ...parent, children: parent.children.slice(0, -1) };
    list.splice(parentIndex + 1, 0, loc.block);
    return list;
  });
}

function locateByPath(tree: Tree, path: number[]): OBlock | null {
  let blocks: Tree = tree;
  let block: OBlock | null = null;
  for (const i of path) {
    block = blocks[i] ?? null;
    if (!block) return null;
    blocks = block.children;
  }
  return block;
}

function removeBlock(tree: Tree, loc: Location): OBlock[] {
  return editList(tree, loc.path.slice(0, -1), (list) => {
    list.splice(loc.path[loc.path.length - 1]!, 1);
    return list;
  });
}

/**
 * Backspace at the start: append this block's text to the previous visible block and
 * remove this one. Refused for a block with children (they would need a new home) and
 * for the first block of the page.
 */
export function mergeWithPrevious(tree: Tree, id: string): { tree: OBlock[]; focus: string; caret: number } | null {
  const order = visibleIds(tree);
  const at = order.indexOf(id);
  if (at <= 0) return null;
  const loc = locate(tree, id)!;
  if (loc.block.children.length > 0) return null;
  const prevId = order[at - 1]!;
  const prev = locate(tree, prevId)!.block;
  const prevText = editableText(prev.content);
  const merged = withEditedText(prev.content, prevText + editableText(loc.block.content));
  const withoutBlock = removeBlock(tree, loc);
  return { tree: mapBlock(withoutBlock, prevId, (b) => ({ ...b, content: merged })), focus: prevId, caret: prevText.length };
}

export function previousVisible(tree: Tree, id: string): string | null {
  const order = visibleIds(tree);
  return order[order.indexOf(id) - 1] ?? null;
}

export function nextVisible(tree: Tree, id: string): string | null {
  const order = visibleIds(tree);
  const at = order.indexOf(id);
  return at === -1 ? null : (order[at + 1] ?? null);
}

/** Does saving this tree write anything worth a file? (A page that is just one empty block does not.) */
export function hasContent(tree: Tree): boolean {
  return tree.length > 1 || tree.some((b) => b.content.trim() !== "" || b.children.length > 0);
}
