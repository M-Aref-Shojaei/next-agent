// In-memory stand-in for the Rust commands, used outside Tauri.
import type { Api, Workspace } from "./api.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";

const workspaces: Workspace[] = [];
const settings = new Map<string, string>([["router.url", "http://127.0.0.1:20128/v1"]]);
let vault: string | null = null;
const waiting = new Map<string, (choice: ApprovalChoice) => void>();
let counter = 0;

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
};
