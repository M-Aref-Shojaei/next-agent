// Typed IPC client. Inside Tauri it calls the Rust commands; in a plain browser
// (vite dev, screenshots) it falls back to an in-memory mock so the UI is usable.
import { invoke, Channel } from "@tauri-apps/api/core";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";
import { mockApi } from "./mock-api.ts";

export interface Workspace {
  id: string;
  path: string;
  name: string;
}
export interface Rule {
  id: string;
  scope: string;
  effect: string;
  kind: string;
  pattern: string;
}
export interface AuditStatus {
  ok: boolean;
  checked: number;
  broken_at: number | null;
}

export interface Api {
  workspaceOpen(path: string): Promise<Workspace>;
  workspaceList(): Promise<Workspace[]>;
  vaultSet(path: string): Promise<string>;
  vaultGet(): Promise<string | null>;
  settingGet(key: string): Promise<string | null>;
  settingSet(key: string, value: string): Promise<void>;
  rulesList(workspaceId: string): Promise<Rule[]>;
  ruleDelete(id: string): Promise<boolean>;
  auditVerify(): Promise<AuditStatus>;
  taskStart(
    workspaceId: string,
    prompt: string,
    mode: string,
    onMessage: (message: StreamMessage) => void,
  ): Promise<string>;
  taskCancel(taskId: string): Promise<void>;
  approvalRespond(approvalId: string, choice: ApprovalChoice): Promise<void>;
}

const tauriApi: Api = {
  workspaceOpen: (path) => invoke("workspace_open", { path }),
  workspaceList: () => invoke("workspace_list"),
  vaultSet: (path) => invoke("vault_set", { path }),
  vaultGet: () => invoke("vault_get"),
  settingGet: (key) => invoke("setting_get", { key }),
  settingSet: (key, value) => invoke("setting_set", { key, value }),
  rulesList: (workspaceId) => invoke("rules_list", { workspaceId }),
  ruleDelete: (id) => invoke("rule_delete", { id }),
  auditVerify: () => invoke("audit_verify"),
  taskStart: (workspaceId, prompt, mode, onMessage) => {
    const onEvent = new Channel<StreamMessage>();
    onEvent.onmessage = onMessage;
    return invoke("task_start", { workspaceId, prompt, mode, onEvent });
  },
  taskCancel: (taskId) => invoke("task_cancel", { taskId }),
  approvalRespond: (approvalId, choice) => invoke("approval_respond", { approvalId, choice }),
};

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api: Api = inTauri() ? tauriApi : mockApi;
