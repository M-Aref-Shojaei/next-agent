// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandStateMachine.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Pure 4-state FSM for island open/close logic, without the greeting and
// voice states (`.coucou`, `.listening`), which our overlay does not have.
// No DOM, no Tauri: it communicates via `onTransition`. Timers are injected
// so tests drive a fake clock (Coucou uses DispatchWorkItems).

export type FsmState = "hidden" | "petit" | "home";

export interface Timers {
  set(fn: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}

export const realTimers: Timers = {
  set: (fn, ms) => setTimeout(fn, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

export class IslandStateMachine {
  state: FsmState = "hidden";

  /** Fired on every transition: (from, to). */
  onTransition: ((from: FsmState, to: FsmState) => void) | null = null;

  /** When it returns true, timers and mouse-leave never auto-collapse or hide the island. */
  isHeldOpen: (() => boolean) | null = null;

  /** petit → hidden delay, seconds (AppState.absenceInterval default: 3 min). */
  petitToHiddenDelay = 3 * 60;
  /**
   * Opt-in (Settings → General → Behavior): hovering opens the island, and an island
   * opened that way folds shortly after the pointer leaves. Off: hover only peeks.
   */
  openOnHover = false;
  /** Grace period after the pointer leaves a hover-opened island. */
  hoverCloseDelay = 0.6;

  private homeDelay = 15;
  private byHover = false;
  private petitHideWork: unknown = null;
  private homeCollapseWork: unknown = null;
  private timers: Timers;

  constructor(timers: Timers = realTimers) {
    this.timers = timers;
  }

  /** home → petit delay (seconds), kept in sync with the auto-close preference (default 15). */
  get homeToPetitDelay(): number {
    return this.homeDelay;
  }
  set homeToPetitDelay(seconds: number) {
    if (seconds === this.homeDelay) return;
    this.homeDelay = seconds;
    if (this.state === "home" && this.homeCollapseWork != null) this.scheduleHomeCollapse();
  }

  /** True while the island is open because of a hover, until the user clicks inside it. */
  get openedByHover(): boolean {
    return this.byHover;
  }

  private held(): boolean {
    return this.isHeldOpen?.() === true;
  }

  // MARK: – Inputs

  /** Mouse entered the island notch area. */
  mouseEntered() {
    if (this.openOnHover && (this.state === "hidden" || this.state === "petit") && !this.held()) {
      this.cancelTimers();
      this.byHover = true;
      this.transition("home");
      return;
    }
    switch (this.state) {
      case "hidden":
        if (this.held()) {
          // Island already expanded by an external call — sync FSM state without transition.
          this.state = "home";
        } else {
          this.cancelTimers();
          this.transition("petit");
        }
        break;
      case "petit":
        this.clearPetitHide();
        break;
      case "home":
        this.clearHomeCollapse();
        break;
    }
  }

  /** Mouse left the island notch area. */
  mouseLeft() {
    switch (this.state) {
      case "hidden":
        break;
      case "petit":
        this.schedulePetitHide();
        break;
      case "home":
        if (!this.held()) this.scheduleHomeCollapse();
        break;
    }
  }

  /**
   * Compact island clicked. Also accepts `hidden`: after an alert the island can be on
   * screen while the FSM never saw the mouse enter, and the click must still open it.
   */
  click() {
    this.byHover = false;
    if (this.state !== "petit" && this.state !== "hidden") return;
    this.cancelTimers();
    this.transition("home");
  }

  /** The app hid the island on its own (the last task ended). Mirror it without side effects. */
  hiddenExternally() {
    if (this.state !== "petit") return;
    this.cancelTimers();
    this.state = "hidden";
  }

  /** The app expanded the island externally (an approval). Sync to `home` without a transition. */
  openedExternally() {
    this.cancelTimers();
    this.byHover = false;
    if (this.state === "home") return;
    this.state = "home";
  }

  /** The app folded the island itself (the close button, an answered approval). */
  collapse() {
    this.byHover = false;
    if (this.state !== "home") return;
    this.cancelTimers();
    this.transition("petit");
  }

  /** Non-alert work event: show compact from hidden. */
  reveal() {
    if (this.state !== "hidden") return;
    this.cancelTimers();
    this.transition("petit");
    this.schedulePetitHide();
  }

  /** The user clicked inside: a hover-opened island now gets the normal auto-close. */
  userInteracted() {
    this.byHover = false;
  }

  // MARK: – Timers

  private schedulePetitHide() {
    this.clearPetitHide();
    this.petitHideWork = this.timers.set(() => {
      this.petitHideWork = null;
      if (this.state !== "petit" || this.held()) return;
      this.transition("hidden");
    }, this.petitToHiddenDelay * 1000);
  }

  private scheduleHomeCollapse() {
    this.clearHomeCollapse();
    const delay = this.byHover ? this.hoverCloseDelay : this.homeDelay;
    this.homeCollapseWork = this.timers.set(() => {
      this.homeCollapseWork = null;
      if (this.state !== "home" || this.held()) return;
      this.byHover = false;
      this.transition("petit");
    }, delay * 1000);
  }

  private clearPetitHide() {
    if (this.petitHideWork != null) this.timers.clear(this.petitHideWork);
    this.petitHideWork = null;
  }

  private clearHomeCollapse() {
    if (this.homeCollapseWork != null) this.timers.clear(this.homeCollapseWork);
    this.homeCollapseWork = null;
  }

  cancelTimers() {
    this.clearPetitHide();
    this.clearHomeCollapse();
  }

  private transition(next: FsmState) {
    if (next === this.state) return;
    const from = this.state;
    this.state = next;
    this.onTransition?.(from, next);
  }
}
