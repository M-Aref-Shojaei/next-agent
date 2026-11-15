<script lang="ts">
  import Kbd from "./Kbd.svelte";
  import IconButton from "./IconButton.svelte";
  import Icon from "./Icon.svelte";
  import { describedBy } from "./types.ts";
  import {
    currentPlatform,
    recordPress,
    shortcutLabel,
    type KeyPress,
    type Platform,
    type Recorded,
  } from "./shortcut.ts";

  let {
    value = $bindable(""),
    /** Accessible name when no visible label names it (e.g. "Toggle island shortcut"). */
    label,
    /** Id for the capture button, so a <SettingsRow>/<Field> label points at it. */
    id,
    describedby,
    placeholder = "Not set",
    requireModifier = true,
    /** Show a clear (×) button while a shortcut is set. */
    clearable = true,
    disabled = false,
    platform = currentPlatform(),
    /** Replace the recorder (e.g. Coucou's, which also detects AltGr characters). */
    record = (e: KeyPress) => recordPress(e, { requireModifier }),
    onchange,
    /** Called when capture starts and stops; pause global shortcuts while true. */
    onrecording,
  }: {
    value?: string;
    label?: string;
    id?: string;
    describedby?: string;
    placeholder?: string;
    requireModifier?: boolean;
    clearable?: boolean;
    disabled?: boolean;
    platform?: Platform;
    record?: (e: KeyPress) => Recorded;
    onchange?: (keys: string) => void;
    onrecording?: (recording: boolean) => void;
  } = $props();

  const uid = $props.id();
  let recording = $state(false);
  let message = $state("");

  function setRecording(on: boolean) {
    if (recording === on) return;
    recording = on;
    message = on ? "Press the new shortcut. Esc cancels, Backspace clears." : "";
    onrecording?.(on);
  }

  function commit(keys: string) {
    value = keys;
    onchange?.(keys);
  }

  function onkeydown(event: KeyboardEvent) {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();
    const result = record(event);
    switch (result.kind) {
      case "pending": return;
      case "keys": commit(result.keys); setRecording(false); return;
      case "clear": commit(""); setRecording(false); return;
      case "cancel": setRecording(false); return;
      case "needsModifier": message = "Add Ctrl, Alt or ⌘ to the key."; return;
      case "unsupported": message = "That key can't be used in a shortcut."; return;
      case "typesCharacter":
        setRecording(false);
        message = `${shortcutLabel(result.keys, platform)} types "${result.typed}" on this keyboard; pick another.`;
        return;
    }
  }
</script>

<div class="ds-shortcut">
  <button
    {id}
    type="button"
    class="capture"
    class:recording
    {disabled}
    aria-label={label ? `${label}: ${value ? shortcutLabel(value, platform) : placeholder}` : undefined}
    aria-describedby={describedBy(describedby, recording && `${uid}-msg`)}
    aria-pressed={recording}
    onclick={() => setRecording(!recording)}
    {onkeydown}
    onblur={() => setRecording(false)}
  >
    {#if recording}
      <span class="rec">Recording…</span>
    {:else if value}
      <Kbd keys={value} {platform} />
    {:else}
      <span class="empty">{placeholder}</span>
    {/if}
  </button>
  {#if clearable && value && !recording}
    <IconButton label="Clear shortcut" size="sm" {disabled} onclick={() => commit("")}>
      <Icon name="x" size={14} />
    </IconButton>
  {/if}
  <span class="msg" id="{uid}-msg" aria-live="polite">{message}</span>
</div>

<style>
  .ds-shortcut { display: inline-flex; flex-wrap: wrap; align-items: center; gap: var(--space-1); }
  .msg { flex-basis: 100%; font-size: var(--font-size-xs); color: var(--color-text-secondary); }
  .msg:empty { display: none; }
  .capture {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 110px;
    height: var(--control-height-md);
    padding: 0 var(--space-2);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
    cursor: pointer;
    transition: border-color var(--duration-fast) var(--ease-standard), box-shadow var(--duration-fast) var(--ease-standard);
  }
  .capture:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .capture:focus-visible { outline: none; border-color: var(--color-accent); box-shadow: var(--field-focus-ring); }
  .capture.recording {
    border-color: var(--color-accent);
    background: var(--color-accent-subtle);
    box-shadow: var(--field-focus-ring);
  }
  .capture:disabled { opacity: 0.5; cursor: not-allowed; }
  .rec { color: var(--color-accent-text); font-size: var(--font-size-sm); font-weight: var(--font-weight-medium); }
  .empty { color: var(--color-text-tertiary); font-size: var(--font-size-sm); }
</style>
