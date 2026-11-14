import assert from "node:assert/strict";
import { test } from "node:test";
import {
  completeLink,
  cycleMarker,
  editableText,
  isCollapsed,
  linkContext,
  markerOf,
  nextMarker,
  setCollapsed,
  setMarker,
  withEditedText,
} from "../src/lib/notes/content.ts";
import { createHistory } from "../src/lib/notes/history.ts";
import { caretLines, editorAction, type KeyContext, type KeyInput } from "../src/lib/notes/keymap.ts";
import {
  fromNodes,
  hasContent,
  indent,
  mergeWithPrevious,
  moveDown,
  moveUp,
  nextVisible,
  outdent,
  previousVisible,
  setText,
  split,
  toggleCollapsed,
  toNodes,
  visibleIds,
  type OBlock,
} from "../src/lib/notes/outline.ts";
import { hasMermaid, parseBody, queryOf, tokenize } from "../src/lib/notes/render.ts";
import { dayNumber, snippetParts } from "../src/lib/notes/types.ts";

interface N {
  content: string;
  children: N[];
}
const node = (content: string, ...children: N[]): N => ({ content, children });
const texts = (tree: readonly OBlock[]): unknown[] => tree.map((b) => (b.children.length ? [b.content, texts(b.children)] : b.content));
const sample = () => fromNodes([node("a", node("a1"), node("a2")), node("b"), node("c", node("c1"))]);
const id = (tree: readonly OBlock[], content: string): string => {
  const walk = (list: readonly OBlock[]): string | undefined => {
    for (const b of list) {
      if (b.content === content) return b.id;
      const hit = walk(b.children);
      if (hit) return hit;
    }
  };
  return walk(tree) ?? assert.fail(`no block ${content}`);
};

// ------------------------------------------------------------------ outline --

test("round trip: only content and children go back to the engine", () => {
  const wire = [{ content: "x\nid:: 1", children: [{ content: "y", children: [], uuid: "u", marker: "TODO" }], collapsed: true }];
  assert.deepEqual(toNodes(fromNodes(wire as never)), [{ content: "x\nid:: 1", children: [{ content: "y", children: [] }] }]);
});

test("indent makes the block the last child of the previous sibling; first block cannot indent", () => {
  const t = sample();
  assert.deepEqual(texts(indent(t, id(t, "b"))!), [["a", ["a1", "a2", "b"]], ["c", ["c1"]]]);
  assert.equal(indent(t, id(t, "a")), null);
  assert.equal(indent(t, id(t, "a1")), null);
});

test("indent expands a collapsed previous sibling so the block stays visible", () => {
  const t = fromNodes([node("a\ncollapsed:: true", node("a1")), node("b")]);
  const out = indent(t, id(t, "b"))!;
  assert.equal(isCollapsed(out[0]!.content), false);
  assert.equal(out[0]!.children.length, 2);
});

test("outdent moves the block after its parent and adopts later siblings", () => {
  const t = sample();
  assert.deepEqual(texts(outdent(t, id(t, "a1"))!), ["a", ["a1", ["a2"]], "b", ["c", ["c1"]]]);
  assert.equal(outdent(t, id(t, "a")), null);
});

test("move up/down swap siblings and hop over the parent at the edges", () => {
  const t = sample();
  assert.deepEqual(texts(moveDown(t, id(t, "a"))!), ["b", ["a", ["a1", "a2"]], ["c", ["c1"]]]);
  assert.deepEqual(texts(moveUp(t, id(t, "c"))!), [["a", ["a1", "a2"]], ["c", ["c1"]], "b"]);
  assert.equal(moveUp(t, id(t, "a")), null);
  assert.equal(moveDown(t, id(t, "c")), null);
  assert.deepEqual(texts(moveUp(t, id(t, "a1"))!), ["a1", ["a", ["a2"]], "b", ["c", ["c1"]]]);
  assert.deepEqual(texts(moveDown(t, id(t, "a2"))!), [["a", ["a1"]], "a2", "b", ["c", ["c1"]]]);
});

test("split: text after the caret becomes a sibling, or the first child of an expanded parent", () => {
  const t = fromNodes([node("hello world", node("kid")), node("plain")]);
  const first = split(t, id(t, "hello world"), 5)!;
  assert.deepEqual(texts(first.tree), [["hello", [" world", "kid"]], "plain"]);
  assert.equal(first.tree[0]!.children[0]!.id, first.focus);
  const second = split(t, id(t, "plain"), 5)!;
  assert.deepEqual(texts(second.tree), [["hello world", ["kid"]], "plain", ""]);
  const folded = fromNodes([node("p\ncollapsed:: true", node("kid"))]);
  assert.equal(split(folded, folded[0]!.id, 1)!.tree.length, 2);
});

