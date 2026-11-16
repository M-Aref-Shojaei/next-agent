// Shared state of the upstream watch, for Settings → Upstreams and its badge.
import { log } from "./log.ts";
import { api } from "./api.ts";
import type { WatchView } from "./upstreams.ts";

class UpstreamStore {
  view = $state<WatchView | null>(null);
  error = $state<string | null>(null);
  #started = false;

  get unseen(): number {
    return this.view?.unseen.length ?? 0;
  }

  /** Load once and follow the background checks. */
  async init() {
    if (this.#started) return;
    this.#started = true;
    try {
      this.view = await api.upstreamWatchStatus();
      await api.onUpstreamWatch((view) => (this.view = view));
    } catch (e) {
      log.warn("ui_error", { kind: "upstreams", error: String(e) });
      this.error = String(e);
      // Try again on the next visit.
      this.#started = false;
    }
  }

  async checkNow() {
    this.error = null;
    if (this.view) this.view = { ...this.view, checking: true };
    try {
      this.view = await api.upstreamWatchCheck();
    } catch (e) {
      log.warn("ui_error", { kind: "upstreams", error: String(e) });
      this.error = String(e);
      if (this.view) this.view = { ...this.view, checking: false };
    }
  }

  async markSeen() {
    try {
      this.view = await api.upstreamWatchMarkSeen();
    } catch (e) {
      log.warn("ui_error", { kind: "upstreams", error: String(e) });
      this.error = String(e);
    }
  }

  async open(url: string) {
    try {
      await api.upstreamOpen(url);
    } catch (e) {
      log.warn("ui_error", { kind: "upstreams", error: String(e) });
      this.error = String(e);
    }
  }
}

export const upstreams = new UpstreamStore();
