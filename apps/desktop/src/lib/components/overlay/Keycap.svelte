<script lang="ts">
  // A dictation key as Flow writes it ("Right Ctrl", "Alt + Shift + F ×2"):
  // free text, so not <Kbd>, which parses "Ctrl+Shift+K" chords.
  let { text, active = false }: { text: string; active?: boolean } = $props();
</script>

<span class="keycap" class:active aria-live={active ? "polite" : undefined}>{text}</span>

<style>
  .keycap {
    display: inline-flex;
    align-items: center;
    min-height: var(--control-height-sm);
    padding: 0 var(--space-2);
    border: var(--border-width) solid var(--color-border);
    border-bottom-width: var(--border-width-strong);
    border-radius: var(--radius-sm);
    background: var(--color-surface-raised);
    font-family: var(--font-mono);
    font-size: var(--font-size-sm);
    color: var(--color-text);
    white-space: nowrap;
  }
  .active { border-color: var(--color-accent); color: var(--color-accent-text); }
</style>
