// The overlay's settings (the vendored Coucou app, ADR-0011), as Next Agent's
// Settings → Overlay pages call them (owner request: one design system, no
// settings window of its own).
//
// Every call is one of the overlay's own Tauri commands, with the same
// arguments and the same error rules as Coucou's settings window had
// (coucou/src/core/bridge.ts, core/flow.ts): `call` gives null when the
// command fails, `callOrThrow` lets the page show why. In a plain browser
// (vite dev, screenshots) they answer from the in-memory stand-in in
// mock-api.ts.
//
// scripts/gen-capabilities.mjs reads the command names from this file: the
// main window is granted exactly these overlay commands, and no others.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { overlayMock } from "./mock-api.ts";
import type {
  AgentHookPlan,
  AgentHookStatus,
  AgyApprovalsStatus,
  BootInfo,
  HookPreview,
  HookStatus,
  LocalServer,
  ModelInfo,
  PhoneAnswersStatus,
  PhoneChatStatus,
  PhoneDetailsStatus,
  PhoneDiffsStatus,
  PhoneLinkPairing,
  PhoneLinkStatus,
  PhoneRecapStatus,
  PhoneRelayState,
  PhoneRelayStatus,
  PhoneServicesStatus,
  PhoneUsageStatus,
  ShortcutStatus,
  ShortcutsReport,
  SubscriptionStatus,
} from "../../coucou/src/core/bridge.ts";
import type { AppStyle, FlowPrefs, FlowStatus } from "../../coucou/src/core/flow.ts";
import type { RecapPrefs } from "../../coucou/src/recap/summary.ts";
import type { Settings } from "../../coucou/src/core/state.ts";
import type { UpdateStatus } from "../../coucou/src/core/update.ts";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Coucou's `call`: null when the command fails (logged), as its settings window did. */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  try {
    return inTauri() ? await invoke<T>(cmd, args) : ((await overlayMock(cmd, args)) as T);
  } catch (err) {
    console.error(`[next-agent] ${cmd} failed`, err);
    return null;
  }
}

/** Coucou's `callOrThrow`: the error reaches the page, which says what went wrong. */
async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return inTauri() ? invoke<T>(cmd, args) : ((await overlayMock(cmd, args)) as T);
}

/** An overlay event (`settings-changed`, `update-status`…). Never throws; a no-op outside Tauri. */
export async function onOverlayEvent<T>(name: string, handler: (payload: T) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  try {
    return await listen<T>(name, (e) => handler(e.payload));
  } catch (err) {
    console.error(`[next-agent] listen ${name} failed`, err);
    return () => {};
  }
}

