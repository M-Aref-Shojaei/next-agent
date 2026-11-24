// Typed IPC client. Inside Tauri it calls the Rust commands; in a plain browser
// (vite dev, screenshots) it falls back to an in-memory mock so the UI is usable.
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DiffAction } from "./diff.ts";
import type { ApprovalChoice, StreamMessage } from "./timeline.ts";
import { mockApi } from "./mock-api.ts";
import { commandError, log, setLogSink, type LogEntry, type LogLevel } from "./log.ts";
export type { LogEntry, LogLevel } from "./log.ts";
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

export interface ProjectEntry {
  name: string;
  children: ProjectEntry[] | null;
}

export interface WorkspaceDiscovery {
  agentInstructions: string | null;
  designMd: string | null;
  tokensCss: string | null;
  vaultPath: string | null;
  files: ProjectEntry[];
  truncated: boolean;
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
  /** Set when the merge worked but the task record could not be updated. */
  notice?: string | null;
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
  workspaceCreate(parent: string, name: string): Promise<Workspace>;
  workspaceDiscover(workspaceId: string): Promise<WorkspaceDiscovery>;
  showOpenFilePicker(): Promise<string | null>;
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
  /** Recent diagnostics entries, newest first (ADR-0013). */
  diagRecent(filter?: { level?: LogLevel; target?: string; limit?: number }): Promise<LogEntry[]>;
  diagSetLevel(level: LogLevel): Promise<void>;
  diagOpenFolder(): Promise<void>;
  /** Writes a redacted export to Downloads. */
  diagExport(): Promise<{ path: string; entries: number }>;
  /** For the UI's own logger (lib/log.ts); Rust allowlists and redacts again. */
  diagLog(level: LogLevel, event: string, fields: Record<string, unknown>): Promise<void>;
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

const SLOW_COMMAND_MS = 2000;

/**
 * Commands that are slow by design (agent tasks, design generation, catalogue
 * refreshes, exports, graph opening). Going over SLOW_COMMAND_MS is expected for
 * them, so it is not logged as `slow_command`. Their failures still are.
 */
export const LONG_RUNNING_COMMANDS: ReadonlySet<string> = new Set([
  "task_start",
  "design_generate",
  "design_iterate",
  "design_convert",
  "design_implement",
  "gateway_catalog_refresh",
  "gateway_connection_test",
  "upstream_watch_check",
  "curator_run",
  "cron_run_now",
  "board_export",
  "graph_open",
]);

/** True when a slow run of `command` should be logged as `slow_command`. */
export function isSlowCommand(command: string, duration_ms: number): boolean {
  return duration_ms > SLOW_COMMAND_MS && !LONG_RUNNING_COMMANDS.has(command);
}

/**
 * The one place the UI calls Rust. A failed command logs `command_failed` and a
 * slow one `slow_command`, with the command name and timing only: never the
 * arguments, which hold people's work. The diagnostics commands are not logged,
 * so logging cannot feed itself.
 */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const started = performance.now();
  const watched = !command.startsWith("diag_");
  try {
    const result = await invoke<T>(command, args);
    const duration_ms = Math.round(performance.now() - started);
    if (watched && isSlowCommand(command, duration_ms)) log.warn("slow_command", { command, duration_ms });
    return result;
  } catch (e) {
    if (watched) log.error("command_failed", { command, duration_ms: Math.round(performance.now() - started), error: commandError(command, e) });
    throw e;
  }
}

