// In-memory stand-in for the Rust commands, used outside Tauri.
import type { Api, BoardDoc, Workspace } from "./api.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";

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
    if (boards.has(t)) throw `a board named "${t}" already exists`;
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
