// The overlay's settings as Settings → Overlay shows them: one copy for every
// page, read once from `boot` and kept in step with the island through
// `settings-changed`, as Coucou's settings window kept its own copy.
import { DEFAULT_SETTINGS, type Settings } from "$coucou/core/state";
import { sanitizeDeclared } from "$coucou/core/pills";
import type { FlowPrefs, FlowStatus, ModelsStatus } from "$coucou/core/flow";
import { onOverlayEvent, overlay } from "./overlay-settings.ts";

class OverlayStore {
  settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  /** The overlay's version (Cargo), for About and Updates. */
  version = $state("");
  loaded = $state(false);
  #loading: Promise<void> | null = null;

  /** Reads the settings once; later calls wait for the same read. */
  load(): Promise<void> {
    this.#loading ??= (async () => {
      const boot = await overlay.boot();
      if (boot) {
        this.settings = { ...this.settings, ...boot.settings };
        this.version = boot.version;
      }
      // A main pill this build can run, and no pill declared twice (settings/main.ts render()).
      this.settings = { ...this.settings, ...sanitizeDeclared(this.settings) };
      await onOverlayEvent<Settings>("settings-changed", (s) => {
        this.settings = { ...this.settings, ...s };
      });
      this.loaded = true;
    })();
    return this.#loading;
  }

  /** Coucou's `save()`: the whole settings object, as the window sent it. */
  save(): Promise<void | null> {
    return overlay.saveSettings($state.snapshot(this.settings) as Settings);
  }

  /** Changes some settings, then saves them. */
  set(change: Partial<Settings>): Promise<void | null> {
    this.settings = { ...this.settings, ...change };
    return this.save();
  }
}

export const overlayStore = new OverlayStore();

/** Before Rust answers (hub/shell.ts DEFAULT_STATUS). */
const DEFAULT_FLOW: FlowStatus = {
  prefs: {
    enabled: false, key: "right_ctrl", keyCombo: [], edge: "bottom", hidePill: false, mic: "", keepDays: 30, cleanup: false,
    defaultStyle: "natural", summaryEngine: "claude", openAtLaunch: true, voice: false, voiceOutput: "system", voiceGender: "female",
    muteOthers: true, oneTapHandsFree: false,
  },
  models: { parakeet: { state: "missing" }, downloadMb: 700 },
  keys: "ok",
  hasClaudeKey: false,
  mics: [],
  platform: typeof navigator !== "undefined" && navigator.userAgent.includes("Windows") ? "windows" : "linux",
  autostarted: false,
};

/**
 * Flow's state (dictation, the Notetaker, voice) as the Coucou window's pages
 * shared it (hub/shell.ts HubContext): read with `refresh`, changed with
 * `setPrefs`, and kept in step by `flow-prefs` and `speech-models`.
 */
class FlowStore {
  status = $state<FlowStatus>(DEFAULT_FLOW);
  #listening = false;

  async refresh(): Promise<FlowStatus> {
    if (!this.#listening) {
      this.#listening = true;
      await onOverlayEvent<FlowPrefs>("flow-prefs", (prefs) => (this.status = { ...this.status, prefs }));
      await onOverlayEvent<ModelsStatus>("speech-models", (models) => (this.status = { ...this.status, models }));
    }
    this.status = (await overlay.flowStatus()) ?? this.status;
    return this.status;
  }

  /** Saves new preferences and keeps them as the page's. */
  async setPrefs(change: Partial<FlowPrefs>): Promise<void> {
    const next = { ...$state.snapshot(this.status.prefs), ...change } as FlowPrefs;
    this.status = { ...this.status, prefs: next };
    await overlay.flowSetPrefs(next);
  }

  /** setPrefs, then a fresh status: the window's pages redrew after these changes. */
  setPrefsAndRefresh(change: Partial<FlowPrefs>): void {
    void this.setPrefs(change).then(() => this.refresh());
  }
}

export const flowStore = new FlowStore();
