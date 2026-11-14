// Typed IPC client. Inside Tauri it calls the Rust commands; in a plain browser
// (vite dev, screenshots) it falls back to an in-memory mock so the UI is usable.
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { IslandInfo } from "./island/layout.ts";
import type { DiffAction } from "./diff.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";
import { mockApi } from "./mock-api.ts";
import type {
  Backlink,
  Backlinks,
  BlockNode,
  GraphData,
  BlockRef,
  GraphInfo,
  JournalEntry,
  PageListItem,
  PageView,
  SearchHit,
} from "./notes/types.ts";
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

export interface OverlaySettings {
  enabled: boolean;
  position: "top-center" | "top-left" | "top-right";
  /** `notch` (default), `menuBar`, `followMouse` or `display:<id>`; null = default. */
  monitor: string | null;
}
export interface OverlayMonitor {
  key: string;
  label: string;
}
export interface OverlayInfo {
  settings: OverlaySettings;
  monitors: OverlayMonitor[];
  /** The resting island on the current screen, and the panel around it. */
  island: IslandInfo;
}
export interface AgentProfileInfo {
  name: string;
  description: string;
  builtin: boolean;
}
export interface ProfileList {
  profiles: AgentProfileInfo[];
  warnings: string[];
}
export interface UndoOutcome {
  seq: number;
  call_id: string;
  tool: string;
  restored: string[];
  remaining: number;
}
export interface OverlayRect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface TaskStarted {
  taskId: string;
  title: string;
  mode: string;
}
export interface BoardMeta {
  name: string;
  modifiedMs: number;
  size: number;
}
export interface BoardDoc {
  name: string;
  content: string;
  revision: string;
}
export interface BoardSketchData {
  name: string;
  elements: unknown[];
  files: unknown;
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
    profile?: string,
  ): Promise<string>;
  taskCancel(taskId: string): Promise<void>;
  /** Agent profiles a task can run as (built-ins + `.next-agent/agents/*.md`). */
  agentProfiles(workspaceId: string): Promise<ProfileList>;
  /** Revert the last mutating step of a finished task. */
  taskUndoStep(taskId: string): Promise<UndoOutcome>;
  taskUndoable(taskId: string): Promise<number>;
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
  overlayInfo(): Promise<OverlayInfo>;
  overlaySetSettings(settings: OverlaySettings): Promise<void>;
  /** Island rectangle (CSS px, window-relative) used for click-through. */
  overlaySetRect(rect: OverlayRect | null): Promise<void>;
  overlaySetExpanded(expanded: boolean): Promise<void>;
  /** PC: fold the window to the hidden island (size), or back to the panel (null). */
  overlaySetCollapsed(size: [number, number] | null): Promise<void>;
  /** Focus the main window on a task. */
  overlayOpenTask(taskId: string): Promise<void>;
  /** Every sequenced task event, broadcast to all windows. */
  onTaskStream(callback: (message: StreamMessage) => void): Promise<() => void>;
  onTaskStarted(callback: (started: TaskStarted) => void): Promise<() => void>;
  /** Pointer entered/left the overlay island (the window is click-through otherwise). */
  onOverlayPointer(callback: (inside: boolean) => void): Promise<() => void>;
  /** The island's screen or placement changed (re-measured resting island). */
  onOverlayGeometry(callback: (island: IslandInfo) => void): Promise<() => void>;
  /** The overlay asked the main window to show a task. */
  onFocusTask(callback: (taskId: string) => void): Promise<() => void>;
  // Design boards: vault `boards/*.excalidraw`. Save errors starting with "conflict:" mean the file changed on disk.
  boardList(): Promise<BoardMeta[]>;
  boardCreate(name: string, scene?: string): Promise<BoardDoc>;
  boardOpen(name: string): Promise<BoardDoc>;
  boardSave(name: string, content: string, baseRevision?: string): Promise<string>;
  boardRename(from: string, to: string): Promise<BoardMeta>;
  /** Moves the board to the vault trash; returns the trash path. */
  boardDelete(name: string): Promise<string>;
  boardSketch(name: string): Promise<BoardSketchData>;
  /** `data` is base64 for png, text for svg. Returns the vault-relative path. */
  boardExport(name: string, ext: "png" | "svg", data: string): Promise<string>;
  // --- notes-ui --- Notes (Logseq graph in the vault): see src-tauri/src/notes.rs.
  graphOpen(path: string, create?: boolean): Promise<GraphInfo>;
  graphSearch(q: string, limit?: number): Promise<SearchHit[]>;
  graphPage(name: string): Promise<PageView>;
  graphBacklinks(name: string): Promise<Backlinks>;
  graphJournals(limit?: number): Promise<JournalEntry[]>;
  graphSavePage(name: string, tree: BlockNode[]): Promise<PageView>;
  graphPages(): Promise<PageListItem[]>;
  graphGraph(): Promise<GraphData>;
  /** `{{query …}}` text (with or without the macro braces); `today` is `yyyyMMdd`. */
  graphQuery(text: string, today: number): Promise<Backlink[]>;
  /** Journal page name of a `yyyyMMdd` day, in the graph's title format. */
  graphJournalName(day: number): Promise<string>;
  /** The block with `id:: uuid`, or null. */
  graphBlock(uuid: string): Promise<BlockRef | null>;
  /** Graph-relative files the watcher re-indexed (also fired after our own saves). */
  onGraphChanged(callback: (files: string[]) => void): Promise<() => void>;
  // --- end notes-ui ---
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
  taskStart: (workspaceId, prompt, mode, onMessage, profile) => {
    const onEvent = new Channel<StreamMessage>();
    onEvent.onmessage = onMessage;
    return invoke("task_start", { workspaceId, prompt, mode, onEvent, profile: profile ?? null });
  },
  agentProfiles: (workspaceId) => invoke("agent_profiles", { workspaceId }),
  taskUndoStep: (taskId) => invoke("task_undo_step", { taskId }),
  taskUndoable: (taskId) => invoke("task_undoable", { taskId }),
  taskCancel: (taskId) => invoke("task_cancel", { taskId }),
  approvalRespond: (taskId, approvalId, choice) =>
    invoke("approval_respond", { taskId, approvalId, choice }),
  reviewAction: (taskId, action) => invoke("review_action", { taskId, action }),
  routerHealth: () => invoke("router_health"),
  boardList: () => invoke("board_list"),
  boardCreate: (name, scene) => invoke("board_create", { name, scene: scene ?? null }),
  boardOpen: (name) => invoke("board_open", { name }),
  boardSave: (name, content, baseRevision) =>
    invoke("board_save", { name, content, baseRevision: baseRevision ?? null }),
  boardRename: (from, to) => invoke("board_rename", { from, to }),
  boardDelete: (name) => invoke("board_delete", { name }),
  boardSketch: (name) => invoke("board_sketch", { name }),
  boardExport: (name, ext, data) => invoke("board_export", { name, ext, data }),
  // --- notes-ui ---
  graphOpen: (path, create) => invoke("graph_open", { path, create: create ?? null }),
  graphSearch: (q, limit) => invoke("graph_search", { q, limit: limit ?? null }),
  graphPage: (name) => invoke("graph_page", { name }),
  graphBacklinks: (name) => invoke("graph_backlinks", { name }),
  graphJournals: (limit) => invoke("graph_journals", { limit: limit ?? null }),
  graphSavePage: (name, tree) => invoke("graph_save_page", { name, tree }),
  graphPages: () => invoke("graph_pages"),
  graphGraph: () => invoke("graph_graph"),
  graphQuery: (text, today) => invoke("graph_query", { text, today }),
  graphBlock: (uuid) => invoke("graph_block", { uuid }),
  graphJournalName: (day) => invoke("graph_journal_name", { day }),
  onGraphChanged: (callback) => listen<string[]>("graph-changed", (e) => callback(e.payload)),
  // --- end notes-ui ---
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
  overlayInfo: () => invoke("overlay_info"),
  overlaySetSettings: (settings) => invoke("overlay_set_settings", { settings }),
  overlaySetRect: (rect) => invoke("overlay_set_rect", { rect }),
  overlaySetExpanded: (expanded) => invoke("overlay_set_expanded", { expanded }),
  overlaySetCollapsed: (size) => invoke("overlay_set_collapsed", { size }),
  overlayOpenTask: (taskId) => invoke("overlay_open_task", { taskId }),
  onTaskStream: (callback) => listen<StreamMessage>("task-stream", (e) => callback(e.payload)),
  onTaskStarted: (callback) => listen<TaskStarted>("task-started", (e) => callback(e.payload)),
  onOverlayPointer: (callback) => listen<boolean>("overlay-pointer", (e) => callback(e.payload)),
  onOverlayGeometry: (callback) => listen<IslandInfo>("overlay-geometry", (e) => callback(e.payload)),
  onFocusTask: (callback) => listen<string>("focus-task", (e) => callback(e.payload)),
};

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api: Api = inTauri() ? tauriApi : mockApi;