const tauriApi: Api = {
  workspaceOpen: (path) => call("workspace_open", { path }),
  workspaceList: () => call("workspace_list"),
  workspaceCreate: (parent, name) => call("workspace_create", { parent, name }),
  workspaceDiscover: (workspaceId) => call("workspace_discover", { workspaceId }),
  showOpenFilePicker: () => call("show_open_file_picker"),
  vaultSet: (path) => call("vault_set", { path }),
  vaultGet: () => call("vault_get"),
  settingGet: (key) => call("setting_get", { key }),
  settingSet: (key, value) => call("setting_set", { key, value }),
  rulesList: (workspaceId) => call("rules_list", { workspaceId }),
  ruleDelete: (id) => call("rule_delete", { id }),
  auditVerify: () => call("audit_verify"),
  taskStart: (workspaceId, prompt, mode, onMessage, profile) => {
    const onEvent = new Channel<StreamMessage>();
    onEvent.onmessage = onMessage;
    return call("task_start", { workspaceId, prompt, mode, onEvent, profile: profile ?? null });
  },
  agentProfiles: (workspaceId) => call("agent_profiles", { workspaceId }),
  taskUndoStep: (taskId) => call("task_undo_step", { taskId }),
  taskUndoable: (taskId) => call("task_undoable", { taskId }),
  taskCancel: (taskId) => call("task_cancel", { taskId }),
  approvalRespond: (taskId, approvalId, choice) =>
    call("approval_respond", { taskId, approvalId, choice }),
  reviewAction: (taskId, action) => call("review_action", { taskId, action }),
  routerHealth: () => call("router_health"),
  diagRecent: (filter) => call("diag_recent", { ...filter }),
  diagSetLevel: (level) => call("diag_set_level", { level }),
  diagOpenFolder: () => call("diag_open_folder"),
  diagExport: () => call("diag_export"),
  diagLog: (level, event, fields) => call("diag_log", { level, event, fields }),
  boardList: () => call("board_list"),
  boardCreate: (name, scene) => call("board_create", { name, scene: scene ?? null }),
  boardOpen: (name) => call("board_open", { name }),
  boardSave: (name, content, baseRevision) =>
    call("board_save", { name, content, baseRevision: baseRevision ?? null }),
  boardRename: (from, to) => call("board_rename", { from, to }),
  boardDelete: (name) => call("board_delete", { name }),
  boardSketch: (name) => call("board_sketch", { name }),
  boardExport: (name, ext, data) => call("board_export", { name, ext, data }),
  // --- notes-ui ---
  graphOpen: (path, create) => call("graph_open", { path, create: create ?? null }),
  graphSearch: (q, limit) => call("graph_search", { q, limit: limit ?? null }),
  graphPage: (name) => call("graph_page", { name }),
  graphBacklinks: (name) => call("graph_backlinks", { name }),
  graphJournals: (limit) => call("graph_journals", { limit: limit ?? null }),
  graphSavePage: (name, tree) => call("graph_save_page", { name, tree }),
  graphPages: () => call("graph_pages"),
  graphGraph: () => call("graph_graph"),
  graphQuery: (text, today) => call("graph_query", { text, today }),
  graphBlock: (uuid) => call("graph_block", { uuid }),
  graphJournalName: (day) => call("graph_journal_name", { day }),
  onGraphChanged: (callback) => listen<string[]>("graph-changed", (e) => callback(e.payload)),
  // --- end notes-ui ---
  learningSettingsGet: () => call("learning_settings_get"),
  learningSettingsSet: (settings) => call("learning_settings_set", { settings }),
  memoryGet: (workspaceId) => call("memory_get", { workspaceId }),
  memorySave: (workspaceId, target, text) => call("memory_save", { workspaceId, target, text }),
  skillsOverview: (workspaceId) => call("skills_overview", { workspaceId }),
  skillRead: (workspaceId, name) => call("skill_read", { workspaceId, name }),
  skillPin: (workspaceId, root, name, pinned) => call("skill_pin", { workspaceId, root, name, pinned }),
  skillArchive: (workspaceId, name) => call("skill_archive", { workspaceId, name }),
  skillRestore: (workspaceId, root, name) => call("skill_restore", { workspaceId, root, name }),
  curatorRun: () => call("curator_run"),
  cronJobs: (workspaceId) => call("cron_jobs", { workspaceId: workspaceId ?? null }),
  cronSave: (input) => call("cron_save", { input }),
  cronPause: (id, paused) => call("cron_pause", { id, paused }),
  cronDelete: (id) => call("cron_delete", { id }),
  cronRunNow: (id) => call("cron_run_now", { id }),
  onTaskStream: (callback) => listen<StreamMessage>("task-stream", (e) => callback(e.payload)),
  onTaskStarted: (callback) => listen<TaskStarted>("task-started", (e) => callback(e.payload)),
  onRouterHealth: (callback) =>
    listen<RouterHealthReport | null>("router-health", (event) => {
      if (event.payload) callback(event.payload);
    }),
  gatewayPresets: () => call("gateway_presets"),
  gatewayConnections: () => call("gateway_connections"),
  gatewayConnectionSave: (input) => call("gateway_connection_save", { input }),
  gatewayConnectionDelete: (id) => call("gateway_connection_delete", { id }),
  gatewayConnectionTest: (id) => call("gateway_connection_test", { id }),
  gatewayCombos: () => call("gateway_combos"),
  gatewayComboSave: (input) => call("gateway_combo_save", { input }),
  gatewayComboDelete: (id) => call("gateway_combo_delete", { id }),
  gatewaySettingsGet: () => call("gateway_settings_get"),
  gatewaySettingsSet: (settings) => call("gateway_settings_set", { settings }),
  gatewayTargets: () => call("gateway_targets"),
  usageSummary: (days) => call("usage_summary", { days }),
  gatewayCatalogStatus: () => call("gateway_catalog_status"),
  gatewayCatalogRefresh: () => call("gateway_catalog_refresh"),
  gatewayCacheCount: () => call("gateway_cache_count"),
  gatewayCacheClear: () => call("gateway_cache_clear"),
  upstreamWatchStatus: () => call("upstream_watch_status"),
  upstreamWatchCheck: () => call("upstream_watch_check"),
  upstreamWatchMarkSeen: () => call("upstream_watch_mark_seen"),
  upstreamOpen: (url) => call("upstream_open", { url }),
  onUpstreamWatch: (callback) => listen<WatchView>("upstream-watch", (event) => callback(event.payload)),
  channelList: () => call("channel_list"),
  channelSave: (config, token) => call("channel_save", { input: { config, token: token ?? null } }),
  channelDelete: (id) => call("channel_delete", { id }),
  channelTest: (id, token) => call("channel_test", { id, token: token ?? null }),
  channelRestart: (id) => call("channel_restart", { id }),
  channelPairings: (id) => call("channel_pairings", { id }),
  channelPairingResolve: (id, code, approve) => call("channel_pairing_resolve", { id, code, approve }),
  channelAllowList: (id) => call("channel_allow_list", { id }),
  channelAllowAdd: (id, senderId, label) => call("channel_allow_add", { id, senderId, label }),
  channelAllowRemove: (id, senderId) => call("channel_allow_remove", { id, senderId }),
  channelActivity: (id, limit) => call("channel_activity", { id, limit: limit ?? null }),
  onChannelHealth: (callback) =>
    listen<{ channelId: string; health: ChannelHealth }>("channel-health", (e) => callback(e.payload.channelId, e.payload.health)),
  onChannelChange: async (callback) => {
    const offs = await Promise.all([
      listen<{ channelId: string }>("channel-pairing", (e) => callback(e.payload.channelId)),
      listen<{ channelId: string }>("channel-activity", (e) => callback(e.payload.channelId)),
    ]);
    return () => offs.forEach((off) => off());
  },
  designSystemGet: (workspaceId) => call("design_system_get", { workspaceId }),
  designSystemGenerate: (workspaceId, overwrite) => call("design_system_generate", { workspaceId, overwrite }),
  designSystemSave: (workspaceId, designMd, tokensCss) => call("design_system_save", { workspaceId, designMd, tokensCss }),
  designGenerate: (workspaceId, prompt, frame, count, board, critique, onProgress) =>
    call("design_generate", { workspaceId, prompt, frame, count, board, critique, onProgress: channel(onProgress) }),
  designHistory: (workspaceId) => call("design_history", { workspaceId }),
  designChoose: (id) => call("design_choose", { id }),
  designIterate: (id, message) => call("design_iterate", { id, message }),
  designPreviewStart: (workspaceId, url, studioOrigin) => call("design_preview_start", { workspaceId, url, studioOrigin }),
  designPreviewStop: (workspaceId) => call("design_preview_stop", { workspaceId }),
  designPreviewLog: (workspaceId) => call("design_preview_log", { workspaceId }),
  designSource: (workspaceId, location) => call("design_source", { workspaceId, location }),
  designApplyStyle: (workspaceId, location, changes, onMessage) =>
    call("design_apply_style", { workspaceId, location, changes, onEvent: channel(onMessage) }),
  designConvert: (id, framework, onMessage) => call("design_convert", { id, framework, onEvent: channel(onMessage) }),
  designImplement: (id, onMessage) => call("design_implement", { id, onEvent: channel(onMessage) }),
};

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api: Api = inTauri() ? tauriApi : mockApi;

// The UI logger sends through the API: into Rust inside Tauri, into the mock list in a browser.
setLogSink((level, event, fields) => api.diagLog(level, event, fields));