test("split keeps hidden properties (id::) on the first half only", () => {
  const t = fromNodes([node("one two\nid:: 6502a8f1-0000-4000-8000-000000000001")]);
  const out = split(t, t[0]!.id, 3)!.tree;
  assert.equal(out[0]!.content, "one\nid:: 6502a8f1-0000-4000-8000-000000000001");
  assert.equal(out[1]!.content, " two");
});

test("backspace merges into the previous visible block and reports the caret", () => {
  const t = fromNodes([node("ab", node("cd")), node("ef")]);
  const merged = mergeWithPrevious(t, id(t, "ef"))!;
  assert.deepEqual(texts(merged.tree), [["ab", ["cdef"]]]);
  assert.equal(merged.caret, 2);
  assert.equal(merged.focus, id(t, "cd"));
});

test("backspace on an empty block removes it; first block and blocks with children are refused", () => {
  const t = fromNodes([node("keep"), node("")]);
  const out = mergeWithPrevious(t, t[1]!.id)!;
  assert.deepEqual(texts(out.tree), ["keep"]);
  assert.equal(mergeWithPrevious(t, t[0]!.id), null);
  const withKids = fromNodes([node("x"), node("y", node("z"))]);
  assert.equal(mergeWithPrevious(withKids, withKids[1]!.id), null);
});

test("merging skips children of a collapsed block", () => {
  const t = fromNodes([node("a\ncollapsed:: true", node("hidden")), node("b")]);
  const out = mergeWithPrevious(t, t[1]!.id)!;
  assert.equal(out.tree.length, 1);
  assert.equal(out.tree[0]!.content, "ab\ncollapsed:: true");
});

test("collapse writes and removes `collapsed:: true` and only for blocks with children", () => {
  const t = sample();
  const folded = toggleCollapsed(t, id(t, "a"))!;
  assert.equal(folded[0]!.content, "a\ncollapsed:: true");
  assert.deepEqual(visibleIds(folded).length, 4);
  assert.equal(toggleCollapsed(folded, folded[0]!.id)![0]!.content, "a");
  assert.equal(toggleCollapsed(t, id(t, "b")), null);
  assert.equal(toggleCollapsed(t, id(t, "a"), false), null);
});

test("untouched blocks keep their identity and bytes when a sibling changes", () => {
  const t = fromNodes([node("one\nid:: 6502a8f1-0000-4000-8000-000000000001\nnote:: x"), node("two")]);
  const next = setText(t, t[1]!.id, "two!")!;
  assert.equal(next[0], t[0]);
  assert.equal(setText(t, t[1]!.id, "two"), null);
  assert.equal(setText(t, t[0]!.id, editableText(t[0]!.content)), null);
});

test("visible navigation helpers and hasContent", () => {
  const t = sample();
  assert.equal(nextVisible(t, id(t, "a2")), id(t, "b"));
  assert.equal(previousVisible(t, id(t, "b")), id(t, "a2"));
  assert.equal(previousVisible(t, id(t, "a")), null);
  assert.equal(hasContent(fromNodes([node("")])), false);
  assert.equal(hasContent(fromNodes([node("x")])), true);
  assert.equal(hasContent(fromNodes([node(""), node("")])), true);
});

// ------------------------------------------------------------------ content --

test("hidden properties are not shown and come back after the property run", () => {
  const raw = "title\nid:: 6502a8f1-0000-4000-8000-000000000001\nowner:: Sam\nmore text";
  assert.equal(editableText(raw), "title\nowner:: Sam\nmore text");
  assert.equal(withEditedText(raw, "title!\nowner:: Sam\nmore text"), "title!\nowner:: Sam\nid:: 6502a8f1-0000-4000-8000-000000000001\nmore text");
  assert.equal(withEditedText(raw, editableText(raw)), raw);
  assert.equal(editableText("a\n\nid:: x"), "a\n\nid:: x"); // not in the property run: user text
});

test("collapsed helpers", () => {
  assert.equal(setCollapsed("a", true), "a\ncollapsed:: true");
  assert.equal(setCollapsed("a\nowner:: x", true), "a\nowner:: x\ncollapsed:: true");
  assert.equal(setCollapsed("a\ncollapsed:: true\nowner:: x", false), "a\nowner:: x");
  assert.equal(isCollapsed("a\ncollapsed:: false"), false);
});

test("task markers cycle", () => {
  assert.equal(markerOf("TODO x"), "TODO");
  assert.equal(markerOf("TODOx"), null);
  assert.equal(setMarker("TODO x\nnote:: 1", "DOING"), "DOING x\nnote:: 1");
  assert.equal(setMarker("## TODO x", "DONE"), "## DONE x");
  assert.equal(setMarker("x", "TODO"), "TODO x");
  assert.equal(setMarker("TODO x", null), "x");
  assert.equal(setMarker("TODO", null), "");
  assert.equal(nextMarker("TODO"), "DOING");
  assert.equal(nextMarker("DOING"), "DONE");
  assert.equal(nextMarker("DONE"), "TODO");
  assert.equal(nextMarker("LATER"), "NOW");
  let c = "task";
  const seen: string[] = [];
  for (let i = 0; i < 4; i++) seen.push((c = cycleMarker(c)));
  assert.deepEqual(seen, ["TODO task", "DOING task", "DONE task", "task"]);
});

