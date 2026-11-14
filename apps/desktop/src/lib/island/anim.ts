// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src/core/anim.ts @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Spring mirrors SwiftUI `.spring(response:dampingFraction:)` and the close
// curve is `.timingCurve(0.45, 0, 0.2, 1, duration: 0.34)`, so the island
// grows and folds exactly like Coucou's (IslandRootView.swift: openSpring /
// closeEase). Pure: runs under `node --test`.

export type EaseFn = (t: number) => number;

export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
export const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** Grow: `.spring(response: 0.5, dampingFraction: 0.72)`. */
export const OPEN_SPRING = { response: 0.5, damping: 0.72 } as const;
/** Shrink: `.timingCurve(0.45, 0, 0.2, 1, duration: 0.34)`. */
export const CLOSE_MS = 340;

/** cubic-bezier(x1,y1,x2,y2), solved on x by bisection. */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): EaseFn {
  const cx = (t: number) => (1 - t) ** 2 * 3 * t * x1 + 3 * (1 - t) * t * t * x2 + t ** 3;
  const cy = (t: number) => (1 - t) ** 2 * 3 * t * y1 + 3 * (1 - t) * t * t * y2 + t ** 3;
  return (x) => {
    let lo = 0;
    let hi = 1;
    let t = x;
    for (let i = 0; i < 12; i++) {
      if (cx(t) < x) lo = t;
      else hi = t;
      t = (lo + hi) / 2;
    }
    return cy(t);
  };
}

export const closeCurve = cubicBezier(0.45, 0, 0.2, 1);

/**
 * SwiftUI-equivalent spring: ω₀ = 2π / response, ζ = dampingFraction.
 * Integrated per frame (sub-stepped) so a dropped frame never destabilises it.
 */
export class Spring {
  value: number;
  target: number;
  velocity = 0;
  omega: number;
  zeta: number;

  constructor(value: number, response: number = OPEN_SPRING.response, damping: number = OPEN_SPRING.damping) {
    this.value = value;
    this.target = value;
    this.omega = (2 * Math.PI) / response;
    this.zeta = damping;
  }

  configure(response: number, damping: number) {
    this.omega = (2 * Math.PI) / response;
    this.zeta = damping;
  }

  set(value: number) {
    this.value = value;
    this.target = value;
    this.velocity = 0;
  }

  get settled(): boolean {
    return Math.abs(this.target - this.value) < 0.01 && Math.abs(this.velocity) < 0.05;
  }

  step(dt: number) {
    const steps = Math.max(1, Math.ceil(dt / (1 / 240)));
    const h = dt / steps;
    for (let i = 0; i < steps; i++) {
      const acc =
        this.omega * this.omega * (this.target - this.value) - 2 * this.zeta * this.omega * this.velocity;
      this.velocity += acc * h;
      this.value += this.velocity * h;
    }
  }
}

/**
 * A value driven by a spring (growing) or a timed curve (shrinking), as
 * IslandContainer animates the island's width, height and corner radius.
 */
export class Tracked {
  private spring: Spring;
  private curveFrom = 0;
  private curveTo = 0;
  private curveStart = 0;
  private curveDur = 0;
  private mode: "spring" | "curve" | "idle" = "idle";

  constructor(value: number) {
    this.spring = new Spring(value);
  }

  get value(): number {
    return this.spring.value;
  }

  get target(): number {
    return this.spring.target;
  }

  get animating(): boolean {
    return this.mode !== "idle";
  }

  /** No animation (reduced motion, a new screen). */
  jump(v: number) {
    this.spring.set(v);
    this.mode = "idle";
  }

  /** Spring to `v` (open / grow). */
  springTo(v: number, response: number = OPEN_SPRING.response, damping: number = OPEN_SPRING.damping) {
    this.spring.configure(response, damping);
    this.spring.target = v;
    this.mode = "spring";
  }

  /** Timed curve to `v` (close / shrink), no overshoot. */
  curveTowards(v: number, now: number, durationMs = CLOSE_MS) {
    this.curveFrom = this.spring.value;
    this.curveTo = v;
    this.curveStart = now;
    this.curveDur = durationMs;
    this.spring.target = v;
    this.spring.velocity = 0;
    this.mode = "curve";
  }

  step(dt: number, now: number) {
    if (this.mode === "spring") {
      this.spring.step(dt);
      if (this.spring.settled) {
        this.spring.value = this.spring.target;
        this.spring.velocity = 0;
        this.mode = "idle";
      }
    } else if (this.mode === "curve") {
      const p = clamp((now - this.curveStart) / this.curveDur, 0, 1);
      this.spring.value = lerp(this.curveFrom, this.curveTo, closeCurve(p));
      if (p >= 1) {
        this.spring.velocity = 0;
        this.mode = "idle";
      }
    }
  }
}
