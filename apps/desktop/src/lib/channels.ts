// Chat channels (Settings → Channels): wire types and pure helpers. Kept
// free of Svelte so it runs under `node --test`. Rules live in Rust
// (`crates/channels`); these helpers only shape the form and labels.

export type DmPolicy = "pairing" | "allowlist" | "open" | "disabled";
export type GroupPolicy = "disabled" | "allowlist" | "open";
export type DmScope = "main" | "per-peer" | "per-channel-peer" | "per-account-channel-peer";
/** Channel tasks never run in auto or bypass mode. */
export type ChannelMode = "manual" | "plan";

export interface ChannelConfig {
  id: string;
  kind: "telegram";
  name: string;
  enabled: boolean;
  workspaceId: string;
  mode: ChannelMode;
  dmPolicy: DmPolicy;
  groupPolicy: GroupPolicy;
  groups: string[];
  requireMention: boolean;
  dmScope: DmScope;
  identityLinks: Record<string, string[]>;
  approvalTimeoutSecs: number;
  tools: string[];
}

export type HealthState = "starting" | "connected" | "reconnecting" | "failed" | "stopped";
export interface ChannelHealth {
  state: HealthState;
  detail: string;
  restarts: number;
  since: number;
}
export interface ChannelInfo {
  config: ChannelConfig;
  /** The token itself never leaves the keychain. */
  hasToken: boolean;
  health: ChannelHealth | null;
}
export interface PairingRequest {
  channelId: string;
  senderId: string;
  chatId: string;
  label: string;
  code: string;
  createdAt: number;
  lastSeenAt: number;
}
export interface AllowEntry {
  senderId: string;
  label: string;
  source: "pairing" | "manual" | string;
  addedAt: string;
}
export interface ChannelActivity {
  ts: string;
  channelId: string;
  direction: "in" | "out" | "system" | string;
  kind: string;
  peer: string;
  text: string;
  taskId: string | null;
}
export interface BotIdentity {
  id: string;
  username: string;
  name: string;
}

/** Mirrors `profile::SELECTABLE_TOOLS` / `DEFAULT_TOOLS` in Rust. */
export const SELECTABLE_TOOLS: { name: string; label: string }[] = [
  { name: "read_file", label: "Read files" },
  { name: "list_dir", label: "List folders" },
  { name: "search", label: "Search" },
  { name: "write_file", label: "Write files (asks on the desktop)" },
  { name: "apply_patch", label: "Apply patches (asks on the desktop)" },
  { name: "edit", label: "Edit files (asks on the desktop)" },
  { name: "run_command", label: "Run commands (asks on the desktop)" },
  { name: "http_fetch", label: "Fetch URLs (asks on the desktop)" },
];
export const DEFAULT_TOOLS = ["read_file", "list_dir", "search", "write_file", "apply_patch", "edit"];

export const PAIRING_TTL_SECS = 3600;

export function newChannel(workspaceId: string): ChannelConfig {
  return {
    id: "",
    kind: "telegram",
    name: "Telegram",
    enabled: true,
    workspaceId,
    mode: "manual",
    dmPolicy: "pairing",
    groupPolicy: "disabled",
    groups: [],
    requireMention: true,
    dmScope: "per-channel-peer",
    identityLinks: {},
    approvalTimeoutSecs: 300,
    tools: [...DEFAULT_TOOLS],
  };
}

/** Problems to fix before saving (the backend checks again). */
export function formErrors(config: ChannelConfig, allow: AllowEntry[], token: string, hasToken: boolean): string[] {
  const errors: string[] = [];
  if (!config.workspaceId) errors.push("Pick the workspace channel tasks run in.");
  if (!hasToken && !token.trim()) errors.push("Paste the bot token from @BotFather.");
  if (token.trim() && !looksLikeToken(token)) errors.push("That does not look like a bot token (123456:ABC…).");
  if (config.dmPolicy === "open" && !allow.some((a) => a.senderId === "*")) {
    errors.push("The open policy needs an explicit * on the allow-list.");
  }
  if (config.mode !== "manual" && config.mode !== "plan") errors.push("Channel tasks run only in manual or plan mode.");
  return errors;
}

export function looksLikeToken(token: string): boolean {
  return /^\d+:[A-Za-z0-9_-]{20,}$/.test(token.trim());
}

export function healthLabel(health: ChannelHealth | null, enabled: boolean): { text: string; tone: "ok" | "info" | "error" } {
  if (!enabled) return { text: "Disabled", tone: "info" };
  if (!health) return { text: "Not started", tone: "info" };
  switch (health.state) {
    case "connected":
      return { text: "Connected", tone: "ok" };
    case "starting":
      return { text: "Starting…", tone: "info" };
    case "reconnecting":
      return { text: `Reconnecting: ${health.detail}`, tone: "error" };
    case "failed":
      return { text: `Failed: ${health.detail}`, tone: "error" };
    default:
      return { text: health.detail || "Stopped", tone: "info" };
  }
}

/** "expires in 42 min" for a pending pairing. */
export function expiresIn(request: PairingRequest, nowSecs: number): string {
  const left = request.createdAt + PAIRING_TTL_SECS - nowSecs;
  if (left <= 0) return "expired";
  const minutes = Math.ceil(left / 60);
  return `expires in ${minutes} min`;
}

/** Comma or newline separated ids → clean list. */
export function parseIds(text: string): string[] {
  return [...new Set(text.split(/[\s,]+/).map((s) => s.trim()).filter(Boolean))];
}

export function toggleTool(tools: string[], name: string, on: boolean): string[] {
  const set = new Set(tools);
  if (on) set.add(name);
  else set.delete(name);
  return SELECTABLE_TOOLS.map((t) => t.name).filter((n) => set.has(n));
}
