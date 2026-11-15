import { test } from "node:test";
import assert from "node:assert/strict";
import { cronInputOf, emptyCronInput, memoryUsage, relativeTime, sortSkills, splitList, usagePercent, type SkillInfo } from "../src/lib/learning.ts";

test("memory usage matches the on-disk count (trimmed, deduplicated)", () => {
  const { entries, used } = memoryUsage(" a \n§\nb\n§\na\n§\n\n");
  assert.deepEqual(entries, ["a", "b"]);
  assert.equal(used, "a\n§\nb".length);
  assert.equal(memoryUsage("é").used, 1);
  assert.equal(usagePercent(1100, 2200), 50);
  assert.equal(usagePercent(5000, 2200), 100);
  assert.equal(usagePercent(1, 0), 0);
});

test("relative times", () => {
  const now = 1_000_000_000;
  assert.equal(relativeTime(null, now), "—");
  assert.equal(relativeTime(now / 1000 + 300, now), "in 5 min");
  assert.equal(relativeTime(now / 1000 - 7200, now), "2 h ago");
  assert.equal(relativeTime(now / 1000 + 3 * 86_400, now), "in 3 d");
});

test("lists, skill order and cron forms", () => {
  assert.deepEqual(splitList("deploy, , notes "), ["deploy", "notes"]);
  const skill = (name: string, state: SkillInfo["state"]): SkillInfo => ({ name, state, description: "", category: null, scope: "workspace", root: "/r", pinned: false, createdBy: null, useCount: 0, viewCount: 0, patchCount: 0, lastActivityAt: null });
  assert.deepEqual(sortSkills([skill("b", "archived"), skill("c", "active"), skill("a", "stale")]).map((s) => s.name), ["a", "c", "b"]);
  const blank = emptyCronInput("ws");
  assert.equal(blank.workspaceId, "ws");
  assert.equal(blank.mode, "auto");
  const input = cronInputOf({ id: "j", workspaceId: "ws", name: "n", prompt: "p", schedule: "30m", scheduleLabel: "every 30m", skills: ["s"], modelRole: null, mode: "manual", workdir: null, timezone: "UTC", graceMinutes: null, repeat: 2, runCount: 0, state: "scheduled", nextRunAt: 1, lastRunAt: null, lastStatus: null, lastError: null, lastTaskId: null, origin: "user" });
  assert.equal(input.id, "j");
  assert.equal(input.modelRole, "");
  assert.equal(input.repeat, 2);
});