test("[[ autocomplete context and completion", () => {
  assert.deepEqual(linkContext("see [[Gar", 9), { start: 4, query: "Gar" });
  assert.deepEqual(linkContext("[[", 2), { start: 0, query: "" });
  assert.equal(linkContext("done [[x]] ", 11), null);
  assert.equal(linkContext("[[a\nb", 5), null);
  assert.equal(linkContext("no links", 4), null);
  assert.deepEqual(completeLink("see [[Gar tail", 9, { start: 4, query: "Gar" }, "Garden"), { text: "see [[Garden]] tail", caret: 14 });
  assert.deepEqual(completeLink("see [[Gar]] tail", 9, { start: 4, query: "Gar" }, "Garden"), { text: "see [[Garden]] tail", caret: 14 });
});

// ------------------------------------------------------------------- render --

test("tokenize finds links, tags, block refs, code and URLs; code and URLs hide tags", () => {
  const u = "6502a8f1-0000-4000-8000-000000000001";
  assert.deepEqual(tokenize(`a [[Page One]] #tag #[[multi word]] ((${u})) \`#no\` https://x.io/#frag.`), [
    { kind: "text", text: "a " },
    { kind: "link", name: "Page One", label: "Page One" },
    { kind: "text", text: " " },
    { kind: "tag", name: "tag" },
    { kind: "text", text: " " },
    { kind: "tag", name: "multi word" },
    { kind: "text", text: " " },
    { kind: "blockref", uuid: u },
    { kind: "text", text: " " },
    { kind: "code", text: "#no" },
    { kind: "text", text: " " },
    { kind: "url", href: "https://x.io/#frag", label: "https://x.io/#frag" },
    { kind: "text", text: "." },
  ]);
  assert.deepEqual(tokenize("C# and a#b"), [{ kind: "text", text: "C# and a#b" }]);
  assert.deepEqual(tokenize("**b** *i* 2*3*4"), [
    { kind: "bold", text: "b" },
    { kind: "text", text: " " },
    { kind: "italic", text: "i" },
    { kind: "text", text: " 2*3*4" },
  ]);
  assert.deepEqual(tokenize("[site](https://a.b/c)"), [{ kind: "url", href: "https://a.b/c", label: "site" }]);
  assert.deepEqual(tokenize("[x](javascript:alert(1))"), [{ kind: "text", text: "[x](javascript:alert(1))" }]);
});

test("parseBody: marker, priority, heading, properties, fences, queries, drawers", () => {
  const body = parseBody("TODO [#A] Ship [[v1]]\nid:: 6502a8f1-0000-4000-8000-000000000001\nowner:: Sam\nSCHEDULED: <2026-11-14 Sat>\n:LOGBOOK:\nCLOCK: x\n:END:\n```mermaid\ngraph TD\n  A-->B\n```\n{{query (and [[x]] (task TODO))}}\nlast");
  assert.equal(body.marker, "TODO");
  assert.equal(body.priority, "A");
  assert.deepEqual(
    body.parts.map((p) => p.kind),
    ["line", "property", "meta", "code", "query", "line"],
  );
  assert.deepEqual(body.parts[3], { kind: "code", lang: "mermaid", code: "graph TD\n  A-->B" });
  assert.deepEqual(body.parts[4], { kind: "query", text: "(and [[x]] (task TODO))" });
  assert.equal(hasMermaid(body), true);
  assert.equal(parseBody("## Title").heading, 2);
  assert.equal(parseBody("DONE ## x").marker, "DONE");
  assert.deepEqual(parseBody("```mermaid\ngraph TD").parts, [{ kind: "code", lang: "mermaid", code: "graph TD" }]);
  assert.equal(parseBody("{{query [[a]]}}").parts[0]?.kind, "query");
  assert.equal(queryOf("{{query (task TODO)}}"), "(task TODO)");
  assert.equal(queryOf("{{embed [[x]]}}"), null);
  assert.deepEqual(
    parseBody("Open tasks: {{query (task TODO)}} (live)").parts.map((p) => p.kind),
    ["line", "query", "line"],
  );
});

// ------------------------------------------------------------------- keymap --

const key = (k: string, extra: Partial<KeyInput> = {}): KeyInput => ({ key: k, shiftKey: false, altKey: false, ctrlKey: false, metaKey: false, ...extra });
const ctx = (extra: Partial<KeyContext> = {}): KeyContext => ({ selectionStart: 3, selectionEnd: 3, onFirstLine: true, onLastLine: true, popupOpen: false, ...extra });

