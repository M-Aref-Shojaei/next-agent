// In-memory stand-in for the Rust commands, used outside Tauri.
import type { Api, BoardDoc, Workspace } from "./api.ts";
import { mockGraphApi } from "./notes/mock-graph.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";
import {
  defaultSettings,
  type Combo,
  type GatewaySettings,
  type ProviderConnection,
  type ProviderPreset,
  type UsageTotals,
} from "./gateway.ts";

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

export const mockApi: Api = {
  ...mockGraphApi, // notes-ui
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
  async overlayInfo() {
    return {
      settings: { enabled: true, position: "top-center" as const, monitor: null },
      monitors: [{ key: "display:MOCK", label: "Mock display" }],
      island: {
        hasNotch: true,
        width: 185,
        height: 32,
        compactExtra: 160,
        panel: [720, 560] as [number, number],
        position: "top-center" as const,
        platform: "macos" as const,
      },
    };
  },
  async overlaySetSettings() {},
  async overlaySetRect() {},
  async overlaySetExpanded() {},
  async overlaySetCollapsed() {},
  async overlayOpenTask() {},
  async onTaskStream() {
    return () => {};
  },
  async onTaskStarted() {
    return () => {};
  },
  async onOverlayPointer() {
    return () => {};
  },
  async onOverlayGeometry() {
    return () => {};
  },
  async onFocusTask() {
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
  async boardExport(name, ext, data) {
    requireVault();
    let path = `assets/${boardName(name)}.${ext}`;
    for (let i = 1; assets.has(path); i += 1) path = `assets/${boardName(name)}-${i}.${ext}`;
    assets.set(path, data);
    return path;
  },
};
