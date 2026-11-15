<script lang="ts">
  // The island's mascot slot: where Coucou draws its character (Mochi, which
  // is not part of Coucou's MIT licence and is not used here). Positioned and
  // sized exactly like Coucou's `BotPlacement`: centred on (cx, cy) in the
  // island's coordinates, `diameter` across. Replace the placeholder below
  // with your own artwork; keep the props.
  let {
    cx,
    cy,
    diameter,
    opacity = 1,
    tone = "idle",
  }: {
    cx: number;
    cy: number;
    diameter: number;
    opacity?: number;
    /** What the agent is doing: idle, working, approval, error, finished. */
    tone?: "rest" | "idle" | "working" | "approval" | "error" | "finished";
  } = $props();
</script>

<div
  class="mascot tone-{tone}"
  style="left:{cx - diameter / 2}px;top:{cy - diameter / 2}px;width:{diameter}px;height:{diameter}px;opacity:{opacity}"
  aria-hidden="true"
></div>

<style>
  /* Gradient orb in place of Coucou's character. Colour follows what the
     agent is doing; it turns while working and breathes while an approval
     waits. At rest it is the brand mark (--brand-gradient). */
  .mascot {
    position: absolute;
    border-radius: var(--radius-full);
    pointer-events: none;
    --tone: var(--color-neutral);
    --tone-2: var(--color-neutral-text);
    background: conic-gradient(from 210deg, var(--tone), var(--tone-2), var(--tone));
    box-shadow: 0 0 calc(var(--glow, 6) * 1px) var(--tone);
    transition: opacity var(--duration-slow) var(--ease-standard), box-shadow var(--duration-slow) var(--ease-standard);
  }
  .tone-working { --tone: var(--color-info); --tone-2: var(--color-accent); animation: turn 2.4s linear infinite; }
  .tone-approval { --tone: var(--color-warn); --tone-2: var(--color-danger); animation: breathe 1.6s ease-in-out infinite; }
  .tone-error { --tone: var(--color-danger); --tone-2: var(--color-danger-text); }
  .tone-rest { --tone: var(--color-brand); --glow: 4; background: var(--brand-gradient); }
  .tone-finished { --tone: var(--color-ok); --tone-2: var(--color-info); }
  @keyframes turn { to { transform: rotate(360deg); } }
  @keyframes breathe { 50% { --glow: 14; transform: scale(1.06); } }
  @media (prefers-reduced-motion: reduce) {
    .mascot { animation: none !important; }
  }
</style>
