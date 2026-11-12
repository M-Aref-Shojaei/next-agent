import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { evaluate } from "../src/index.ts";

interface Fixture {
  context: { root: string; home: string; workspaceId: string; caseInsensitive: boolean; allowlistedHosts: string[] };
  cases: { name: string; mode: "plan" | "manual" | "auto" | "bypass"; request: Record<string, unknown>; expect: string; rule: string }[];
}

const fixture: Fixture = JSON.parse(readFileSync(new URL("../../../fixtures/policy-cases.json", import.meta.url), "utf8"));
const ctx = {
  root: fixture.context.root,
  home: fixture.context.home,
  workspaceId: fixture.context.workspaceId,
  caseInsensitive: fixture.context.caseInsensitive,
  allowlistedHosts: fixture.context.allowlistedHosts,
};

test("fixture is non-trivial", () => {
  assert.ok(fixture.cases.length >= 25, "the corpus should cover the whole decision table");
});

for (const testCase of fixture.cases) {
  test(`policy: ${testCase.name}`, () => {
    const actual = evaluate(testCase.request as never, testCase.mode, ctx);
    assert.equal(actual.verdict, testCase.expect, `${testCase.name} verdict`);
    assert.equal(actual.rule, testCase.rule, `${testCase.name} rule`);
  });
}
