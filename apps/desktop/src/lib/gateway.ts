// Types and pure helpers for the built-in LLM gateway pages (Settings →
// Providers, Usage). The IPC calls live in api.ts.

export type CavemanLevel = "lite" | "full" | "ultra";
export type ComboStrategy = "fallback" | "round-robin";
export const ROLES = ["chat", "design", "summary", "review"] as const;
export type Role = (typeof ROLES)[number];

export interface ProviderPreset {
  id: string;
  label: string;
  kind: "openai" | "anthropic" | "gemini";
  baseUrl: string;
  needsKey: boolean;
  models: string[];
}

/** A connection as returned by the backend. The API key is never included. */
export interface ProviderConnection {
  id: string;
  provider: string;
  kind: string;
  name: string;
  priority: number;
  isActive: boolean;
  baseUrl: string;
  baseUrlOverride: string;
  models: string[];
  needsKey: boolean;
  hasKey: boolean;
}

export interface ConnectionInput {
  id?: string;
  provider: string;
  name?: string;
  priority?: number;
  isActive?: boolean;
  baseUrl?: string;
  models?: string[];
  /** Non-empty = store in the OS keychain. Never read back. */
  apiKey?: string;
  clearKey?: boolean;
}

export interface Combo {
  id: string;
  name: string;
  models: string[];
  strategy: ComboStrategy | null;
  stickyLimit: number | null;
}

export interface GatewaySettings {
  defaultModel: string | null;
  tokenSaver: boolean;
  caveman: CavemanLevel | null;
  ponytail: CavemanLevel | null;
  rtk: boolean;
  cacheControl: boolean;
  comboStrategy: ComboStrategy;
  roles: Partial<Record<Role, string>>;
}

export interface GatewayTarget {
  value: string;
  label: string;
  kind: "model" | "combo";
}

export interface ConnectionTest {
  ok: boolean;
  detail: string;
  models: string[];
}

export interface UsageTotals {
  key: string;
  requests: number;
  failures: number;
  promptTokens: number;
  completionTokens: number;
  cachedTokens: number;
  reasoningTokens: number;
  cost: number;
}

export interface UsageRecent {
  ts: string;
  provider: string;
  model: string;
  role: string;
  combo: string | null;
  status: number;
  promptTokens: number;
  completionTokens: number;
  cachedTokens: number;
  cost: number;
  costKnown: boolean;
  latencyMs: number;
}

export interface UsageSummary {
  days: number;
  totals: UsageTotals;
  byProvider: UsageTotals[];
  byModel: UsageTotals[];
  byRole: UsageTotals[];
  daily: UsageTotals[];
  recent: UsageRecent[];
  saverBytesBefore: number;
  saverBytesAfter: number;
}

export function defaultSettings(): GatewaySettings {
  return {
    defaultModel: null,
    tokenSaver: true,
    caveman: null,
    ponytail: null,
    rtk: true,
    cacheControl: true,
    comboStrategy: "fallback",
    roles: {},
  };
}

/** "a, b\nc" → ["a", "b", "c"] (blank entries dropped). */
export function parseList(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** USD with enough precision for small amounts; always labelled an estimate by callers. */
export function formatCost(usd: number): string {
  if (usd === 0) return "$0";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  return `$${usd.toFixed(2)}`;
}

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/** Percentage of tool-output bytes RTK removed (0 when nothing ran). */
export function savedPercent(before: number, after: number): number {
  if (before <= 0) return 0;
  return Math.round(((before - after) / before) * 100);
}

export interface DayBar {
  day: string;
  value: number;
  /** 0..1 of the tallest bar. */
  height: number;
}

function isoDay(date: Date): string {
  return date.toISOString().slice(0, 10);
}

/**
 * One bar per calendar day for the last `days` days (oldest first), filling
 * days without usage with zero. `today` is injectable for tests.
 */
export function dailyBars(
  daily: UsageTotals[],
  days: number,
  metric: "cost" | "tokens" | "requests",
  today: Date = new Date(),
): DayBar[] {
  const byDay = new Map(daily.map((d) => [d.key, d]));
  const value = (d: UsageTotals | undefined) =>
    !d ? 0 : metric === "cost" ? d.cost : metric === "tokens" ? d.promptTokens + d.completionTokens : d.requests;
  const bars: DayBar[] = [];
  for (let i = days - 1; i >= 0; i--) {
    const date = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() - i));
    const day = isoDay(date);
    bars.push({ day, value: value(byDay.get(day)), height: 0 });
  }
  const max = Math.max(0, ...bars.map((b) => b.value));
  for (const bar of bars) bar.height = max > 0 ? bar.value / max : 0;
  return bars;
}

/** The provider prefix of a `provider/model` target ("" for combos). */
export function providerOf(target: string): string {
  const slash = target.indexOf("/");
  return slash > 0 ? target.slice(0, slash) : "";
}
