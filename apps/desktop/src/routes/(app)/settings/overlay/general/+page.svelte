<script lang="ts">
  // Settings → Overlay → General: Coucou's General section (settings/main.ts
  // generalSection, recapRows, languageRow), same commands and same rules.
  import { onMount } from "svelte";
  import { Button, Select, SettingsRow, SettingsSection, Slider, Switch, TextField } from "$lib/ds/index.ts";
  import { clampAutoClose, overlay, resolveScreen } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import { closedIsland } from "$coucou/core/closed";
  import { SOUND_NAMES } from "$coucou/core/sound";
  import { LANGUAGES } from "$coucou/i18n/i18n";

  const s = $derived(store.settings);

  // Your own sounds: the folder, a reload, and how many are replaced.
  let customCount = $state(0);
  const countCustom = () => void overlay.customSounds(SOUND_NAMES).then((own) => (customCount = own?.length ?? 0));
  function reloadSounds() {
    void overlay.reloadSounds().then(() => window.setTimeout(countCustom, 300));
  }

  // The closed island (core/closed.ts): what stays at the top of the screen.
  const closedOptions = [
    { value: "mochi", label: "Next Agent, as on a Mac without a notch" },
    { value: "time", label: "The time" },
    { value: "datetime", label: "The date and the time" },
    { value: "claude", label: "Claude usage" },
    // Next Agent: removed WhatsApp/Spotify (owner request): "What's playing".
    { value: "empty", label: "Nothing, just the island" },
    { value: "hidden", label: "Hide the island" },
  ];

  let monitors = $state<{ key: string; label: string }[]>([]);
  let monitorsRead = $state(false);
  const screenOptions = $derived([
    { value: "primary", label: "Main display" },
    { value: "cursor", label: "Display under the cursor" },
    ...monitors.map((m) => ({ value: m.key, label: m.label })),
  ]);
  // Until the displays are listed the saved value shows as it is, as the window did.
  const screenValue = $derived(monitorsRead ? resolveScreen(s.screen, monitors.map((m) => m.key)) : s.screen);

  // A field being edited keeps what is in it; it catches up when it is left.
  let autoClose = $state(String(Math.round(store.settings.autoCloseInterval)));
  let editingAutoClose = $state(false);
  $effect(() => {
    if (!editingAutoClose) autoClose = String(Math.round(s.autoCloseInterval));
  });
  function saveAutoClose() {
    const value = clampAutoClose(autoClose);
    autoClose = String(value);
    void store.set({ autoCloseInterval: value });
  }

  // Weekly recap: the prefs live with the history in Rust.
  let recapOn = $state(true);
  let clearing = $state(false);
  let cleared = $state(false);
  async function clearRecap() {
    clearing = true;
    await overlay.recapClear();
    cleared = true;
    window.setTimeout(() => {
      clearing = false;
      cleared = false;
    }, 2400);
  }

  const languageOptions = [{ value: "", label: "System" }, ...LANGUAGES.map(({ code, name }) => ({ value: code, label: name }))];
  const languageValue = $derived(LANGUAGES.some((l) => l.code === s.language) ? s.language : "");

  onMount(() => {
    countCustom();
    void overlay.listMonitors().then((list) => {
      monitors = list ?? [];
      monitorsRead = true;
    });
    void overlay.recapPrefs().then((prefs) => {
      if (prefs) recapOn = prefs.enabled;
    });
  });
</script>

<SettingsSection id="sound" title="Sound">
  <SettingsRow label="Sound">
    {#snippet children(ctl)}
      <Slider
        aria-label="Volume"
        min={0}
        max={0.2}
        step={0.005}
        value={s.soundVolume}
        format={(v) => `${Math.round((v / 0.2) * 100)}%`}
        oninput={(e) => void store.set({ soundVolume: Number(e.currentTarget.value) })}
      />
      <Switch id={ctl.id} checked={s.soundEnabled} onchange={(on) => void store.set({ soundEnabled: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Your own sounds"
    description="Drop a file named like one of Next Agent's sounds (finish.wav, approval.mp3, greet.m4a…) in the sounds folder to replace it, then Reload."
  >
    {#snippet children()}
      {#if customCount > 0}<span class="note">{customCount} custom</span>{/if}
      <Button onclick={() => void overlay.revealSoundsFolder()}>Open sounds folder</Button>
      <Button onclick={reloadSounds}>Reload sounds</Button>
    {/snippet}
  </SettingsRow>
</SettingsSection>

<SettingsSection id="island" title="Island">
  <SettingsRow
    label="Open on hover"
    description="Hovering the island opens it; it folds again shortly after the pointer leaves. Click inside to keep it open."
  >
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={s.openOnHover} onchange={(on) => void store.set({ openOnHover: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Open for every pill’s alerts"
    description="When a pill other than the one shown finishes or hits an error, the notch shows it for a moment, then the pill you had comes back."
  >
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={s.alertsFromAllPills} onchange={(on) => void store.set({ alertsFromAllPills: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Show clock in the island" description="The date and the time in the middle of the island when it is compact.">
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={s.showClock} onchange={(on) => void store.set({ showClock: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Closed island shows"
    description="A PC has no notch: once closed, a small island stays at the top of the screen. Hover it to bring Next Agent back. Hidden, move the pointer to the very top of the screen."
  >
    {#snippet children(ctl)}
      <Select
        id={ctl.id}
        aria-describedby={ctl.describedby}
        options={closedOptions}
        value={closedIsland(s.closedIsland)}
        onchange={(e) => void store.set({ closedIsland: e.currentTarget.value })}
      />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Auto-close" description="Seconds after you leave the island (5 to 120).">
    {#snippet children(ctl)}
      <TextField
        id={ctl.id}
        aria-describedby={ctl.describedby}
        type="number"
        min="5"
        max="120"
        step="1"
        style="--ds-control-width: 96px"
        bind:value={autoClose}
        onfocus={() => (editingAutoClose = true)}
        onblur={() => (editingAutoClose = false)}
        onchange={saveAutoClose}
      />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Island lives on">
    {#snippet children(ctl)}
      <Select id={ctl.id} options={screenOptions} value={screenValue} onchange={(e) => void store.set({ screen: e.currentTarget.value })} />
    {/snippet}
  </SettingsRow>
</SettingsSection>

<SettingsSection id="app" title="App">
  <SettingsRow label="Launch at startup">
    {#snippet children(ctl)}
      <Switch id={ctl.id} checked={s.autostart} onchange={(on) => void store.set({ autostart: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Language" description="The island, its windows and the tray menu.">
    {#snippet children(ctl)}
      <Select id={ctl.id} aria-describedby={ctl.describedby} options={languageOptions} value={languageValue} onchange={(e) => void store.set({ language: e.currentTarget.value })} />
    {/snippet}
  </SettingsRow>
</SettingsSection>

<SettingsSection id="recap" title="Weekly recap">
  <SettingsRow
    label="Keep a history of my coding sessions"
    description="Counts and project names only — never commands, files or prompts. Kept on this computer for 12 weeks."
  >
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} bind:checked={recapOn} onchange={(on) => void overlay.recapSetEnabled(on)} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Clear history">
    {#snippet children()}
      {#if cleared}<span class="note" role="status">History cleared.</span>{/if}
      <Button variant="danger" disabled={clearing} onclick={clearRecap}>Clear history</Button>
    {/snippet}
  </SettingsRow>
  {#snippet footer()}No telemetry. Network requests only go to the services you configure yourself.{/snippet}
</SettingsSection>

<style>
  .note { font-size: var(--font-size-sm); color: var(--color-text-secondary); }
</style>
