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
