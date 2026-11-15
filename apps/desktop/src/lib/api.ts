// Typed IPC client. Inside Tauri it calls the Rust commands; in a plain browser
// (vite dev, screenshots) it falls back to an in-memory mock so the UI is usable.
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
import type { WatchView } from "./upstreams.ts";
import type { CronInput, CronJob, CuratorReport, LearningSettings, MemoryState, SkillInfo } from "./learning.ts";
import type {
  AllowEntry,
  BotIdentity,
  ChannelActivity,
  ChannelConfig,
  ChannelHealth,
  ChannelInfo,
  PairingRequest,
} from "./channels.ts";
import type {
  DesignBatch,
  DesignDirection,
  DesignFiles,
  DesignProgress,
  DesignTask,
  FrameKind,
  Framework,
  PreviewInfo,
  RawLocation,
  SourceView,
  StyleChange,
} from "./design.ts";

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
export interface TaskStarted {
  taskId: string;
  title: string;
  mode: string;
  /** `cron` for scheduled runs started by the app itself. */
  source?: string;
  /** Set for tasks that came from a chat channel, e.g. "Telegram · @alice". */
  origin?: string;
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
  /** Settings → Overlay: open Coucou's own settings window (on `page`). */
  overlayOpen(page?: string): Promise<void>;
  // Settings → Upstreams: report-only watch of upstream GitHub repositories.
  upstreamWatchStatus(): Promise<WatchView>;
  /** "Check now": asks GitHub unless its rate limit still applies. */
  upstreamWatchCheck(): Promise<WatchView>;
  upstreamWatchMarkSeen(): Promise<WatchView>;
  /** Opens a watched upstream's GitHub page in the browser (others are refused). */
  upstreamOpen(url: string): Promise<void>;
  onUpstreamWatch(callback: (view: WatchView) => void): Promise<() => void>;
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
  // Memory and skills (Settings → Memory and skills), ported from Hermes Agent.
  learningSettingsGet(): Promise<LearningSettings>;
  learningSettingsSet(settings: LearningSettings): Promise<void>;
  memoryGet(workspaceId: string): Promise<MemoryState>;
  memorySave(workspaceId: string, target: "memory" | "user", text: string): Promise<MemoryState>;
  skillsOverview(workspaceId: string): Promise<SkillInfo[]>;
  skillRead(workspaceId: string, name: string): Promise<string>;
  skillPin(workspaceId: string, root: string, name: string, pinned: boolean): Promise<void>;
  /** Moves the skill to `.archive/` (never deletes). */
  skillArchive(workspaceId: string, name: string): Promise<void>;
  skillRestore(workspaceId: string, root: string, name: string): Promise<void>;
  curatorRun(): Promise<CuratorReport>;
  cronJobs(workspaceId?: string): Promise<CronJob[]>;
  cronSave(input: CronInput): Promise<CronJob>;
  cronPause(id: string, paused: boolean): Promise<CronJob>;
  cronDelete(id: string): Promise<boolean>;
  /** Starts the job now as its own task; returns the task id. */
  cronRunNow(id: string): Promise<string>;
  /** Every task's stream, as an app event (scheduled runs use it). */
  onTaskStream(callback: (message: StreamMessage) => void): Promise<() => void>;
  onTaskStarted(callback: (started: TaskStarted) => void): Promise<() => void>;
  // Chat channels (Settings → Channels). Tokens go to the OS keychain and never come back.
  channelList(): Promise<ChannelInfo[]>;
  /** `token` only when the owner pasted a new one. */
  channelSave(config: ChannelConfig, token?: string): Promise<ChannelInfo>;
  channelDelete(id: string): Promise<boolean>;
  /** Test a pasted token, or the saved one for `id`. */
  channelTest(id: string | null, token?: string): Promise<BotIdentity>;
  channelRestart(id: string): Promise<void>;
  channelPairings(id: string): Promise<PairingRequest[]>;
  /** Approve or deny a pairing code (desktop only). */
  channelPairingResolve(id: string, code: string, approve: boolean): Promise<PairingRequest>;
  channelAllowList(id: string): Promise<AllowEntry[]>;
  channelAllowAdd(id: string, senderId: string, label: string): Promise<void>;
  channelAllowRemove(id: string, senderId: string): Promise<boolean>;
  channelActivity(id: string, limit?: number): Promise<ChannelActivity[]>;
  onChannelHealth(callback: (channelId: string, health: ChannelHealth) => void): Promise<() => void>;
  /** A pairing request or new activity on a channel (refresh the page). */
  onChannelChange(callback: (channelId: string) => void): Promise<() => void>;
  // Design Studio (crates/design, src-tauri/src/design.rs).
  /** DESIGN.md + tokens.css, or a draft from the project's CSS when unsaved. */
  designSystemGet(workspaceId: string): Promise<DesignFiles>;
  /** Read the project's CSS and write both files. Errors starting "exists:" need `overwrite`. */
  designSystemGenerate(workspaceId: string, overwrite: boolean): Promise<DesignFiles>;
  designSystemSave(workspaceId: string, designMd: string, tokensCss: string): Promise<DesignFiles>;
  designGenerate(
    workspaceId: string,
    prompt: string,
    frame: FrameKind,
    count: number,
    board: string | null,
    critique: boolean,
    onProgress: (p: DesignProgress) => void,
  ): Promise<DesignBatch>;
  designHistory(workspaceId: string): Promise<DesignDirection[]>;
  designChoose(id: string): Promise<void>;
  designIterate(id: string, message: string): Promise<DesignDirection>;
  /** `url` null = start the workspace's dev server. `studioOrigin` = window.location.origin. */
  designPreviewStart(workspaceId: string, url: string | null, studioOrigin: string): Promise<PreviewInfo>;
  designPreviewStop(workspaceId: string): Promise<void>;
  designPreviewLog(workspaceId: string): Promise<string[]>;
  designSource(workspaceId: string, location: RawLocation): Promise<SourceView>;
  /** Writes a previewed style change as a task in a worktree (review + merge in Tasks). */
  designApplyStyle(
    workspaceId: string,
    location: RawLocation,
    changes: StyleChange[],
    onMessage: (message: StreamMessage) => void,
  ): Promise<DesignTask>;
  designConvert(id: string, framework: Framework | null, onMessage: (message: StreamMessage) => void): Promise<DesignTask>;
  designImplement(id: string, onMessage: (message: StreamMessage) => void): Promise<DesignTask>;
}

