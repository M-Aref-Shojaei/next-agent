// Typed IPC client. Inside Tauri it calls the Rust commands; in a plain browser
// (vite dev, screenshots) it falls back to an in-memory mock so the UI is usable.
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DiffAction } from "./diff.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";
import { mockApi } from "./mock-api.ts";
import type {
  Combo,
  ConnectionInput,
  ConnectionTest,
  GatewaySettings,
  GatewayTarget,
  ProviderConnection,
  ProviderPreset,
  UsageSummary,
  CatalogStatus,
} from "./gateway.ts";

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

export interface RouterHealthReport {
  status: "ok" | "degraded" | "down";
  detail: string;
  latencyMs: number | null;
  checks: number;
}
export interface ReviewOutcome {
  action: DiffAction;
  commit: string | null;
  files: string[];
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
  approvalRespond(taskId: string, approvalId: string, choice: ApprovalChoice): Promise<void>;
  reviewAction(taskId: string, action: DiffAction): Promise<ReviewOutcome>;
  routerHealth(): Promise<RouterHealthReport | null>;
  /** Subscribe to router status changes; returns an unsubscribe function. */
  onRouterHealth(callback: (report: RouterHealthReport) => void): Promise<() => void>;
  // Built-in LLM gateway (Settings → Providers, Usage).
  gatewayPresets(): Promise<ProviderPreset[]>;
  gatewayConnections(): Promise<ProviderConnection[]>;
  gatewayConnectionSave(input: ConnectionInput): Promise<ProviderConnection>;
  gatewayConnectionDelete(id: string): Promise<boolean>;
  gatewayConnectionTest(id: string): Promise<ConnectionTest>;
  gatewayCombos(): Promise<Combo[]>;
  gatewayComboSave(input: Combo): Promise<Combo>;
  gatewayComboDelete(id: string): Promise<boolean>;
  gatewaySettingsGet(): Promise<GatewaySettings>;
  gatewaySettingsSet(settings: GatewaySettings): Promise<void>;
  gatewayTargets(): Promise<GatewayTarget[]>;
  usageSummary(days: number): Promise<UsageSummary>;
  gatewayCatalogStatus(): Promise<CatalogStatus>;
  /** Manual refresh only: downloads OpenRouter's public model list. */
  gatewayCatalogRefresh(): Promise<CatalogStatus>;
  gatewayCacheCount(): Promise<number>;
  gatewayCacheClear(): Promise<number>;
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
  approvalRespond: (taskId, approvalId, choice) =>
    invoke("approval_respond", { taskId, approvalId, choice }),
  reviewAction: (taskId, action) => invoke("review_action", { taskId, action }),
  routerHealth: () => invoke("router_health"),
  onRouterHealth: (callback) =>
    listen<RouterHealthReport | null>("router-health", (event) => {
      if (event.payload) callback(event.payload);
    }),
  gatewayPresets: () => invoke("gateway_presets"),
  gatewayConnections: () => invoke("gateway_connections"),
  gatewayConnectionSave: (input) => invoke("gateway_connection_save", { input }),
  gatewayConnectionDelete: (id) => invoke("gateway_connection_delete", { id }),
  gatewayConnectionTest: (id) => invoke("gateway_connection_test", { id }),
  gatewayCombos: () => invoke("gateway_combos"),
  gatewayComboSave: (input) => invoke("gateway_combo_save", { input }),
  gatewayComboDelete: (id) => invoke("gateway_combo_delete", { id }),
  gatewaySettingsGet: () => invoke("gateway_settings_get"),
  gatewaySettingsSet: (settings) => invoke("gateway_settings_set", { settings }),
  gatewayTargets: () => invoke("gateway_targets"),
  usageSummary: (days) => invoke("usage_summary", { days }),
  gatewayCatalogStatus: () => invoke("gateway_catalog_status"),
  gatewayCatalogRefresh: () => invoke("gateway_catalog_refresh"),
  gatewayCacheCount: () => invoke("gateway_cache_count"),
  gatewayCacheClear: () => invoke("gateway_cache_clear"),
};

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api: Api = inTauri() ? tauriApi : mockApi;
