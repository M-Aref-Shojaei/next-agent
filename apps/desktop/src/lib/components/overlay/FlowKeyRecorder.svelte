<script lang="ts">
  // The keys of a dictation shortcut of one's own, and "Record" (Coucou's
  // hub/pages/flowsettings.ts shortcutRecorder): press the keys together
  // (Alt + Shift + F, Left Ctrl…), let go, and they are kept. Esc cancels.
  import { onDestroy } from "svelte";
  import { Button } from "$lib/ds/index.ts";
  import Keycap from "./Keycap.svelte";
  import { comboFromKeys } from "$coucou/hub/keys";
  import { keyLabel } from "$coucou/hub/ui";

  let {
    combo,
    platform,
    id,
    describedby,
    ondone,
  }: {
    combo: string[];
    platform: "windows" | "linux";
    id?: string;
    describedby?: string;
    ondone: (combo: string[]) => void;
  } = $props();

  const label = (keys: string[]) => (keys.length ? keyLabel(`custom:${keys.join("+")}`, platform) : "—");

  let recording = $state(false);
  let shown = $state("");
  let held = new Set<string>();
  let seen: string[] = [];
  const text = $derived(recording ? shown : label(combo));

  function stop() {
    window.removeEventListener("keydown", down, true);
    window.removeEventListener("keyup", up, true);
    recording = false;
  }
  function down(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape") {
      stop();
      return;
    }
    held.add(e.code);
    seen = comboFromKeys([...held]);
    shown = seen.length ? label(seen) : "…";
  }
  // Done once every key is up: what was held together is the shortcut.
  function up(e: KeyboardEvent) {
    e.preventDefault();
    held.delete(e.code);
    if (held.size > 0) return;
    stop();
    if (seen.length) ondone(seen);
  }
  function start() {
    held = new Set();
    seen = [];
    shown = "Press the keys…";
    recording = true;
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
  }
  onDestroy(stop);
</script>

<span class="recorder">
  <Keycap {text} active={recording} />
  <Button {id} aria-describedby={describedby} size="sm" pressed={recording} onclick={() => (recording ? stop() : start())}>
    {recording ? "Cancel" : "Record"}
  </Button>
</span>

<style>
  .recorder { display: inline-flex; align-items: center; gap: var(--space-2); }
</style>
