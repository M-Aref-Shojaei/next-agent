import assert from "node:assert/strict";
import test from "node:test";
import {
  extractWikilinks,
  pageFileBody,
  parseFrontmatter,
  slugify,
  taskRecordPath,
  titleFromFileBody,
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

import { readFileSync } from "node:fs";
import {
  TASK_END_MARKER,
  journalLine,
  journalPath,
  parseJournalLinks,
  parseTaskRecord,
  taskPageTitle,
  taskRecordStem,
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
    const [iso, workspace, title, record, journal] = line.split("\t");
    assert.equal(taskRecordPath(iso, workspace, title), record, line);
    assert.equal(journalPath(iso), journal, line);
  }
});

test("page file names match the shared Rust/TS fixture (Logseq triple-lowbar)", () => {
  const cases = fixture("page-file-names.tsv")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"));
  assert.ok(cases.length >= 15);
  for (const line of cases) {
    const [title, body, back] = line.split("\t");
    assert.equal(pageFileBody(title), body, line);
    assert.equal(titleFromFileBody(body), back, line);
  }
});

test("ISO dates are not shifted across timezones", () => {
  assert.equal(journalPath("2026-12-31T23:59:59-08:00"), "journals/2026_12_31.md");
  assert.equal(journalPath("2026-12-31T23:59:59-08:00", "yyyy-MM-dd"), "journals/2026-12-31.md");
  assert.equal(taskRecordPath("yesterday", "w", "x"), null);
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
  assert.equal(record.frontmatter.task, record.title);
  assert.ok(record.userNotes?.startsWith("User notes below this line are preserved."));
  assert.ok(fixture("golden-task-record.md").includes(TASK_END_MARKER));
  // The Logseq page name in the frontmatter is the one the writer derives.
  const stem = taskRecordStem(String(record.frontmatter.finished), record.title);
  assert.equal(record.frontmatter.title, taskPageTitle(String(record.frontmatter.workspace), stem ?? ""));
  assert.equal(
    taskRecordPath(String(record.frontmatter.finished), String(record.frontmatter.workspace), record.title),
    "pages/Next Agent___my-app___Tasks___2026-11-10-add-login-form-validation.md",
  );
});

test("parses journal links", () => {
  const links = parseJournalLinks(
    "- [[Next Agent/a/Tasks/2026-11-10-x]] — done (auto)\n- plain bullet\n",
  );
  assert.deepEqual(links, [{ page: "Next Agent/a/Tasks/2026-11-10-x", status: "done", mode: "auto" }]);
});

test("parses the golden journal written by Rust", () => {
  const page = "Next Agent/my-app/Tasks/2026-11-10-add-login-form-validation";
  assert.equal(fixture("golden-journal.md"), `${journalLine(page, "done", "auto")}\n`);
  assert.deepEqual(parseJournalLinks(fixture("golden-journal.md")), [
    { page, status: "done", mode: "auto" },
  ]);
});
