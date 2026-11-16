// In-memory stand-in for the Rust commands, used outside Tauri.
import type { Api, BoardDoc, Workspace } from "./api.ts";
import type { LogEntry, LogLevel } from "./log.ts";
import { mockGraphApi } from "./notes/mock-graph.ts";
import { designMockApi } from "./design-mock.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";
import type { WatchView } from "./upstreams.ts";
import { ENTRY_DELIMITER, MEMORY_LIMITS, memoryUsage, type CronJob, type LearningSettings, type SkillInfo } from "./learning.ts";
import {
  defaultSettings,
  type Combo,
  type GatewaySettings,
  type ProviderConnection,
  type ProviderPreset,
  type UsageTotals,
} from "./gateway.ts";
import type { AllowEntry, ChannelActivity, ChannelInfo, PairingRequest } from "./channels.ts";

const presets: ProviderPreset[] = [
  { id: "openai", label: "OpenAI", kind: "openai", baseUrl: "https://api.openai.com/v1", needsKey: true, models: ["gpt-4.1", "gpt-4o-mini"] },
  { id: "anthropic", label: "Anthropic", kind: "anthropic", baseUrl: "https://api.anthropic.com/v1", needsKey: true, models: ["claude-sonnet-4-5"] },
  { id: "gemini", label: "Google Gemini", kind: "gemini", baseUrl: "https://generativelanguage.googleapis.com/v1beta", needsKey: true, models: ["gemini-2.5-flash"] },
  { id: "ollama", label: "Ollama (local)", kind: "openai", baseUrl: "http://127.0.0.1:11434/v1", needsKey: false, models: ["llama3.2"] },
  { id: "custom", label: "Custom (OpenAI-compatible)", kind: "openai", baseUrl: "http://127.0.0.1:8000/v1", needsKey: false, models: [] },
];
const connections: ProviderConnection[] = [];
const keyed = new Set<string>();
const combos: Combo[] = [];
let gatewaySettings: GatewaySettings = defaultSettings();
let catalog: { models: number; updatedAt: number | null } = { models: 0, updatedAt: null };
let cacheEntries = 3;
const PIN = "0d7fe58884d99f275f3238e8dbe29ddee69fa9e8";
let upstreamView: WatchView = {
  watched: [{ id: "coucou", repo: "Louis-CFM/coucou", commit: PIN }],
  reports: [],
  lastCheck: null,
  rateLimitedUntil: null,
  unseen: [],
  checking: false,
  error: null,
};
const mockDay = (offset: number, cost: number, requests: number): UsageTotals => ({
  key: new Date(Date.now() - offset * 86_400_000).toISOString().slice(0, 10),
  requests, failures: 0, promptTokens: requests * 1200, completionTokens: requests * 300,
  cachedTokens: requests * 400, reasoningTokens: 0, cost,
});

const workspaces: Workspace[] = [];
const settings = new Map<string, string>([["router.url", "http://127.0.0.1:20128/v1"]]);
let vault: string | null = null;
const waiting = new Map<string, (choice: ApprovalChoice) => void>();
let counter = 0;

const EMPTY_SCENE = JSON.stringify({ type: "excalidraw", version: 2, source: "next-agent", elements: [], appState: {}, files: {} });
let learningSettings: LearningSettings = { backgroundReview: true };
const memoryText = { memory: "Project uses pnpm and cargo nextest.", user: "Name: Sam\n§\nPrefers concise answers." };
const mockSkills: SkillInfo[] = [
  { name: "deploy", description: "Use when shipping a release.", category: "ops", scope: "workspace", root: "/ws/.next-agent/skills", state: "active", pinned: false, createdBy: "agent", useCount: 4, viewCount: 4, patchCount: 1, lastActivityAt: null },
  { name: "notes", description: "Use when writing daily notes.", category: null, scope: "user", root: "/vault/skills", state: "stale", pinned: true, createdBy: null, useCount: 1, viewCount: 1, patchCount: 0, lastActivityAt: null },
];
const cronJobs: CronJob[] = [];
const fileOf = (target: "memory" | "user") => (target === "memory" ? "MEMORY.md" : "USER.md");
const memoryFile = (target: "memory" | "user") => {
  const { entries, used } = memoryUsage(memoryText[target]);
  return { target, text: entries.join(ENTRY_DELIMITER), entries: entries.length, used, limit: MEMORY_LIMITS[target], path: `/data/memory/${fileOf(target)}` };
};
const findSkill = (name: string) => {
  const skill = mockSkills.find((s) => s.name === name);
  if (!skill) throw `no skill named ${name}`;
  return skill;
};
const findJob = (id: string) => {
  const job = cronJobs.find((j) => j.id === id);
  if (!job) throw `No job '${id}'.`;
  return job;
};

const boards = new Map<string, { content: string; rev: number; modifiedMs: number }>();
const trash: string[] = [];
const assets = new Map<string, string>();

