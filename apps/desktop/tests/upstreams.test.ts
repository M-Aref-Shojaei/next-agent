import assert from "node:assert/strict";
import test from "node:test";
import { formatIso, formatUnix, rateLimitNote, reportFor, summary, type UpstreamReport, type WatchView } from "../src/lib/upstreams.ts";

const report = (aheadBy: number, releases: number): UpstreamReport => ({
  id: "coucou",
  repo: "Louis-CFM/coucou",
  pinned: "0".repeat(40),
  pinnedDate: null,
  branch: "main",
  aheadBy,
  commits: [],
  releases: Array.from({ length: releases }, (_, i) => ({ tag: `v${i}`, name: `v${i}`, date: "", url: "", prerelease: false })),
  areas: [],
  compareUrl: "",
  checkedAt: 0,
  error: null,
});

const view = (over: Partial<WatchView> = {}): WatchView => ({
  watched: [],
  reports: [report(1, 0)],
  lastCheck: null,
  rateLimitedUntil: null,
  unseen: [],
  checking: false,
  error: null,
  ...over,
});

test("summary counts commits and releases since the pin", () => {
  assert.equal(summary(undefined), "Not checked yet.");
  assert.equal(summary(report(0, 0)), "Up to date with the pin.");
  assert.equal(summary(report(1, 0)), "1 new commit since the pin.");
  assert.equal(summary(report(3, 2)), "3 new commits, 2 new releases since the pin.");
});

test("rate-limit note only while GitHub makes us wait", () => {
  assert.equal(rateLimitNote(null, 100), null);
  assert.equal(rateLimitNote(view({ rateLimitedUntil: 50 }), 100), null);
  assert.match(rateLimitNote(view({ rateLimitedUntil: 100 + 600 }), 100) ?? "", /about 10 min/);
});

test("formatting tolerates missing values", () => {
  assert.equal(formatUnix(null), "never");
  assert.equal(formatIso(""), "");
  assert.equal(formatIso("not a date"), "");
  assert.notEqual(formatIso("2026-11-14T08:00:00Z"), "");
  assert.equal(reportFor(view(), "coucou")?.aheadBy, 1);
  assert.equal(reportFor(view(), "other"), undefined);
});
