import assert from "node:assert/strict";
import test from "node:test";
import {
  dailyBars,
  defaultSettings,
  effectiveStrategy,
  formatCost,
  formatTs,
  formatTokens,
  parseList,
  providerOf,
  savedPercent,
  switchReason,
  type UsageTotals,
} from "../src/lib/gateway.ts";

const day = (key: string, cost: number, requests = 1): UsageTotals => ({
  key, requests, failures: 0, promptTokens: 100, completionTokens: 50, cachedTokens: 0, reasoningTokens: 0, cost,
});

test("dailyBars fills missing days and scales to the tallest bar", () => {
  const today = new Date(Date.UTC(2026, 10, 14));
  const bars = dailyBars([day("2026-11-12", 2), day("2026-11-14", 1), day("2026-10-01", 99)], 7, "cost", today);
  assert.equal(bars.length, 7);
  assert.equal(bars[0].day, "2026-11-08");
  assert.equal(bars[6].day, "2026-11-14");
  assert.deepEqual(bars.map((b) => b.value), [0, 0, 0, 0, 2, 0, 1]);
  assert.equal(bars[4].height, 1);
  assert.equal(bars[6].height, 0.5);
  const tokens = dailyBars([day("2026-11-14", 0)], 1, "tokens", today);
  assert.equal(tokens[0].value, 150);
  assert.equal(dailyBars([], 3, "requests", today).every((b) => b.height === 0), true);
});

test("formatting helpers", () => {
  assert.equal(formatCost(0), "$0");
  assert.equal(formatCost(0.0032), "$0.0032");
  assert.equal(formatCost(1.234), "$1.23");
  assert.equal(formatTokens(950), "950");
  assert.equal(formatTokens(12_300), "12.3k");
  assert.equal(formatTokens(2_500_000), "2.5M");
  assert.equal(savedPercent(1000, 400), 60);
  assert.equal(savedPercent(0, 0), 0);
});

test("lists, providers and defaults", () => {
  assert.deepEqual(parseList("a, b\n\n c ,"), ["a", "b", "c"]);
  assert.equal(providerOf("openrouter/openai/gpt-4o"), "openrouter");
  assert.equal(providerOf("fast"), "");
  const settings = defaultSettings();
  assert.equal(settings.tokenSaver, true);
  assert.equal(settings.caveman, null);
  assert.equal(settings.comboStrategy, "fallback");
});

test("timestamps and effective strategy", () => {
  assert.equal(formatTs("2026-11-14T10:25:04.123Z"), "2026-11-14 10:25:04");
  assert.equal(effectiveStrategy(null, "smart"), "smart");
  assert.equal(effectiveStrategy("", "fallback"), "fallback");
  assert.equal(effectiveStrategy("round-robin", "smart"), "round-robin");
});

test("switch reasons read as plain words", () => {
  assert.equal(switchReason("failover"), "Failover (model failed)");
  assert.equal(switchReason("quality:truncated"), "Low quality: truncated answers");
  assert.equal(switchReason("quality:bad-tool-json"), "Low quality: broken tool-call JSON");
  assert.equal(switchReason("quality:new-kind"), "Low quality: new-kind");
});
