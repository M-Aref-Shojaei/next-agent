<script lang="ts">
  // Settings → Overlay → Dictation: Flow's settings (hub/pages/flowsettings.ts:
  // the key, the pill, the microphone, the voice model, the window, the
  // history) and its styles (hub/pages/styles.ts). The Notetaker's summaries
  // have their own page.
  import { onMount } from "svelte";
  import {
    Banner, Button, Dialog, IconButton, Icon, SegmentedControl, Select, SettingsRow, SettingsSection, Switch,
  } from "$lib/ds/index.ts";
  import Keycap from "$lib/components/overlay/Keycap.svelte";
  import FlowKeyRecorder from "$lib/components/overlay/FlowKeyRecorder.svelte";
  import VoiceModel from "$lib/components/overlay/VoiceModel.svelte";
  import { errorText, overlay } from "$lib/overlay-settings.ts";
  import { flowStore } from "$lib/overlay-store.svelte.ts";
  import { flowKeyId, type AppStyle, type Edge, type FlowKey, type Style } from "$coucou/core/flow";
  import { keyLabel } from "$coucou/hub/ui";

  const status = $derived(flowStore.status);
  const prefs = $derived(status.prefs);
  const wayland = $derived(status.keys === "wayland");
  const keyId = $derived(wayland ? "wayland" : flowKeyId(prefs));
  const keycap = (twice: boolean) => {
    const label = keyLabel(keyId, status.platform);
    return twice ? (label.includes(" + ") ? `${label} ×2` : `${label} ${label}`) : label;
  };

  const keyOptions = $derived<{ value: FlowKey; label: string }[]>([
    { value: "right_ctrl", label: "Right Ctrl" },
    { value: "right_alt", label: "Right Alt" },
    { value: "right_shift", label: "Right Shift" },
    { value: "left_ctrl", label: "Left Ctrl" },
    { value: "left_alt", label: "Left Alt" },
    { value: "left_shift", label: "Left Shift" },
    { value: "left_ctrl_super", label: status.platform === "windows" ? "Ctrl + Win" : "Ctrl + Super" },
    { value: "custom", label: "A shortcut of your own…" },
  ]);
  const edges: { value: Edge; label: string }[] = [
    { value: "bottom", label: "Bottom" },
    { value: "bottom-left", label: "Bottom-left" },
    { value: "bottom-right", label: "Bottom-right" },
    { value: "left", label: "Left" },
    { value: "right", label: "Right" },
  ];
  const micOptions = $derived.by(() => {
    const mics = [{ value: "", label: "System default" }, ...status.mics.map((m) => ({ value: m.name, label: m.name }))];
    if (prefs.mic && !mics.some((m) => m.value === prefs.mic)) mics.push({ value: prefs.mic, label: prefs.mic });
    return mics;
  });
  const keepOptions = [
    { value: "0", label: "Don't keep" },
    { value: "7", label: "7 days" },
    { value: "30", label: "30 days" },
    { value: "365", label: "1 year" },
    { value: "-1", label: "Forever" },
  ];


  // ── Privacy: one write at a time (hub/save.ts) ────────────────────────────────
  let erasing = $state(false);
  let eraseError = $state("");
  let confirmErase = $state(false);
  async function erase(everything: boolean) {
    if (erasing) return;
    erasing = true;
    eraseError = "";
    try {
      await overlay.flowClear(everything);
    } catch (cause) {
      eraseError = cause instanceof Error ? cause.message : String(cause);
      throw cause;
    } finally {
      erasing = false;
    }
  }

  // ── Styles ────────────────────────────────────────────────────────────────────
  const styleOptions: { value: Style; label: string }[] = [
    { value: "verbatim", label: "As said" },
    { value: "natural", label: "Natural" },
    { value: "polished", label: "Polished" },
  ];
  let styles = $state<AppStyle[]>([]);
  let stylesError = $state("");
  let savingStyles = $state(false);
  let running = $state<{ id: string; name: string }[]>([]);
  let addValue = $state("");
  const addOptions = $derived([
    { value: "", label: "+ Add an app" },
    ...(running.length === 0 ? [{ value: "-", label: "No other open app", disabled: true }] : running.map((a) => ({ value: `${a.id}\n${a.name}`, label: a.name }))),
  ]);

  /** Keeps the form as it was until the native store confirms the write. */
  async function saveStyles(next: AppStyle[]) {
    if (savingStyles) return;
    savingStyles = true;
    stylesError = "";
    try {
      await overlay.flowSetStyles(next);
      styles = next;
    } catch (cause) {
      stylesError = cause instanceof Error ? cause.message : String(cause);
      // Shows the actual selection again when the store rejects it.
      styles = [...styles];
    } finally {
      savingStyles = false;
    }
  }
  async function fillAdd() {
    running = ((await overlay.flowRunningApps()) ?? []).filter((a) => !styles.some((s) => s.id === a.id));
    addValue = "";
  }
  async function addApp(value: string) {
    const [id, name] = value.split("\n");
    if (!id || id === "-") return;
    await saveStyles([...styles, { id, name: name ?? id, style: prefs.defaultStyle }]);
    void fillAdd();
  }

  onMount(async () => {
    await flowStore.refresh();
    styles = (await overlay.flowStyles()) ?? [];
    void fillAdd();
  });