test("keymap: Enter splits, Shift+Enter is a newline, Tab / Shift+Tab indent / outdent", () => {
  assert.equal(editorAction(key("Enter"), ctx()), "split");
  assert.equal(editorAction(key("Enter", { shiftKey: true }), ctx()), null);
  assert.equal(editorAction(key("Tab"), ctx()), "indent");
  assert.equal(editorAction(key("Tab", { shiftKey: true }), ctx()), "outdent");
  assert.equal(editorAction(key("Enter", { isComposing: true }), ctx()), null);
});

test("keymap: Alt+Up/Down move, Mod+Z / Mod+Shift+Z undo/redo, Mod+Enter cycles the task", () => {
  assert.equal(editorAction(key("ArrowUp", { altKey: true }), ctx()), "moveUp");
  assert.equal(editorAction(key("ArrowDown", { altKey: true }), ctx()), "moveDown");
  assert.equal(editorAction(key("z", { metaKey: true }), ctx()), "undo");
  assert.equal(editorAction(key("Z", { metaKey: true, shiftKey: true }), ctx()), "redo");
  assert.equal(editorAction(key("y", { ctrlKey: true }), ctx()), "redo");
  assert.equal(editorAction(key("Enter", { ctrlKey: true }), ctx()), "cycleTask");
  assert.equal(editorAction(key("ArrowUp", { metaKey: true }), ctx()), "collapse");
  assert.equal(editorAction(key("ArrowDown", { metaKey: true }), ctx()), "expand");
  assert.equal(editorAction(key("b", { metaKey: true }), ctx()), null);
});

test("keymap: Backspace merges only at the very start; arrows leave the block only at its edge", () => {
  assert.equal(editorAction(key("Backspace"), ctx({ selectionStart: 0, selectionEnd: 0 })), "mergePrevious");
  assert.equal(editorAction(key("Backspace"), ctx({ selectionStart: 1, selectionEnd: 1 })), null);
  assert.equal(editorAction(key("Backspace"), ctx({ selectionStart: 0, selectionEnd: 4 })), null);
  assert.equal(editorAction(key("ArrowUp"), ctx({ onFirstLine: true })), "focusPrevious");
  assert.equal(editorAction(key("ArrowUp"), ctx({ onFirstLine: false })), null);
  assert.equal(editorAction(key("ArrowDown"), ctx({ onLastLine: false })), null);
  assert.equal(editorAction(key("Escape"), ctx()), "exit");
});

test("keymap: an open [[ popup takes Enter, Tab, arrows and Escape", () => {
  const open = ctx({ popupOpen: true });
  assert.equal(editorAction(key("Enter"), open), "popupAccept");
  assert.equal(editorAction(key("Tab"), open), "popupAccept");
  assert.equal(editorAction(key("ArrowDown"), open), "popupNext");
  assert.equal(editorAction(key("ArrowUp"), open), "popupPrevious");
  assert.equal(editorAction(key("Escape"), open), "popupClose");
  assert.equal(editorAction(key("z", { metaKey: true }), open), "undo");
});

test("caretLines", () => {
  assert.deepEqual(caretLines("a\nb", 1), { onFirstLine: true, onLastLine: false });
  assert.deepEqual(caretLines("a\nb", 3), { onFirstLine: false, onLastLine: true });
});

// ------------------------------------------------------------------ history --

test("history: typing merges inside the window, structural edits do not, redo clears on new edits", () => {
  let t = 0;
  const h = createHistory<string>({ now: () => t, windowMs: 1000 });
  h.record("", "type:b1");
  t = 500;
  h.record("a", "type:b1"); // merged: the step still starts from ""
  t = 2000;
  h.record("ab", "type:b1"); // window passed: new step
  h.record("abc"); // structural
  assert.equal(h.undo("abcd"), "abc");
  assert.equal(h.undo("abc"), "ab");
  assert.equal(h.undo("ab"), "");
  assert.equal(h.undo(""), undefined);
  assert.equal(h.redo(""), "ab");
  h.record("ab", "x");
  assert.equal(h.canRedo, false);
});

test("history keeps at most `limit` steps", () => {
  const h = createHistory<number>({ limit: 3 });
  for (let i = 0; i < 10; i++) h.record(i);
  let n = 0;
  while (h.undo(0) !== undefined) n++;
  assert.equal(n, 3);
});

// --------------------------------------------------------------------- misc --

test("snippet parts and day numbers", () => {
  assert.deepEqual(snippetParts("a hit b"), [
    { text: "a ", match: false },
    { text: "hit", match: true },
    { text: " b", match: false },
  ]);
  assert.equal(dayNumber(new Date(2026, 10, 14)), 20261114);
});
