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
>
  <span class="core"></span>
</div>

<style>
  /* Placeholder: a soft disc with a status dot, nothing more. */
  .mascot {
    position: absolute;
    border-radius: 50%;
    background: #1d1f23;
    display: grid;
    place-items: center;
    pointer-events: none;
    transition: opacity 0.25s ease-in-out;
    --tone: #9398a1;
  }
  .core {
    width: 36%;
    height: 36%;
    border-radius: 50%;
    background: var(--tone);
    box-shadow: 0 0 6px var(--tone);
  }
  /* BotEngine glow colours (windows/src/core/layout.ts botGlowColor). */
  .tone-working { --tone: #3b9eff; }
  .tone-approval { --tone: #f5a524; }
  .tone-error { --tone: #f4505e; }
  .tone-finished { --tone: #34d399; }
</style>
