import assert from "node:assert/strict";
import test from "node:test";
import {
  extractWikilinks,
  parseFrontmatter,
  slugify,
  taskRecordPath,
} from "../src/index.ts";

test("parses frontmatter keys and inline lists", () => {
  const { data, body } = parseFrontmatter(
    "---\ntype: task\ntags: [a, \"b c\"]\nstatus: done\n---\n# Title\n",
  );
  assert.equal(data.type, "task");
  assert.deepEqual(data.tags, ["a", "b c"]);
  assert.equal(data.status, "done");
  assert.equal(body, "# Title\n");
});

test("returns body unchanged without frontmatter", () => {
  assert.deepEqual(parseFrontmatter("# hi\n"), { data: {}, body: "# hi\n" });
});

test("ignores malformed frontmatter lines", () => {
  const { data } = parseFrontmatter("---\nnonsense\n: empty\nok: yes\n---\n");
  assert.deepEqual(data, { ok: "yes" });
});

test("extracts wikilinks without aliases and anchors", () => {
  const links = extractWikilinks("See [[Roadmap|the plan]], [[ADR-0001#Decision]] and [[x]].");
  assert.deepEqual(links, ["Roadmap", "ADR-0001", "x"]);
});

test("slugify is stable and ascii-safe", () => {
  assert.equal(slugify("Add login form validation"), "add-login-form-validation");
  assert.equal(slugify("  Héllo, World!  "), "hello-world");
  assert.equal(slugify("!!!"), "untitled");
  assert.equal(slugify("a".repeat(200)).length, 80);
});

test("task record paths are nested by year and month", () => {
  assert.equal(
    taskRecordPath(new Date(2026, 10, 10), "Add login form"),
    "Tasks/2026/11/2026-11-10-add-login-form.md",
  );
  assert.equal(
    taskRecordPath(new Date(2026, 0, 5), "x"),
    "Tasks/2026/01/2026-01-05-x.md",
  );
});

import { readFileSync } from "node:fs";
import {
  TASK_END_MARKER,
  journalPath,
  parseJournalLinks,
  parseTaskRecord,
  taskRecordPathFromIso,
  workspaceFolder,
} from "../src/index.ts";

const fixture = (name: string) =>
  readFileSync(new URL(`../../../fixtures/vault/${name}`, import.meta.url), "utf8");

test("record paths match the shared Rust/TS fixture", () => {
  const cases = fixture("path-cases.tsv")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"));
  assert.ok(cases.length >= 6);
  for (const line of cases) {
    const [iso, title, expected] = line.split("\t");
    assert.equal(taskRecordPathFromIso(iso, title), expected, line);
  }
});

test("ISO dates are not shifted across timezones", () => {
  assert.equal(journalPath("2026-12-31T23:59:59-08:00"), "Journal/2026-12-31.md");
  assert.equal(taskRecordPathFromIso("yesterday", "x"), null);
  assert.equal(journalPath("nope"), null);
});

test("workspace folders cannot escape", () => {
  assert.equal(workspaceFolder("../../etc"), "etc");
  assert.equal(workspaceFolder("a/b"), "a-b");
  assert.equal(workspaceFolder("..."), "workspace");
  assert.equal(workspaceFolder("my-app"), "my-app");
});

test("parses the golden task record written by Rust", () => {
  const record = parseTaskRecord(fixture("golden-task-record.md"));
  assert.equal(record.frontmatter.type, "task");
  assert.equal(record.frontmatter.status, "completed");
  assert.equal(record.frontmatter.merged, "false");
  assert.deepEqual(record.frontmatter.tags, ["next-agent/task"]);
  assert.equal(record.title, "Add login form validation");
  assert.equal(record.frontmatter.title, record.title);
  assert.ok(record.userNotes?.startsWith("User notes below this line are preserved."));
  assert.ok(fixture("golden-task-record.md").includes(TASK_END_MARKER));
  // The record path derived from its own frontmatter matches the writer's.
  assert.equal(
    taskRecordPathFromIso(String(record.frontmatter.finished), String(record.frontmatter.title)),
    "Tasks/2026/11/2026-11-10-add-login-form-validation.md",
  );
});

test("parses journal links", () => {
  const links = parseJournalLinks(
    "# 2026-11-10\n\n- [[2026-11-10-add-login-form]] — done (auto)\n- plain bullet\n",
  );
  assert.deepEqual(links, [{ stem: "2026-11-10-add-login-form", status: "done", mode: "auto" }]);
});