function channel<T>(on: (m: T) => void): Channel<T> {
  const c = new Channel<T>();
  c.onmessage = on;
  return c;
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
  learningSettingsGet: () => invoke("learning_settings_get"),
  learningSettingsSet: (settings) => invoke("learning_settings_set", { settings }),
  memoryGet: (workspaceId) => invoke("memory_get", { workspaceId }),
  memorySave: (workspaceId, target, text) => invoke("memory_save", { workspaceId, target, text }),
  skillsOverview: (workspaceId) => invoke("skills_overview", { workspaceId }),
  skillRead: (workspaceId, name) => invoke("skill_read", { workspaceId, name }),
  skillPin: (workspaceId, root, name, pinned) => invoke("skill_pin", { workspaceId, root, name, pinned }),
  skillArchive: (workspaceId, name) => invoke("skill_archive", { workspaceId, name }),
  skillRestore: (workspaceId, root, name) => invoke("skill_restore", { workspaceId, root, name }),
  curatorRun: () => invoke("curator_run"),
  cronJobs: (workspaceId) => invoke("cron_jobs", { workspaceId: workspaceId ?? null }),
  cronSave: (input) => invoke("cron_save", { input }),
  cronPause: (id, paused) => invoke("cron_pause", { id, paused }),
  cronDelete: (id) => invoke("cron_delete", { id }),
  cronRunNow: (id) => invoke("cron_run_now", { id }),
  onTaskStream: (callback) => listen<StreamMessage>("task-stream", (e) => callback(e.payload)),
  onTaskStarted: (callback) => listen<TaskStarted>("task-started", (e) => callback(e.payload)),
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
  overlayOpen: (page) => invoke("overlay_open", { page: page ?? null }),
  upstreamWatchStatus: () => invoke("upstream_watch_status"),
  upstreamWatchCheck: () => invoke("upstream_watch_check"),
  upstreamWatchMarkSeen: () => invoke("upstream_watch_mark_seen"),
  upstreamOpen: (url) => invoke("upstream_open", { url }),
  onUpstreamWatch: (callback) => listen<WatchView>("upstream-watch", (event) => callback(event.payload)),
  channelList: () => invoke("channel_list"),
  channelSave: (config, token) => invoke("channel_save", { input: { config, token: token ?? null } }),
  channelDelete: (id) => invoke("channel_delete", { id }),
  channelTest: (id, token) => invoke("channel_test", { id, token: token ?? null }),
  channelRestart: (id) => invoke("channel_restart", { id }),
  channelPairings: (id) => invoke("channel_pairings", { id }),
  channelPairingResolve: (id, code, approve) => invoke("channel_pairing_resolve", { id, code, approve }),
  channelAllowList: (id) => invoke("channel_allow_list", { id }),
  channelAllowAdd: (id, senderId, label) => invoke("channel_allow_add", { id, senderId, label }),
  channelAllowRemove: (id, senderId) => invoke("channel_allow_remove", { id, senderId }),
  channelActivity: (id, limit) => invoke("channel_activity", { id, limit: limit ?? null }),
  onChannelHealth: (callback) =>
    listen<{ channelId: string; health: ChannelHealth }>("channel-health", (e) => callback(e.payload.channelId, e.payload.health)),
  onChannelChange: async (callback) => {
    const offs = await Promise.all([
      listen<{ channelId: string }>("channel-pairing", (e) => callback(e.payload.channelId)),
      listen<{ channelId: string }>("channel-activity", (e) => callback(e.payload.channelId)),
    ]);
    return () => offs.forEach((off) => off());
  },
  designSystemGet: (workspaceId) => invoke("design_system_get", { workspaceId }),
  designSystemGenerate: (workspaceId, overwrite) => invoke("design_system_generate", { workspaceId, overwrite }),
  designSystemSave: (workspaceId, designMd, tokensCss) => invoke("design_system_save", { workspaceId, designMd, tokensCss }),
  designGenerate: (workspaceId, prompt, frame, count, board, critique, onProgress) =>
    invoke("design_generate", { workspaceId, prompt, frame, count, board, critique, onProgress: channel(onProgress) }),
  designHistory: (workspaceId) => invoke("design_history", { workspaceId }),
  designChoose: (id) => invoke("design_choose", { id }),
  designIterate: (id, message) => invoke("design_iterate", { id, message }),
  designPreviewStart: (workspaceId, url, studioOrigin) => invoke("design_preview_start", { workspaceId, url, studioOrigin }),
  designPreviewStop: (workspaceId) => invoke("design_preview_stop", { workspaceId }),
  designPreviewLog: (workspaceId) => invoke("design_preview_log", { workspaceId }),
  designSource: (workspaceId, location) => invoke("design_source", { workspaceId, location }),
  designApplyStyle: (workspaceId, location, changes, onMessage) =>
    invoke("design_apply_style", { workspaceId, location, changes, onEvent: channel(onMessage) }),
  designConvert: (id, framework, onMessage) => invoke("design_convert", { id, framework, onEvent: channel(onMessage) }),
  designImplement: (id, onMessage) => invoke("design_implement", { id, onEvent: channel(onMessage) }),
};

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api: Api = inTauri() ? tauriApi : mockApi;
