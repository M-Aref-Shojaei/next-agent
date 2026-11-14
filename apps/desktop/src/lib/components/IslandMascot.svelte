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
    tone?: "idle" | "working" | "approval" | "error" | "finished";
  } = $props();
</script>

<div
  class="mascot tone-{tone}"
  style="left:{cx - diameter / 2}px;top:{cy - diameter / 2}px;width:{diameter}px;height:{diameter}px;opacity:{opacity}"
  aria-hidden="true"
></div>

<style>
  /* Gradient orb in place of Coucou's character. Colour follows what the
     agent is doing (Coucou's BotEngine glow colours); it turns while
     working and breathes while an approval waits. */
  .mascot {
    position: absolute;
    border-radius: 50%;
    pointer-events: none;
    --tone: #9398a1;
    --tone-2: #5b5f66;
    background:
      radial-gradient(circle at 32% 28%, rgb(255 255 255 / 0.55), transparent 42%),
      conic-gradient(from 210deg, var(--tone), var(--tone-2), var(--tone));
    box-shadow: 0 0 calc(var(--glow, 6) * 1px) var(--tone);
    transition: opacity 0.25s ease-in-out, box-shadow 0.3s ease;
  }
  .tone-working { --tone: #3b9eff; --tone-2: #8b5cf6; animation: turn 2.4s linear infinite; }
  .tone-approval { --tone: #f5a524; --tone-2: #f4505e; animation: breathe 1.6s ease-in-out infinite; }
  .tone-error { --tone: #f4505e; --tone-2: #b4233c; }
  .tone-finished { --tone: #34d399; --tone-2: #0ea5e9; }
  @keyframes turn { to { transform: rotate(360deg); } }
  @keyframes breathe { 50% { --glow: 14; transform: scale(1.06); } }
  @media (prefers-reduced-motion: reduce) {
    .mascot { animation: none !important; }
  }
</style>