export const overlay = {
  // ── General ────────────────────────────────────────────────────────────────
  boot: () => call<BootInfo>("boot"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  listMonitors: () => call<{ key: string; label: string }[]>("list_monitors"),
  customSounds: (names: readonly string[]) => call<string[]>("custom_sounds", { names }),
  revealSoundsFolder: () => call<void>("reveal_sounds_folder"),
  reloadSounds: () => call<void>("reload_sounds"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  recapPrefs: () => call<RecapPrefs>("recap_prefs"),
  recapSetEnabled: (enabled: boolean) => call<void>("recap_set_enabled", { enabled }),
  recapClear: () => call<void>("recap_clear"),

  // ── Updates ────────────────────────────────────────────────────────────────
  updateStatus: () => call<UpdateStatus>("update_status"),
  updateSetConsent: (allowed: boolean) => call<void>("update_set_consent", { allowed }),
  updateCheck: () => call<UpdateStatus>("update_check"),
  updateSetSource: (source: string) => callOrThrow<UpdateStatus>("update_set_source", { source }),
  updateShow: () => call<void>("update_show"),
  updateOpenPage: () => call<void>("update_open_page"),

  // ── Agents: Claude Code hooks, other agents, plan usage ─────────────────────
  hooksStatus: () => call<HookStatus>("hooks_status"),
  agentHooksStatus: () => call<Record<string, boolean>>("agent_hooks_status"),
  /** Diff to show before anything is written. `install: false` previews removal. */
  hooksPreview: (install: boolean) => callOrThrow<HookPreview>("hooks_preview", { install }),
  /** Writes ~/.claude/settings.json: only after the user confirmed this preview's fingerprint. */
  hooksApply: (install: boolean, fingerprint: string) => callOrThrow<string>("hooks_apply", { install, fingerprint }),
  agentHooksList: () => call<AgentHookStatus[]>("agent_hooks_list"),
  agentHooksPreview: (agent: string, install: boolean) =>
    callOrThrow<AgentHookPlan>("agent_hooks_preview", { agent, install }),
  agentHooksApply: (agent: string, install: boolean, fingerprint: string) =>
    callOrThrow<string>("agent_hooks_apply", { agent, install, fingerprint }),
  agyApprovalsStatus: () => call<AgyApprovalsStatus>("agy_approvals_status"),
  agyApprovalsSetEnabled: (enabled: boolean) =>
    callOrThrow<AgyApprovalsStatus>("agy_approvals_set_enabled", { enabled }),
  statusLinePreview: (install: boolean) => callOrThrow<HookPreview>("status_line_preview", { install }),
  statusLineApply: (install: boolean, fingerprint: string) =>
    callOrThrow<string>("status_line_apply", { install, fingerprint }),

  // ── Chat, keys, integrations ───────────────────────────────────────────────
  /** Only ever tells whether a key exists, never its value. */
  secretPresent: (key: string) => call<boolean>("secret_present", { key }),
  secretSet: (key: string, value: string) => callOrThrow<void>("secret_set", { key, value }),
  secretClear: (key: string) => callOrThrow<void>("secret_clear", { key }),
  claudeReady: () => call<boolean>("claude_ready"),
  /** Claude Code stands in for a missing Anthropic key (Coucou's `keyReady`). */
  keyReady: (key: string) =>
    key === "anthropic-api-key" ? call<boolean>("claude_ready") : call<boolean>("secret_present", { key }),
  claudeApiModels: () => callOrThrow<ModelInfo[]>("claude_api_models"),
  chatModels: (provider: string) => callOrThrow<ModelInfo[]>("chat_models", { provider }),
  subscriptionStatus: () => call<SubscriptionStatus[]>("subscription_status"),
  subscriptionSet: (provider: string, on: boolean) => call<SubscriptionStatus[]>("subscription_set", { provider, on }),
  /** The custom server's key, bound to the address it is entered for. */
  localSetKey: (url: string, key: string) => call<void>("local_set_key", { url, key }),
  localConnect: (provider: "ollama" | "lmstudio" | "custom", url: string) =>
    callOrThrow<LocalServer>("local_connect", { provider, url }),

  // ── Shortcuts ──────────────────────────────────────────────────────────────
  shortcutsStatus: () => call<ShortcutsReport>("shortcuts_status"),
  /** Lets go of every global shortcut while a new one is recorded. */
  shortcutsSuspend: (suspended: boolean) => call<void>("shortcuts_suspend", { suspended }),

  // ── Phone link ─────────────────────────────────────────────────────────────
  phoneLinkStatus: () => call<PhoneLinkStatus>("phone_link_status"),
  phoneLinkSetEnabled: (enabled: boolean) => callOrThrow<PhoneLinkStatus>("phone_link_set_enabled", { enabled }),
  phoneLinkPairing: () => callOrThrow<PhoneLinkPairing>("phone_link_pairing"),
  phoneLinkNewPairing: () => callOrThrow<PhoneLinkPairing>("phone_link_new_pairing"),
  phoneDetailsStatus: () => call<PhoneDetailsStatus>("phone_details_status"),
  phoneDetailsSetEnabled: (enabled: boolean) => callOrThrow<PhoneDetailsStatus>("phone_details_set_enabled", { enabled }),
  phoneAnswersStatus: () => call<PhoneAnswersStatus>("phone_answers_status"),
  phoneAnswersSetEnabled: (enabled: boolean) => callOrThrow<PhoneAnswersStatus>("phone_answers_set_enabled", { enabled }),
  phoneDiffsStatus: () => call<PhoneDiffsStatus>("phone_diffs_status"),
  phoneDiffsSetEnabled: (enabled: boolean) => callOrThrow<PhoneDiffsStatus>("phone_diffs_set_enabled", { enabled }),
  phoneServicesStatus: () => call<PhoneServicesStatus>("phone_services_status"),
  phoneServicesSet: (services: string[]) => callOrThrow<PhoneServicesStatus>("phone_services_set", { services }),
  phoneRelayStatus: () => call<PhoneRelayStatus>("phone_relay_status"),
  phoneRelaySet: (enabled: boolean, url: string) => callOrThrow<PhoneRelayStatus>("phone_relay_set", { enabled, url }),
  /** Write-only: stored in the OS keystore, never sent back. */
  phoneRelaySetAccess: (access: string) => callOrThrow<PhoneRelayStatus>("phone_relay_set_access", { access }),
  phoneUsageStatus: () => call<PhoneUsageStatus>("phone_usage_status"),
  phoneUsageSetEnabled: (enabled: boolean) => callOrThrow<PhoneUsageStatus>("phone_usage_set_enabled", { enabled }),
  phoneRecapStatus: () => call<PhoneRecapStatus>("phone_recap_status"),
  phoneRecapSetEnabled: (enabled: boolean) => callOrThrow<PhoneRecapStatus>("phone_recap_set_enabled", { enabled }),
  phoneChatStatus: () => call<PhoneChatStatus>("phone_chat_status"),
  phoneChatSetEnabled: (enabled: boolean) => callOrThrow<PhoneChatStatus>("phone_chat_set_enabled", { enabled }),
  phoneChatSetModels: (models: string[]) => callOrThrow<PhoneChatStatus>("phone_chat_set_models", { models }),

  // ── Flow: dictation, Notetaker, voice ──────────────────────────────────────
  flowStatus: () => call<FlowStatus>("flow_status"),
  flowSetPrefs: (prefs: FlowPrefs) => call<void>("flow_set_prefs", { prefs }),
  flowClear: (everything: boolean) => callOrThrow<void>("flow_clear", { everything }),
  flowStyles: () => call<AppStyle[]>("flow_styles"),
  flowSetStyles: (styles: AppStyle[]) => callOrThrow<void>("flow_set_styles", { styles }),
  flowRunningApps: () => call<{ id: string; name: string }[]>("flow_running_apps"),
  flowPasteAgain: () => call<void>("flow_paste_again"),
  flowModelsDownload: () => call<void>("flow_models_download"),
  flowModelsRemove: () => call<void>("flow_models_remove"),
};

// ── Pages and routes ──────────────────────────────────────────────────────────

/** Settings → Overlay, one page per section of Coucou's window. */
export const OVERLAY_SECTIONS = [
  { id: "general", label: "General" },
  { id: "pills", label: "Pills and colours" },
  { id: "agents", label: "Agents" },
  { id: "chat", label: "Chat" },
  { id: "integrations", label: "Integrations" },
  { id: "dictation", label: "Dictation" },
  { id: "notetaker", label: "Notetaker" },
  { id: "voice", label: "Voice" },
  { id: "shortcuts", label: "Shortcuts" },
  { id: "phone", label: "Phone link" },
  { id: "updates", label: "Updates" },
  { id: "about", label: "About" },
] as const;

export type OverlaySection = (typeof OVERLAY_SECTIONS)[number]["id"];

export const OVERLAY_ROOT = "/settings/overlay";

/**
 * Coucou's page ids (its window's pages, the tray's "settings", the Flow
 * pill's "settings") → the Settings route that holds them now. Unknown ids
 * land on General, as Coucou's window fell back to its first page.
 */
const PAGE_ROUTES: Record<string, string> = {
  general: "general",
  pills: "pills",
  agents: "agents",
  chat: "chat",
  integrations: "integrations",
  phone: "phone",
  shortcuts: "shortcuts",
  voice: "voice",
  updates: "updates",
  about: "about",
  dictation: "dictation",
  settings: "dictation",
  flowsettings: "dictation",
  styles: "dictation#styles",
  notetaker: "notetaker",
};

export function overlaySettingsHref(page: string | null | undefined): string {
  return `${OVERLAY_ROOT}/${PAGE_ROUTES[page ?? ""] ?? "general"}`;
}

/**
 * Coucou's window, the tray and the island ask Rust to show a settings page
 * (`show_hub`); Rust shows the main window and tells it which one.
 */
export function onOverlaySettingsRequest(go: (href: string) => void): Promise<() => void> {
  return onOverlayEvent<string>("overlay-settings-open", (page) => go(overlaySettingsHref(page)));
}

// ── Coucou's small rules, kept as they were ────────────────────────────────────

/** An error as the settings window printed it. */
export function errorText(err: unknown): string {
  return String(err).replace(/^Error:\s*/, "");
}

/** Where secrets.rs keeps the keys on this OS (settings/main.ts KEY_STORE). */
export function keyStoreName(userAgent: string): string {
  return userAgent.includes("Windows") ? "Windows Credential Manager" : "Secret Service (GNOME Keyring, KWallet)";
}

/** General → Auto-close: whole seconds from 5 to 120, 15 when unreadable. */
export function clampAutoClose(raw: unknown): number {
  return Math.max(5, Math.min(120, Number(raw) || 15));
}

/**
 * General → Island lives on: the option to show for a saved display. One saved
 * under an older key (moved, resized, or saved before names were kept) is
 * found by its place or its name; one that is gone shows as the main one.
 */
export function resolveScreen(saved: string, keys: readonly string[]): string {
  const [place, name] = saved.split("|");
  const found =
    keys.find((k) => k === saved) ??
    (saved.startsWith("at:") ? keys.find((k) => k.split("|")[0] === place) : undefined) ??
    (name ? keys.find((k) => k.split("|")[1] === name) : undefined);
  if (found) return found;
  return ["primary", "cursor"].includes(saved) ? saved : "primary";
}

/** One line of a settings.json diff, coloured by its first character. */
export function diffLines(text: string): { kind: "add" | "del" | "ctx"; text: string }[] {
  return text.split("\n").map((line) => ({
    kind: line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx",
    text: line,
  }));
}

/** Chat → Claude: the key, else Claude Code (switched on above), else what is missing. */
export function apiState(hasKey: boolean, relay: boolean, store: string): string {
  if (hasKey) return `Key saved in the ${store}.`;
  return relay ? "No key: the chat can go through your Claude subscription above." : "No key yet — the chat needs one.";
}

/** What a local model server's address means for the user's data. */
export function exposureNotice(
  exposure: "local" | "remote" | "remote-http" | "invalid",
  url: string,
  withKey: boolean,
): { tone: "hint" | "warn" | "danger"; text: string } | null {
  switch (exposure) {
    case "local":
      return { tone: "hint", text: "Nothing leaves your PC: the server runs on this computer." };
    case "remote":
      return { tone: "hint", text: "This address is another machine: what you ask is sent to it." };
    case "remote-http":
      return withKey
        ? { tone: "warn", text: "Warning: the key would be sent unencrypted (http://) to another machine. Use https://, or a server on this computer." }
        : { tone: "hint", text: "This address is another machine, over plain http: what you ask travels unencrypted." };
    case "invalid":
      return url.trim() ? { tone: "danger", text: "Not a valid http:// or https:// address." } : null;
  }
}

/** Shortcuts: the tag next to a shortcut, from Rust's report (null: nothing to say). */
export function shortcutTag(
  duplicate: boolean,
  report: { status: ShortcutStatus; typed?: string; trigger?: string } | undefined,
): { tone: "neutral" | "warn" | "danger"; text: string } | null {
  if (duplicate) return { tone: "danger", text: "Used twice" };
  switch (report?.status) {
    case "inUse": return { tone: "warn", text: "In use by another app" };
    case "duplicate": return { tone: "danger", text: "Used twice" };
    case "invalid": return { tone: "danger", text: "Not a valid shortcut" };
    case "typesCharacter": return { tone: "warn", text: `Types “${report.typed ?? "?"}”` };
    case "unsupported": return { tone: "neutral", text: "Not available" };
    case "refused": return { tone: "warn", text: "Not set by your desktop" };
    // Wayland: the desktop may run it on other keys than the ones asked for.
    case "active": return report.trigger ? { tone: "neutral", text: `Desktop: ${report.trigger}` } : null;
    default: return null;
  }
}

/** Updates: the line after "Check now". */
export function updateFeedback(status: Pick<UpdateStatus, "check">): string {
  if (status.check === "upToDate") return "Next Agent is up to date";
  if (status.check === "unreachable") return "Couldn't reach GitHub";
  return "";
}

/** Phone link: what the status line says about the relay. */
export function relayStateText(state: PhoneRelayState): string {
  switch (state) {
    case "connecting": return "Reaching the relay…";
    case "waiting": return "Connected to the relay, waiting for the phone.";
    case "linked": return "The phone is connected through the relay.";
    case "accessRefused": return "The relay refused the access key.";
    case "unreachable": return "The relay cannot be reached. Trying again.";
    case "roomTaken": return "This pairing is in use somewhere else. Press Pair again.";
    case "tooMany": return "The relay is limiting connections from here. Trying again later.";
    default: return "Off";
  }
}

/** Agents → done: the backups written (one per line) and what to do next. */
export function agentDoneText(backups: string, note: string): string {
  const saved = backups ? `Done. Previous version saved as ${backups.split("\n").join(", ")}.` : "Done.";
  return `${saved} ${note}`;
}
