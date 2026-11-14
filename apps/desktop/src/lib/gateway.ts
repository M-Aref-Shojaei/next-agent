// Types and pure helpers for the built-in LLM gateway pages (Settings →
// Providers, Usage). The IPC calls live in api.ts.

export type CavemanLevel = "lite" | "full" | "ultra";
export type ComboStrategy =
  | "fallback"
  | "round-robin"
  | "smart"
  | "weighted"
  | "cost-optimized"
  | "lkgp"
  | "context-optimized";
export const STRATEGIES: { value: ComboStrategy; label: string }[] = [
  { value: "fallback", label: "Fallback (in order)" },
  { value: "round-robin", label: "Round-robin" },
  { value: "smart", label: "Smart (one model per task)" },
  { value: "weighted", label: "Weighted (random by weight)" },
  { value: "cost-optimized", label: "Cheapest first" },
  { value: "lkgp", label: "Last model that worked first" },
  { value: "context-optimized", label: "Largest context first" },
];
export type Effort = "off" | "low" | "medium" | "high" | "max";
export const EFFORTS: Effort[] = ["off", "low", "medium", "high", "max"];
export type BudgetPeriod = "daily" | "monthly";
export type BudgetAction = "block" | "cheapest";
/** Smart-combo confidence thresholds offered in the editor (0.8 is the default). */
export const THRESHOLDS = [0.7, 0.8, 0.9] as const;
export const DEFAULT_THRESHOLD = 0.8;
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
  /** Smart only: switch models when answer quality drops below this. */
  confidenceThreshold: number | null;
  /** Weighted only: one weight per model ([] = all 1). */
  weights: number[];
}

export interface ResponseCacheSettings {
  enabled: boolean;
  ttlSecs: number;
  maxEntries: number;
  anyTemperature: boolean;
}

export interface ReasoningSetting {
  effort: Effort;
  /** Wins over `effort` when set. */
  maxTokens: number | null;
}

export interface Budget {
  /** A role, or null for all spend. */
  role: string | null;
  period: BudgetPeriod;
  limitUsd: number;
}

export interface BudgetStatus {
  budget: Budget;
  spent: number;
  level: "ok" | "warn" | "over";
}

export interface CatalogStatus {
  models: number;
  /** Unix seconds; null = never imported. */
  updatedAt: number | null;
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
  cacheAffinity: boolean;
  responseCache: ResponseCacheSettings;
  /** Role → setting (roles without an entry are off). */
  reasoning: Record<string, ReasoningSetting>;
  budgets: Budget[];
  budgetAction: BudgetAction;
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

/** A smart combo moving a task to another model. */
export interface ModelSwitch {
  ts: string;
  session: string;
  combo: string;
  from: string;
  to: string;
  /** `failover` or `quality:<signal>` (empty, refused, bad-tool-json, truncated). */
  reason: string;
  confidence: number;
}

export interface UsageSummary {
  days: number;
  totals: UsageTotals;
  byProvider: UsageTotals[];
  byModel: UsageTotals[];
  byRole: UsageTotals[];
  daily: UsageTotals[];
  recent: UsageRecent[];
  switches: ModelSwitch[];
  cacheHits: number;
  cacheSavedTokens: number;
  cacheSavedCost: number;
  budgets: BudgetStatus[];
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
    cacheAffinity: true,
    responseCache: { enabled: false, ttlSecs: 3600, maxEntries: 500, anyTemperature: false },
    reasoning: Object.fromEntries(ROLES.map((role) => [role, { effort: "off", maxTokens: null }])),
    budgets: [],
    budgetAction: "block",
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

/** Share of input tokens served from the provider's prompt cache (0–100). */
export function cacheHitRate(t: { promptTokens: number; cachedTokens: number }): number {
  if (t.promptTokens <= 0) return 0;
  return Math.round((t.cachedTokens / t.promptTokens) * 100);
}

/** Spend as a share of a budget (0–100, capped). */
export function budgetPercent(status: BudgetStatus): number {
  if (status.budget.limitUsd <= 0) return 0;
  return Math.min(100, Math.round((status.spent / status.budget.limitUsd) * 100));
}

/** "Daily budget for chat" / "Monthly budget". */
export function budgetLabel(b: Budget): string {
  const period = b.period === "daily" ? "Daily" : "Monthly";
  return b.role ? `${period} budget for ${b.role}` : `${period} budget`;
}

/** Weights text "3, 1" → [3, 1]; blank → []. Invalid entries become 1. */
export function parseWeights(text: string): number[] {
  return parseList(text).map((w) => {
    const n = Math.floor(Number(w));
    return Number.isFinite(n) && n >= 0 ? n : 1;
  });
}

/** "2026-11-14T10:25:04.123Z" → "2026-11-14 10:25:04". */
export function formatTs(ts: string): string {
  return ts.replace("T", " ").slice(0, 19);
}

/** A combo's own strategy, else the global one. */
export function effectiveStrategy(strategy: ComboStrategy | "" | null, global: ComboStrategy): ComboStrategy {
  return strategy || global;
}

/** "quality:truncated" → "Quality: truncated answers"; "failover" → "Failover". */
export function switchReason(reason: string): string {
  if (reason === "failover") return "Failover (model failed)";
  const signal = reason.startsWith("quality:") ? reason.slice("quality:".length) : reason;
  const labels: Record<string, string> = {
    empty: "empty answers",
    refused: "refusals",
    "bad-tool-json": "broken tool-call JSON",
    truncated: "truncated answers",
  };
  return `Low quality: ${labels[signal] ?? signal}`;
}

/** The provider prefix of a `provider/model` target ("" for combos). */
export function providerOf(target: string): string {
  const slash = target.indexOf("/");
  return slash > 0 ? target.slice(0, slash) : "";
}