</script>

<SettingsSection id="keys" title="Keys">
  <SettingsRow label="Hold a key to dictate anywhere">
    {#snippet children(ctl)}
      <Switch id={ctl.id} checked={prefs.enabled} onchange={(on) => void flowStore.setPrefs({ enabled: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Hold to dictate"
    description={!wayland && prefs.key === "right_alt" ? "On a keyboard with AltGr, the right Alt types characters: pick another key." : undefined}
  >
    {#snippet children(ctl)}
      {#if wayland}
        <Keycap text={keycap(false)} />
      {:else}
        <Select id={ctl.id} aria-describedby={ctl.describedby} options={keyOptions} value={prefs.key} onchange={(e) => flowStore.setPrefsAndRefresh({ key: e.currentTarget.value as FlowKey })} />
      {/if}
    {/snippet}
  </SettingsRow>
  {#if prefs.key === "custom" && !wayland}
    <SettingsRow label="Your shortcut" description="Hold the keys together to dictate. Its letter no longer reaches the app in front while you hold them.">
      {#snippet children(ctl)}
        <FlowKeyRecorder
          id={ctl.id}
          describedby={ctl.describedby}
          combo={prefs.keyCombo}
          platform={status.platform}
          ondone={(combo) => flowStore.setPrefsAndRefresh({ key: "custom", keyCombo: combo })}
        />
      {/snippet}
    </SettingsRow>
  {/if}
  <SettingsRow label="Where the pill is">
    {#snippet children(ctl)}
      <SegmentedControl labelledby={ctl.labelId} size="sm" options={edges} value={prefs.edge} onchange={(edge) => void flowStore.setPrefs({ edge })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Hide the pill until I dictate" description="It shows only while you dictate, record a call or get a message. The dictation key still works.">
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={prefs.hidePill} onchange={(on) => void flowStore.setPrefs({ hidePill: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Hands-free"
    description={prefs.oneTapHandsFree ? "Tap once, speak as long as you like, tap again to paste." : "Tap twice, speak as long as you like, tap once to paste."}
  >
    {#snippet children()}<Keycap text={keycap(!prefs.oneTapHandsFree)} />{/snippet}
  </SettingsRow>
  <SettingsRow label="Hands-free with one press">
    {#snippet children(ctl)}
      <Switch id={ctl.id} checked={prefs.oneTapHandsFree} onchange={(on) => flowStore.setPrefsAndRefresh({ oneTapHandsFree: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Paste the last dictation again" description="When the words went to the wrong place.">
    {#snippet children(ctl)}
      <Button id={ctl.id} size="sm" onclick={() => void overlay.flowPasteAgain()}>Paste again</Button>
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Cancel a dictation">
    {#snippet children()}<Keycap text="Esc" />{/snippet}
  </SettingsRow>
  {#snippet footer()}
    {#if wayland}
      <Banner tone="warn">On Wayland, the desktop holds the dictation key for Next Agent: Ctrl+Alt+V, or the keys you picked when it asked. The first paste asks once to let Next Agent type for you.</Banner>
    {:else if status.keys !== "ok"}
      <Banner tone="warn">Next Agent can't watch the keyboard here: use the pill's microphone to dictate.</Banner>
    {/if}
  {/snippet}
</SettingsSection>

<SettingsSection id="microphone" title="Microphone">
  <SettingsRow label="Microphone" description="The one Next Agent listens to for dictation and calls.">
    {#snippet children(ctl)}
      <Select id={ctl.id} aria-describedby={ctl.describedby} options={micOptions} value={prefs.mic} onchange={(e) => void flowStore.setPrefs({ mic: e.currentTarget.value })} />
    {/snippet}
  </SettingsRow>
  {#if status.platform === "windows"}
    <!-- Silence while you speak (flow/hush.rs): Windows mutes each app on its own. -->
    <SettingsRow
      label="Mute the computer while you speak"
      description="Videos, music and calls go quiet while Next Agent listens to you, then come back. Next Agent's own sounds keep playing."
    >
      {#snippet children(ctl)}
        <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={prefs.muteOthers} onchange={(on) => void flowStore.setPrefs({ muteOthers: on })} />
      {/snippet}
    </SettingsRow>
  {/if}
</SettingsSection>

<SettingsSection id="recognition" title="Recognition">
  <SettingsRow
    label="Parakeet, on your computer"
    description="The most accurate, offline, 25 languages. Downloaded once (about {status.models.downloadMb} MB)."
  >
    {#snippet children()}<VoiceModel />{/snippet}
  </SettingsRow>
  {#snippet footer()}Your voice never leaves this computer: Parakeet turns it into text here, and the recording is dropped as soon as it is pasted.{/snippet}
</SettingsSection>

<SettingsSection id="styles" title="Styles" description="Claude cleans up what you say, in the style of the app you write in.">
  <SettingsRow label="Clean up with Claude" description="Removes the “um”, the repeats and the corrections you made while speaking, and fixes the punctuation.">
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={prefs.cleanup} onchange={(on) => void flowStore.setPrefs({ cleanup: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Other apps">
    {#snippet children(ctl)}
      <Select id={ctl.id} options={styleOptions} value={prefs.defaultStyle} onchange={(e) => void flowStore.setPrefs({ defaultStyle: e.currentTarget.value as Style })} />
    {/snippet}
  </SettingsRow>
  {#each styles as app (app.id)}
    <SettingsRow label={app.name} disabled={!prefs.cleanup}>
      {#snippet children(ctl)}
        <Select
          id={ctl.id}
          options={styleOptions}
          value={app.style}
          disabled={savingStyles}
          onchange={(e) => void saveStyles(styles.map((x) => (x.id === app.id ? { ...x, style: e.currentTarget.value as Style } : x)))}
        />
        <IconButton label="Delete {app.name}" disabled={savingStyles} onclick={() => void saveStyles(styles.filter((x) => x.id !== app.id))}>
          <Icon name="x" size={14} />
        </IconButton>
      {/snippet}
    </SettingsRow>
  {/each}
  <SettingsRow label="Style per app">
    {#snippet children(ctl)}
      <Select
        id={ctl.id}
        options={addOptions}
        value={addValue}
        disabled={savingStyles}
        onfocus={() => void fillAdd()}
        onchange={(e) => void addApp(e.currentTarget.value)}
      />
    {/snippet}
  </SettingsRow>
  {#snippet footer()}
    <div class="notes">
      {#if !status.hasClaudeKey}<Banner tone="warn">The clean-up uses your Claude key. Add it in Overlay → Chat.</Banner>{/if}
      {#if stylesError}<Banner tone="danger">{stylesError}</Banner>{/if}
      <p>With the clean-up on, the text (never your voice) goes to Anthropic with your Claude key. Off, everything stays on this computer.</p>
    </div>
  {/snippet}
</SettingsSection>

<SettingsSection id="privacy" title="Privacy">
  <SettingsRow label="Keep the history">
    {#snippet children(ctl)}
      <Select id={ctl.id} options={keepOptions} value={String(prefs.keepDays)} onchange={(e) => void flowStore.setPrefs({ keepDays: Number(e.currentTarget.value) })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Erase">
    {#snippet children()}
      <Button size="sm" disabled={erasing} onclick={() => void erase(false).catch(() => {})}>Erase the history</Button>
      <Button size="sm" variant="danger" disabled={erasing} onclick={() => (confirmErase = true)}>Erase everything</Button>
    {/snippet}
  </SettingsRow>
  {#snippet footer()}
    <div class="notes">
      {#if eraseError}<Banner tone="danger">{errorText(eraseError)}</Banner>{/if}
      <p>The history, the statistics and your notes stay on this computer, in Next Agent's folder. Dictations are never saved as audio; a call's audio is deleted as soon as its transcript is written.</p>
    </div>
  {/snippet}
</SettingsSection>

<Dialog
  bind:open={confirmErase}
  tone="danger"
  title="Erase the history and the statistics?"
  confirmLabel="Erase everything"
  onconfirm={() => erase(true)}
/>

<style>
  .notes { display: grid; gap: var(--space-2); }
  .notes p { margin: 0; }
</style>