function boardName(raw: string): string {
  const name = raw.trim();
  if (!name || /[/\\:*?"<>|]/.test(name) || name.startsWith(".") || name.endsWith(".")) throw `invalid board name: ${raw}`;
  if (/^(con|prn|aux|nul|conin\$|conout\$|(com|lpt)[1-9¹²³])$/i.test((name.split(".")[0] ?? "").trimEnd())) throw `board name is reserved by the operating system: ${raw}`;
  return name;
}
function boardDoc(name: string): BoardDoc {
  const b = boards.get(name);
  if (!b) throw `board "${name}" not found`;
  return { name, content: b.content, revision: String(b.rev) };
}
function requireVault() {
  if (vault === null) throw "no vault configured: set a vault folder in Settings first";
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// Chat channels: one demo pairing request so the Settings page has content.
const channels: ChannelInfo[] = [];
const channelTokens = new Set<string>();
const pairings = new Map<string, PairingRequest[]>();
const allowLists = new Map<string, AllowEntry[]>();
const channelLog = new Map<string, ChannelActivity[]>();
const nowSecs = () => Math.floor(Date.now() / 1000);
function findChannel(id: string): ChannelInfo {
  const c = channels.find((x) => x.config.id === id);
  if (!c) throw `unknown channel ${id}`;
  return c;
}

function envelope(taskId: string, seq: number, kind: string, data: object): StreamMessage {
  return {
    type: "event",
    event: { v: 1, id: `${taskId}:${seq}`, task_id: taskId, seq, ts: new Date().toISOString(), kind, data },
  };
}

async function runDemo(
  taskId: string,
  mode: string,
  send: (m: StreamMessage) => void,
): Promise<void> {
  let seq = 0;
  const emit = async (kind: string, data: object) => {
    send(envelope(taskId, seq++, kind, data));
    await sleep(150);
  };
  await emit("task_started", { mode, executor: "mock" });
  await emit("message", { text: "Looking at the project first." });
  const calls = [
    { id: "call_1", tool: "read_file", ask: false },
    { id: "call_2", tool: "write_file", ask: mode === "manual" },
    { id: "call_3", tool: "run_command", ask: false },
  ];
  let failed = false;
  for (const call of calls) {
    await emit("tool_requested", { call_id: call.id, tool: call.tool });
    let denied = mode === "plan" && call.tool !== "read_file";
    if (call.ask) {
      const approvalId = `${taskId}:${call.id}`;
      const answer = new Promise<ApprovalChoice>((resolve) => waiting.set(approvalId, resolve));
      send({
        type: "approval",
        task_id: taskId,
        approval_id: approvalId,
        call_id: call.id,
        tool: call.tool,
        summary: "NOTES.md (8 bytes)",
        reason: "mutating tool in manual mode",
      });
      denied = (await answer) === "deny";
    }
    await emit("permission_decided", {
      call_id: call.id,
      decision: denied ? "deny" : "allow",
      reason: denied ? "blocked" : "mock policy",
    });
    if (!denied) {
      send({ type: "tool_output", task_id: taskId, call_id: call.id, ok: true, output: `(mock) ${call.tool} completed` });
    }
    await emit("tool_finished", { call_id: call.id, ok: !denied });
    if (denied) {
      failed = true;
      break;
    }
  }
  await emit("task_completed", { summary: failed ? "1 tool call(s) failed" : "3 tool call(s) executed" });
}

// Diagnostics: newest first, like the Rust ring buffer. Sample entries make the page look alive in a browser.
const LEVEL_RANK: Record<LogLevel, number> = { error: 0, warn: 1, info: 2, debug: 3 };
let logLevel: LogLevel = "info";
const sample = (ago: number, level: LogLevel, target: string, event: string, fields: Record<string, unknown>): LogEntry => ({
  ts: new Date(Date.now() - ago * 1000).toISOString(), level, target, event, fields,
});
const logEntries: LogEntry[] = [
  sample(20, "info", "router", "provider_call", { provider: "anthropic", model: "claude-sonnet-4-5", status: 200, duration_ms: 812, tokens_in: 1200, tokens_out: 300 }),
  sample(95, "warn", "router", "provider_call_failed", { provider: "openai", model: "gpt-4.1", status: 529, duration_ms: 1330, attempt: 2 }),
  sample(240, "info", "tasks", "task_finished", { task_id: "t_01JMOCK", mode: "agent", ok: true, duration_ms: 41200 }),
  sample(300, "error", "ui", "command_failed", { command: "gateway_combo_save", duration_ms: 14, error: "unknown role `x`" }),
  sample(600, "info", "app", "app_started", { version: "0.1.0", os: "macos", arch: "aarch64" }),
];

export const mockApi: Api = {
  ...mockGraphApi, // notes-ui
  async diagRecent(filter) {
    return logEntries
      .filter((e) => (!filter?.level || e.level === filter.level) && (!filter?.target || e.target === filter.target))
      .slice(0, filter?.limit ?? 200);
  },
  async diagSetLevel(level) {
    logLevel = level;
  },
  async diagOpenFolder() {},
  async diagExport() {
    return { path: "~/Downloads/next-agent-diagnostics.jsonl", entries: logEntries.length };
  },
  async diagLog(level, event, fields) {
    if (LEVEL_RANK[level] > LEVEL_RANK[logLevel]) return;
    // Like Rust, events from the UI belong to the "ui" area.
    logEntries.unshift({ ts: new Date().toISOString(), level, target: "ui", event, fields: { ...fields } });
    logEntries.length = Math.min(logEntries.length, 2000);
  },
  async workspaceOpen(path) {
    const trimmed = path.trim();
    if (!trimmed) throw "workspace path is empty";
    const existing = workspaces.find((w) => w.path === trimmed);
    if (existing) return existing;
    const ws = { id: `ws_mock${workspaces.length}`, path: trimmed, name: trimmed.split("/").filter(Boolean).pop() ?? trimmed };
    workspaces.push(ws);
    return ws;
  },
  async workspaceList() {
    return [...workspaces];
  },
  async vaultSet(path) {
    vault = path;
    return path;
  },
  async vaultGet() {
    return vault;
  },
  async settingGet(key) {
    return settings.get(key) ?? null;
  },
  async settingSet(key, value) {
    settings.set(key, value);
  },
  async rulesList() {
    return [];
  },
  async ruleDelete() {
    return false;
  },
  async auditVerify() {
    return { ok: true, checked: 0, broken_at: null };
  },
  async taskStart(_ws, _prompt, mode, onMessage) {
    const taskId = `tsk_mock${(counter += 1)}`;
    void runDemo(taskId, mode, onMessage);
    return taskId;
  },
  async taskCancel() {},
  async agentProfiles() {
    return {
      profiles: [
        { name: "build", description: "Default agent with all tools", builtin: true },
        { name: "plan", description: "Read-only planning", builtin: true },
      ],
      warnings: [],
    };
  },
  async taskUndoStep() {
    throw "mock API: nothing to undo";
  },
  async taskUndoable() {
    return 0;
  },
  async approvalRespond(_taskId, approvalId, choice) {
    const resolve = waiting.get(approvalId);
    if (!resolve) throw `no pending approval ${approvalId}`;
    waiting.delete(approvalId);
    resolve(choice);
  },
  async reviewAction(_taskId, action) {
    return { action, commit: action === "merge" ? "mock000" : null, files: [] };
  },
  async routerHealth() {
    return { status: "down", detail: "mock API: no router", latencyMs: null, checks: 1 };
  },
  async onRouterHealth() {
    return () => {};
  },
  async gatewayPresets() {
    return presets;
  },
  async gatewayConnections() {
    return connections.map((c) => ({ ...c, hasKey: keyed.has(c.id) }));
  },
  async gatewayConnectionSave(input) {
    const preset = presets.find((p) => p.id === input.provider);
    if (!preset) throw `unknown provider \`${input.provider}\``;
    const id = input.id || `conn_mock${(counter += 1)}`;
    if (input.clearKey) keyed.delete(id);
    else if (input.apiKey?.trim()) keyed.add(id);
    const saved: ProviderConnection = {
      id, provider: preset.id, kind: preset.kind, name: input.name?.trim() || preset.label,
      priority: input.priority ?? 100, isActive: input.isActive ?? true,
      baseUrl: input.baseUrl?.trim() || preset.baseUrl, baseUrlOverride: input.baseUrl?.trim() ?? "",
      models: input.models ?? [], needsKey: preset.needsKey, hasKey: keyed.has(id),
    };
    const at = connections.findIndex((c) => c.id === id);
    if (at >= 0) connections[at] = saved;
    else connections.push(saved);
    return saved;
  },
  async gatewayConnectionDelete(id) {
    const at = connections.findIndex((c) => c.id === id);
    if (at >= 0) connections.splice(at, 1);
    keyed.delete(id);
    return at >= 0;
  },
  async gatewayConnectionTest(id) {
    const conn = connections.find((c) => c.id === id);
    if (!conn) throw `unknown connection ${id}`;
    if (conn.needsKey && !keyed.has(id)) return { ok: false, detail: "no API key stored", models: [] };
    return { ok: true, detail: "mock: 2 models available", models: ["model-a", "model-b"] };
  },
  async gatewayCombos() {
    return [...combos];
  },
  async gatewayComboSave(input) {
    if (!input.name.trim() || input.models.length === 0) throw "a combo needs a name and a model";
    const saved = { ...input, id: input.id || `combo_mock${(counter += 1)}` };
    const at = combos.findIndex((c) => c.id === saved.id);
    if (at >= 0) combos[at] = saved;
    else combos.push(saved);
    return saved;
  },
  async gatewayComboDelete(id) {
    const at = combos.findIndex((c) => c.id === id);
    if (at >= 0) combos.splice(at, 1);
    return at >= 0;
  },
  async gatewaySettingsGet() {
    return structuredClone(gatewaySettings);
  },
  async gatewaySettingsSet(next) {
    gatewaySettings = JSON.parse(JSON.stringify(next)); // UI state is a proxy; structuredClone rejects it.
  },
  async gatewayTargets() {
    return [
      ...combos.map((c) => ({ value: c.name, label: `${c.name} (combo)`, kind: "combo" as const })),
      ...connections.flatMap((c) =>
        (c.models.length ? c.models : (presets.find((p) => p.id === c.provider)?.models ?? [])).map((m) => ({
          value: `${c.provider}/${m}`, label: `${c.provider}/${m} (${c.name})`, kind: "model" as const,
        })),
      ),
    ];
  },
  async usageSummary(days) {
    const daily = [mockDay(4, 0.42, 12), mockDay(2, 1.1, 30), mockDay(0, 0.27, 8)];
    const sum = (f: (d: UsageTotals) => number) => daily.reduce((a, d) => a + f(d), 0);
    const totals: UsageTotals = {
      key: "total", requests: sum((d) => d.requests), failures: 1, promptTokens: sum((d) => d.promptTokens),
      completionTokens: sum((d) => d.completionTokens), cachedTokens: sum((d) => d.cachedTokens), reasoningTokens: 0,
      cost: sum((d) => d.cost),
    };
    return {
      days, totals, daily,
      byProvider: [{ ...totals, key: "anthropic" }],
      byModel: [{ ...totals, key: "claude-sonnet-4-5" }],
      byRole: [{ ...totals, key: "chat" }],
      recent: [{ ts: new Date().toISOString(), provider: "anthropic", model: "claude-sonnet-4-5", role: "chat", combo: null,
        status: 200, promptTokens: 1200, completionTokens: 300, cachedTokens: 400, cost: 0.0081, costKnown: true, latencyMs: 2300 }],
      switches: [{ ts: new Date().toISOString(), session: "task_mock1", combo: "steady", from: "groq/llama-3.3-70b-versatile",
        to: "anthropic/claude-sonnet-4-5", reason: "quality:truncated", confidence: 0.7 }],
      cacheHits: 4, cacheSavedTokens: 6_000, cacheSavedCost: 0.03,
      budgets: gatewaySettings.budgets.map((budget) => ({ budget, spent: budget.limitUsd * 0.85, level: "warn" as const })),
      saverBytesBefore: 120_000, saverBytesAfter: 52_000,
    };
  },
  async gatewayCatalogStatus() {
    return { ...catalog };
  },
  async gatewayCatalogRefresh() {
    catalog = { models: 342, updatedAt: Math.floor(Date.now() / 1000) };
    return { ...catalog };
  },
  async gatewayCacheCount() {
    return cacheEntries;
  },
  async gatewayCacheClear() {
    const removed = cacheEntries;
    cacheEntries = 0;
    return removed;
  },
  async onTaskStream() {
    return () => {};
  },
  async onTaskStarted() {
    return () => {};
  },
  async upstreamWatchStatus() {
    return upstreamView;
  },
  async upstreamWatchCheck() {
    const now = Math.floor(Date.now() / 1000);
    const commit = (n: number, title: string, areas: string[]) => ({
      sha: `${n}`.repeat(40), short: `${n}`.repeat(7), title, areas,
      date: new Date(Date.now() - n * 3_600_000).toISOString(),
      url: `https://github.com/Louis-CFM/coucou/commit/${`${n}`.repeat(40)}`,
    });
    upstreamView = {
      ...upstreamView,
      lastCheck: now,
      unseen: ["coucou"],
      reports: [{
        id: "coucou", repo: "Louis-CFM/coucou", pinned: PIN, pinnedDate: "2026-11-14T08:00:00Z", branch: "main",
        aheadBy: 2,
        commits: [commit(1, "feat(island): example commit", ["island"]), commit(2, "Fix a typo", [])],
        releases: [],
        areas: [{ path: "windows/src-tauri", files: 3 }, { path: "windows/src", files: 1 }],
        compareUrl: `https://github.com/Louis-CFM/coucou/compare/${PIN}...main`,
        checkedAt: now,
        error: null,
      }],
    };
    return upstreamView;
  },
  async upstreamWatchMarkSeen() {
    upstreamView = { ...upstreamView, unseen: [] };
    return upstreamView;
  },
  async upstreamOpen() {},
  async onUpstreamWatch() {
    return () => {};
  },
  async boardList() {
    requireVault();
    return [...boards.entries()]
      .map(([name, b]) => ({ name, modifiedMs: b.modifiedMs, size: b.content.length }))
      .sort((a, b) => b.modifiedMs - a.modifiedMs || a.name.localeCompare(b.name));
  },
  async boardCreate(name, scene) {
    requireVault();
    const n = boardName(name);
    if (boards.has(n)) throw `a board named "${n}" already exists`;
    boards.set(n, { content: scene ?? EMPTY_SCENE, rev: 1, modifiedMs: Date.now() });
    return boardDoc(n);
  },
  async boardOpen(name) {
    requireVault();
    return boardDoc(boardName(name));
  },
  async boardSave(name, content, baseRevision) {
    requireVault();
    const n = boardName(name);
    const b = boards.get(n);
    if (!b && baseRevision !== undefined) throw `conflict: deleted: board "${n}" no longer exists on disk`;
    if (b && baseRevision !== undefined && baseRevision !== String(b.rev)) {
      throw `conflict: board "${n}" changed on disk since it was opened`;
    }
    const rev = (b?.rev ?? 0) + 1;
    boards.set(n, { content, rev, modifiedMs: Date.now() });
    return String(rev);
  },
  async boardRename(from, to) {
    requireVault();
    const f = boardName(from);
    const t = boardName(to);
    const b = boards.get(f);
    if (!b) throw `board "${f}" not found`;
    if (boards.has(t) && f.toLowerCase() !== t.toLowerCase()) throw `a board named "${t}" already exists`;
    boards.delete(f);
    boards.set(t, b);
    return { name: t, modifiedMs: b.modifiedMs, size: b.content.length };
  },
  async boardDelete(name) {
    requireVault();
    const n = boardName(name);
    const b = boards.get(n);
    if (!b) throw `board "${n}" not found`;
    boards.delete(n);
    const path = `.trash/boards/${n}-${Date.now()}.excalidraw`;
    trash.push(path);
    return path;
  },
  async boardSketch(name) {
    requireVault();
    const scene = JSON.parse(boardDoc(boardName(name)).content) as { elements?: { isDeleted?: boolean }[]; files?: unknown };
    return { name, elements: (scene.elements ?? []).filter((e) => !e.isDeleted), files: scene.files ?? null };
  },
  async channelList() {
    return channels.map((c) => ({ ...c, config: { ...c.config } }));
  },
  async channelSave(config, token) {
    if (token !== undefined && !/^\d+:[A-Za-z0-9_-]{20,}$/.test(token.trim())) throw "that does not look like a Telegram bot token";
    if (!config.workspaceId) throw "pick the workspace channel tasks run in";
    if (config.dmPolicy === "open" && !(allowLists.get(config.id) ?? []).some((a) => a.senderId === "*")) {
      throw "the open DM policy needs an explicit `*` entry on the allow-list";
    }
    const id = config.id || `ch_mock${channels.length + 1}`;
    if (token) channelTokens.add(id);
    const hasToken = channelTokens.has(id);
    const health = !config.enabled
      ? { state: "stopped" as const, detail: "disabled", restarts: 0, since: nowSecs() }
      : hasToken
        ? { state: "connected" as const, detail: "receiving messages", restarts: 0, since: nowSecs() }
        : { state: "failed" as const, detail: "no bot token saved: paste one from @BotFather", restarts: 0, since: nowSecs() };
    const info: ChannelInfo = { config: { ...config, id }, hasToken, health };
    const index = channels.findIndex((c) => c.config.id === id);
    if (index >= 0) channels[index] = info;
    else {
      channels.push(info);
      pairings.set(id, [{ channelId: id, senderId: "424242", chatId: "424242", label: "@demo_user", code: "K7QX2M9P", createdAt: nowSecs() - 300, lastSeenAt: nowSecs() - 60 }]);
      channelLog.set(id, [{ ts: new Date().toISOString(), channelId: id, direction: "in", kind: "pairing", peer: "@demo_user (424242)", text: "hello", taskId: null }]);
    }
    return info;
  },
  async channelDelete(id) {
    const index = channels.findIndex((c) => c.config.id === id);
    if (index < 0) return false;
    channels.splice(index, 1);
    channelTokens.delete(id);
    return true;
  },
  async channelTest(id, token) {
    if (!token && !(id && channelTokens.has(id))) throw "paste a bot token first";
    await sleep(200);
    return { id: "1234567", username: "next_agent_demo_bot", name: "Next Agent (mock)" };
  },
  async channelRestart(id) {
    findChannel(id);
  },
  async channelPairings(id) {
    return (pairings.get(id) ?? []).filter((p) => p.createdAt + 3600 > nowSecs());
  },
  async channelPairingResolve(id, code, approve) {
    const list = pairings.get(id) ?? [];
    const request = list.find((p) => p.code === code.toUpperCase());
    if (!request) throw "no pending pairing request with that code (it may have expired)";
    pairings.set(id, list.filter((p) => p !== request));
    if (approve) {
      const allow = allowLists.get(id) ?? [];
      allowLists.set(id, [...allow.filter((a) => a.senderId !== request.senderId), { senderId: request.senderId, label: request.label, source: "pairing", addedAt: new Date().toISOString() }]);
    }
    return request;
  },
  async channelAllowList(id) {
    return [...(allowLists.get(id) ?? [])];
  },
  async channelAllowAdd(id, senderId, label) {
    if (!/^(\*|[A-Za-z0-9_@.:-]{1,64})$/.test(senderId.trim())) throw "enter a numeric Telegram user id (or * for anyone)";
    const allow = allowLists.get(id) ?? [];
    allowLists.set(id, [...allow.filter((a) => a.senderId !== senderId.trim()), { senderId: senderId.trim(), label, source: "manual", addedAt: new Date().toISOString() }]);
  },
  async channelAllowRemove(id, senderId) {
    if (senderId === "*" && findChannel(id).config.dmPolicy === "open") throw "switch the DM policy away from open before removing `*`";
    const allow = allowLists.get(id) ?? [];
    allowLists.set(id, allow.filter((a) => a.senderId !== senderId));
    return allow.length !== (allowLists.get(id) ?? []).length;
  },
  async channelActivity(id, limit) {
    return (channelLog.get(id) ?? []).slice(0, limit ?? 100);
  },
  async onChannelHealth() {
    return () => {};
  },
  async onChannelChange() {
    return () => {};
  },
  async boardExport(name, ext, data) {
    requireVault();
    let path = `assets/${boardName(name)}.${ext}`;
    for (let i = 1; assets.has(path); i += 1) path = `assets/${boardName(name)}-${i}.${ext}`;
    assets.set(path, data);
    return path;
  },
  async learningSettingsGet() {
    return { ...learningSettings };
  },
  async learningSettingsSet(next) {
    learningSettings = { ...next };
  },
  async memoryGet() {
    return { memory: memoryFile("memory"), user: memoryFile("user") };
  },
  async memorySave(_ws, target, text) {
    const limit = MEMORY_LIMITS[target];
    const { used } = memoryUsage(text);
    if (used > limit) throw `${fileOf(target)} would be ${used}/${limit} chars; shorten or remove entries first.`;
    memoryText[target] = text;
    return { memory: memoryFile("memory"), user: memoryFile("user") };
  },
  async skillsOverview() {
    return mockSkills.map((s) => ({ ...s }));
  },
  async skillRead(_ws, name) {
    const s = findSkill(name);
    return `---\nname: ${s.name}\ndescription: ${s.description}\n---\n# ${s.name}\n\n1. Do the thing.\n`;
  },
  async skillPin(_ws, _root, name, pinned) {
    findSkill(name).pinned = pinned;
  },
  async skillArchive(_ws, name) {
    findSkill(name).state = "archived";
  },
  async skillRestore(_ws, _root, name) {
    findSkill(name).state = "active";
  },
  async curatorRun() {
    return { checked: mockSkills.length, markedStale: [], archived: [], reactivated: [] };
  },
  async cronJobs(workspaceId) {
    return cronJobs.filter((j) => !workspaceId || j.workspaceId === workspaceId).map((j) => ({ ...j }));
  },
  async cronSave(input) {
    if (!input.schedule.trim()) throw "Schedule is required.";
    if (input.mode === "bypassPermissions") throw "Scheduled jobs never run in bypass mode.";
    const existing = input.id ? findJob(input.id) : undefined;
    const job: CronJob = {
      id: existing?.id ?? `job${(counter += 1)}`,
      workspaceId: input.workspaceId,
      name: input.name || input.prompt.slice(0, 50) || "cron job",
      prompt: input.prompt,
      schedule: input.schedule,
      scheduleLabel: input.schedule,
      skills: input.skills,
      modelRole: input.modelRole || null,
      mode: input.mode || "auto",
      workdir: input.workdir || null,
      timezone: input.timezone || null,
      graceMinutes: input.graceMinutes ?? null,
      repeat: input.repeat ?? null,
      runCount: existing?.runCount ?? 0,
      state: existing?.state ?? "scheduled",
      nextRunAt: Math.floor(Date.now() / 1000) + 3600,
      lastRunAt: existing?.lastRunAt ?? null,
      lastStatus: existing?.lastStatus ?? null,
      lastError: null,
      lastTaskId: existing?.lastTaskId ?? null,
      origin: "user",
    };
    if (existing) cronJobs.splice(cronJobs.indexOf(existing), 1, job);
    else cronJobs.push(job);
    return { ...job };
  },
  async cronPause(id, paused) {
    const job = findJob(id);
    job.state = paused ? "paused" : "scheduled";
    job.nextRunAt = paused ? null : Math.floor(Date.now() / 1000) + 3600;
    return { ...job };
  },
  async cronDelete(id) {
    const index = cronJobs.findIndex((j) => j.id === id);
    if (index >= 0) cronJobs.splice(index, 1);
    return index >= 0;
  },
  async cronRunNow(id) {
    const job = findJob(id);
    job.runCount += 1;
    job.lastRunAt = Math.floor(Date.now() / 1000);
    job.lastStatus = "started";
    job.lastTaskId = `t${(counter += 1)}`;
    return job.lastTaskId;
  },
  ...designMockApi,
};

// ── The overlay's settings (lib/overlay-settings.ts), outside Tauri ──────────
// A small in-memory stand-in for the overlay's commands, so Settings → Overlay
// renders and its controls move in plain vite dev. Writes only change memory.

type Args = Record<string, unknown> | undefined;
const overlayState = {
  settings: {
    soundEnabled: true, soundVolume: 0.12, autoCloseInterval: 15, openOnHover: false, alertsFromAllPills: false,
    showClock: false, closedIsland: "mochi", absenceInterval: 180,
    activeIntegrations: ["integration_resend", "integration_n8n", "integration_vercel", "integration_github"],
    mainPill: "integration_claude", screen: "primary", autostart: false, hooksInstalled: false, model: "claude-opus-5",
    showPlanInNotch: false, showPlanInCompactIsland: false, planRelayInstalled: false, showCodexPlanInNotch: false,
    chatProvider: "anthropic", chatModels: {}, chatSubscriptions: {}, ollamaUrl: "", lmstudioUrl: "", customUrl: "",
    shortcuts: {}, mochiOutfit: "auto", pillColors: {}, language: "", phoneLink: false,
  } as Record<string, unknown>,
  secrets: new Set<string>(["github-token"]),
  hooks: { installed: false, planRelayInstalled: false },
  agents: [
    { id: "codex", name: "Codex", installed: false, path: "~/.codex/hooks.json", hookReady: true, approvals: true, note: "Restart Codex to pick them up." },
    { id: "gemini", name: "Gemini CLI", installed: true, path: "~/.gemini/settings.json", hookReady: true, approvals: false, note: "" },
    { id: "antigravity", name: "Antigravity", installed: true, path: "~/.antigravity/hooks.json", hookReady: true, approvals: false, note: "" },
  ],
  agy: { enabled: false, hooksReady: true },
  phone: { enabled: false, details: false, answers: false, diffs: false, usage: false, recap: false, chat: false, services: [] as string[], models: [] as string[] },
  update: {
    consent: true as boolean | null, source: "", current: "0.1.0", latest: null, canInstall: false,
    phase: { kind: "idle" }, check: "none", platform: "linux",
  } as Record<string, unknown>,
  recap: true,
  flow: {
    prefs: {
      enabled: false, key: "right_ctrl", keyCombo: [], edge: "bottom", hidePill: false, mic: "", keepDays: 30, cleanup: false,
      defaultStyle: "natural", summaryEngine: "claude", openAtLaunch: true, voice: false, voiceOutput: "system", voiceGender: "female",
      muteOthers: true, oneTapHandsFree: false,
    } as Record<string, unknown>,
    models: { parakeet: { state: "missing" } as Record<string, unknown>, downloadMb: 700 },
  },
  styles: [{ id: "slack", name: "Slack", style: "natural" }],
};
const SAMPLE_DIFF = ' {\n   "hooks": {\n+    "PreToolUse": [{ "hooks": [{ "command": "next-agent-hook" }] }],\n     "Stop": []\n   }\n }';
const hookPreview = (install: boolean) => ({
  diff: install ? SAMPLE_DIFF : SAMPLE_DIFF.replace("+", "-"),
  backup: "~/.claude/settings.json.next-agent-2026-11-15.bak",
  settingsPath: "~/.claude/settings.json",
  path: "~/.claude/settings.json",
  fingerprint: "mock",
});
const flowStatus = () => ({
  ...overlayState.flow, keys: "ok", hasClaudeKey: overlayState.secrets.has("anthropic-api-key"),
  mics: [{ name: "Built-in Microphone", default: true }], platform: "linux", autostarted: false,
});
const phoneStatus = () => ({
  enabled: overlayState.phone.enabled, running: overlayState.phone.enabled, port: 47615, host: "192.168.1.20",
  name: "This computer", clients: 0, error: null, relayState: "off",
});
const pairing = () => ({
  link: "coucou://pair?mock", host: "192.168.1.20", port: 47615, name: "This computer", relay: null,
  qrSvg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><path d="M0 0h2v2H0zM2 2h2v2H2z"/></svg>',
});
const relayOff = (hasAccess = false) => ({ enabled: false, url: "", hasAccess, state: "off", shown: false });
const flag = (key: "details" | "answers" | "diffs" | "usage" | "recap") => ({
  [`phone_${key}_status`]: () => ({ enabled: overlayState.phone[key] }),
  [`phone_${key}_set_enabled`]: (a: Args) => ({ enabled: (overlayState.phone[key] = Boolean(a?.enabled)) }),
});

const overlayCommands: Record<string, (a: Args) => unknown> = {
  boot: () => ({
    settings: { ...overlayState.settings, hooksInstalled: overlayState.hooks.installed, planRelayInstalled: overlayState.hooks.planRelayInstalled },
    screen: { x: 0, y: 0, width: 1440, height: 900, scale: 2 }, version: "0.1.0", hookPath: "", cursorPoll: true,
  }),
  save_settings: (a) => void (overlayState.settings = { ...(a?.settings as object) }),
  list_monitors: () => [{ key: "at:0,0|Built-in Retina Display", label: "Built-in Retina Display" }],
  custom_sounds: () => [],
  reveal_sounds_folder: () => undefined,
  reload_sounds: () => undefined,
  open_url: () => undefined,
  recap_prefs: () => ({ enabled: overlayState.recap, hideProjects: false }),
  recap_set_enabled: (a) => void (overlayState.recap = Boolean(a?.enabled)),
  recap_clear: () => undefined,
  update_status: () => ({ ...overlayState.update }),
  update_set_consent: (a) => void (overlayState.update.consent = Boolean(a?.allowed)),
  update_check: () => ({ ...overlayState.update, check: overlayState.update.source ? "upToDate" : "none" }),
  update_set_source: (a) => {
    const source = String(a?.source ?? "");
    if (source && !/^[\w.-]+\/[\w.-]+$/.test(source)) throw "not a GitHub owner/repo";
    overlayState.update.source = source;
    return { ...overlayState.update };
  },
  update_show: () => undefined,
  update_open_page: () => undefined,
  hooks_status: () => ({
    ...overlayState.hooks, settingsPath: "~/.claude/settings.json",
    hookPath: "~/Library/Application Support/dev.nextagent.desktop/next-agent-hook", hookReady: true,
  }),
  agent_hooks_status: () => ({ agent_gemini: true }),
  hooks_preview: (a) => hookPreview(Boolean(a?.install)),
  hooks_apply: (a) => {
    overlayState.hooks.installed = Boolean(a?.install);
    return "~/.claude/settings.json.next-agent-2026-11-15.bak";
  },
  agent_hooks_list: () => overlayState.agents.map((x) => ({ ...x })),
  agent_hooks_preview: (a) => hookPreview(Boolean(a?.install)),
  agent_hooks_apply: (a) => {
    const agent = overlayState.agents.find((x) => x.id === a?.agent);
    if (agent) agent.installed = Boolean(a?.install);
    return "";
  },
  agy_approvals_status: () => ({ ...overlayState.agy }),
  agy_approvals_set_enabled: (a) => ({ ...(overlayState.agy = { ...overlayState.agy, enabled: Boolean(a?.enabled) }) }),
  status_line_preview: (a) => hookPreview(Boolean(a?.install)),
  status_line_apply: (a) => {
    overlayState.hooks.planRelayInstalled = Boolean(a?.install);
    return "";
  },
  secret_present: (a) => overlayState.secrets.has(String(a?.key)),
  secret_set: (a) => {
    if (a?.value) overlayState.secrets.add(String(a.key));
    else overlayState.secrets.delete(String(a?.key));
  },
  secret_clear: (a) => void overlayState.secrets.delete(String(a?.key)),
  claude_ready: () => overlayState.secrets.has("anthropic-api-key"),
  claude_api_models: () => [{ id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5" }, { id: "claude-opus-5-5", label: "Claude Opus 5.5" }],
  chat_models: () => [{ id: "gpt-5", label: "GPT-5" }],
  subscription_status: () => [
    {
      provider: "anthropic", tool: "Claude Code", installed: true, signIn: "claude", installUrl: "https://claude.com/claude-code", subscriptionOnly: false,
      on: Boolean((overlayState.settings.chatSubscriptions as Record<string, boolean>).anthropic),
    },
    { provider: "openai", tool: "Codex CLI", installed: false, on: false, signIn: "codex login", installUrl: "https://developers.openai.com/codex", subscriptionOnly: false },
  ],
  subscription_set: (a) => {
    overlayState.settings.chatSubscriptions = { ...(overlayState.settings.chatSubscriptions as object), [String(a?.provider)]: Boolean(a?.on) };
    return overlayCommands.subscription_status(undefined);
  },
  local_set_key: () => undefined,
  local_connect: (a) => {
    throw `Nothing answers at ${String(a?.url) || "the usual address"}.`;
  },
  shortcuts_status: () => ({ actions: [], blocked: null, command: "next-agent --shortcut", portal: null }),
  shortcuts_suspend: () => undefined,
  phone_link_status: phoneStatus,
  phone_link_set_enabled: (a) => {
    overlayState.phone.enabled = Boolean(a?.enabled);
    return phoneStatus();
  },
  phone_link_pairing: pairing,
  phone_link_new_pairing: pairing,
  ...flag("details"),
  ...flag("answers"),
  ...flag("diffs"),
  ...flag("usage"),
  ...flag("recap"),
  phone_services_status: () => ({ services: [...overlayState.phone.services] }),
  phone_services_set: (a) => ({ services: (overlayState.phone.services = [...((a?.services as string[]) ?? [])]) }),
  phone_relay_status: () => relayOff(),
  phone_relay_set: () => relayOff(),
  phone_relay_set_access: () => relayOff(true),
  phone_chat_status: () => ({ enabled: overlayState.phone.chat, models: [...overlayState.phone.models] }),
  phone_chat_set_enabled: (a) => ({ enabled: (overlayState.phone.chat = Boolean(a?.enabled)), models: [...overlayState.phone.models] }),
  phone_chat_set_models: (a) => ({ enabled: overlayState.phone.chat, models: (overlayState.phone.models = [...((a?.models as string[]) ?? [])]) }),
  flow_status: flowStatus,
  flow_set_prefs: (a) => void (overlayState.flow.prefs = { ...(a?.prefs as object) }),
  flow_clear: () => undefined,
  flow_styles: () => overlayState.styles.map((s) => ({ ...s })),
  flow_set_styles: (a) => void (overlayState.styles = [...((a?.styles as typeof overlayState.styles) ?? [])]),
  flow_running_apps: () => [{ id: "code", name: "Visual Studio Code" }, { id: "mail", name: "Mail" }],
  flow_paste_again: () => undefined,
  flow_models_download: () => void (overlayState.flow.models.parakeet = { state: "ready" }),
  flow_models_remove: () => void (overlayState.flow.models.parakeet = { state: "missing" }),
};

/** One overlay command, answered from memory. Unknown commands fail, like a missing Tauri command. */
export async function overlayMock(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const handler = overlayCommands[cmd];
  if (!handler) throw new Error(`no mock for ${cmd}`);
  return handler(args);
}
