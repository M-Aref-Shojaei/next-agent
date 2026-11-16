// UI side of the local diagnostics log (docs/plan/Logging System Plan.md, ADR-0013).
// Records how the app behaves, never what people work on: only allowlisted field
// names go out, only numbers, booleans and short redacted strings, and a call
// that passes anything else loses that field instead of leaking it. The Rust
// side filters and redacts again; this is the first of two gates.

export type LogLevel = "error" | "warn" | "info" | "debug";

export interface LogEntry {
  ts: string;
  level: LogLevel;
  target: string;
  event: string;
  fields: Record<string, unknown>;
  span?: Record<string, unknown>;
}

export type LogFieldValue = string | number | boolean;
export type LogFields = Record<string, unknown>;

/** Field names the log accepts. Keep in step with crates/diagnostics/src/fields.rs. */
export const ALLOWED_FIELDS: ReadonlySet<string> = new Set([
  "error", "kind", "code", "status", "duration_ms", "attempt", "count", "bytes",
  "tokens_in", "tokens_out", "tokens_cached", "cost_micros", "provider", "model", "combo",
  "role", "mode", "tool", "decision", "rule_kind", "command", "ok", "task_id", "approval_id",
  "channel_id", "channel_kind", "cron_id", "session_id", "window", "page", "setting",
  "version", "from_version", "to_version", "migration", "os", "arch", "level", "reason",
  "retries", "queue", "files", "exit_code", "signal", "port",
]);

/** Names that must never be logged, even if someone adds them to the allowlist. */
export const FORBIDDEN_FIELDS: ReadonlySet<string> = new Set([
  "prompt", "content", "text", "body", "answer", "response", "message_text", "title", "name",
  "note", "query", "args", "input", "output", "diff", "path",
  "token", "key", "secret", "password", "cookie", "authorization", "sender", "username", "email", "phone",
]);

export const MAX_STRING = 300;
/** Client-side ceiling; the Rust side allows 20 a second. */
export const RATE_LIMIT = { max: 10, windowMs: 1000 } as const;

const SECRET_SHAPES: readonly RegExp[] = [
  /\bsk-[A-Za-z0-9_-]{8,}/g,
  /\bAIza[0-9A-Za-z_-]{20,}/g,
  /\bgh[pousr]_[A-Za-z0-9]{20,}/g,
  /\bgithub_pat_[A-Za-z0-9_]{20,}/g,
  /\bxox[a-z]-[A-Za-z0-9-]{10,}/g,
  /\b\d{6,}:[A-Za-z0-9_-]{30,}/g,
  /\bBearer\s+[A-Za-z0-9._~+/=-]{8,}/gi,
  /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g,
  /\b[A-Fa-f0-9]{32,}\b/g,
  /\b[A-Za-z0-9+/_-]{40,}={0,2}/g,
];

/** Secret shapes, home folders, emails and phone numbers out; cut to 300 characters. */
export function redact(text: string): string {
  let out = text;
  for (const re of SECRET_SHAPES) out = out.replace(re, "[secret]");
  out = out
    .replace(/\/(?:Users|home)\/[^/\s]+/g, "~")
    .replace(/[A-Za-z]:\\Users\\[^\\\s]+/g, "~")
    .replace(/[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g, "[redacted]")
    .replace(/\+?\d[\d\s().-]{7,}\d/g, "[redacted]");
  return out.length > MAX_STRING ? out.slice(0, MAX_STRING) : out;
}

/** Keeps allowlisted names with a number, boolean or string value; anything else is dropped. */
export function filterFields(fields: LogFields | undefined): Record<string, LogFieldValue> {
  const kept: Record<string, LogFieldValue> = {};
  for (const [name, value] of Object.entries(fields ?? {})) {
    if (!ALLOWED_FIELDS.has(name) || FORBIDDEN_FIELDS.has(name)) continue;
    if (typeof value === "string") kept[name] = redact(value);
    else if (typeof value === "boolean") kept[name] = value;
    else if (typeof value === "number" && Number.isFinite(value)) kept[name] = value;
  }
  return kept;
}

/** Sliding-window limiter. `allow(now)` says whether one more event fits. */
export function createLimiter(max: number = RATE_LIMIT.max, windowMs: number = RATE_LIMIT.windowMs) {
  let stamps: number[] = [];
  return {
    allow(now: number): boolean {
      stamps = stamps.filter((t) => now - t < windowMs);
      if (stamps.length >= max) return false;
      stamps.push(now);
      return true;
    },
    reset() {
      stamps = [];
    },
  };
}

type Sink = (level: LogLevel, event: string, fields: Record<string, LogFieldValue>) => Promise<unknown> | void;
let sink: Sink | null = null;
const limiter = createLimiter();
let suppressed = 0;

/** api.ts connects the logger to `api.diagLog`; tests connect their own. */
export function setLogSink(next: Sink | null): void {
  sink = next;
}

/** Test hook: forgets the rate-limit window and the suppressed count. */
export function resetLogState(): void {
  suppressed = 0;
  limiter.reset();
}

function emit(level: LogLevel, event: string, fields?: LogFields): void {
  if (!sink) return;
  if (!limiter.allow(Date.now())) {
    suppressed++;
    return;
  }
  const kept = filterFields(fields);
  // After a burst, the next event says how many were skipped.
  const out = suppressed > 0 ? { ...kept, retries: suppressed } : kept;
  suppressed = 0;
  try {
    // A logger must never throw into the code it observes, nor loop on its own failure.
    void Promise.resolve(sink(level, event, out)).catch(() => {});
  } catch {
    /* ignore */
  }
}

export const log = {
  debug: (event: string, fields?: LogFields) => emit("debug", event, fields),
  info: (event: string, fields?: LogFields) => emit("info", event, fields),
  warn: (event: string, fields?: LogFields) => emit("warn", event, fields),
  error: (event: string, fields?: LogFields) => emit("error", event, fields),
};

/** The message of a thrown value, redacted and cut: the only free text the UI sends. */
export function errorMessage(e: unknown): string {
  return redact(String(e instanceof Error ? e.message : e));
}

let installed = false;

/** Window errors and unhandled rejections become `ui_error` events. Runs once. */
export function installGlobalHandlers(
  target: Pick<Window, "addEventListener" | "location"> | undefined = typeof window === "undefined" ? undefined : window,
): void {
  if (installed || !target) return;
  installed = true;
  const page = () => target.location?.pathname ?? "";
  target.addEventListener("error", (event) => {
    const e = event as ErrorEvent;
    log.error("ui_error", { kind: "error", error: errorMessage(e.error ?? e.message), page: page() });
  });
  target.addEventListener("unhandledrejection", (event) => {
    log.error("ui_error", { kind: "unhandled_rejection", error: errorMessage((event as PromiseRejectionEvent).reason), page: page() });
  });
}

installGlobalHandlers();
