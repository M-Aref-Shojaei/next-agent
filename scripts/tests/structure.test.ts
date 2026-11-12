// Tests the structural checker itself: it must catch the failure modes it
// exists to prevent, and stay quiet on valid Rust.
import { test } from "node:test";
import assert from "node:assert/strict";
import { scan, orphanFunctions } from "../lib/rust-structure.mjs";

test("balanced code produces no problems", () => {
  assert.deepEqual(scan('fn f() { let x = "abc"; }'), []);
});

test("an unterminated string is reported", () => {
  const problems = scan('fn f() { let x = "abc; }');
  assert.ok(problems.some((p) => p.includes("unterminated string")), problems.join("; "));
});

test("a missing closing brace is reported", () => {
  const problems = scan("impl Foo {\n    fn a(&self) {}\n");
  assert.ok(problems.some((p) => p.includes("never closed")), problems.join("; "));
});

test("mismatched delimiters are reported", () => {
  const problems = scan("fn f() { g(]; }");
  assert.ok(problems.length > 0);
});

test("comments and raw strings do not confuse the scanner", () => {
  const source = [
    "// a brace { in a comment",
    "/* block { with } braces */",
    'fn f() -> &\'static str { r#"raw } { text"# }',
  ].join("\n");
  assert.deepEqual(scan(source), []);
});

test("a method appended after an impl block is flagged", () => {
  const source = [
    "impl Foo {",
    "    pub fn a(&self) -> u8 { 1 }",
    "}",
    "    /// Orphaned method.",
    "    pub fn b(&self) -> u8 {",
    "        2",
    "    }",
  ].join("\n");
  assert.deepEqual(orphanFunctions(source), [5]);
});

test("free functions are not flagged", () => {
  const source = ["fn helper(x: u8) -> u8 {", "    x + 1", "}", "pub fn other(self_like: u8) {}"].join(
    "\n",
  );
  assert.deepEqual(orphanFunctions(source), []);
});
