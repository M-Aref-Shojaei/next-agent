<script lang="ts">
  // Settings → Overlay → Voice: « OK Next Agent » (Coucou's hub/pages/voice.ts):
  // turning it on, how it answers out loud, and what leaves the computer.
  import { onMount } from "svelte";
  import { Banner, Button, SegmentedControl, SettingsRow, SettingsSection, Switch, TextField } from "$lib/ds/index.ts";
  import VoiceModel from "$lib/components/overlay/VoiceModel.svelte";
  import { keyStoreName, overlay } from "$lib/overlay-settings.ts";
  import { flowStore } from "$lib/overlay-store.svelte.ts";
  import type { VoiceOutput } from "$coucou/core/flow";

  const ELEVEN = "elevenlabs-api-key";
  const KEY_STORE = keyStoreName(navigator.userAgent);

  const status = $derived(flowStore.status);
  const prefs = $derived(status.prefs);
  const ready = $derived(status.models.parakeet.state === "ready");

  const outputs: { value: VoiceOutput; label: string }[] = [
    { value: "system", label: "System voice" },
    { value: "elevenlabs", label: "ElevenLabs" },
    { value: "off", label: "Text only" },
  ];
  const genders: { value: "female" | "male"; label: string }[] = [
    { value: "female", label: "Female" },
    { value: "male", label: "Male" },
  ];


  // The ElevenLabs key: saved in the keychain, never shown again.
  let elevenPresent = $state(false);
  let elevenKey = $state("");
  let elevenFailed = $state(false);
  const readEleven = async () => (elevenPresent = (await overlay.secretPresent(ELEVEN)) ?? false);
  function saveEleven() {
    const value = elevenKey.trim();
    if (!value) return;
    overlay.secretSet(ELEVEN, value).then(
      () => {
        elevenKey = "";
        elevenFailed = false;
        void readEleven();
      },
      () => (elevenFailed = true),
    );
  }

  onMount(() => {
    void flowStore.refresh();
    void readEleven();
  });
</script>

<SettingsSection id="listening" title="Listening" description="Say « OK Next Agent », then ask anything: add a pill, play music, the weather, an email to prepare.">
  <SettingsRow label="Listen for « OK Next Agent »" description="The microphone stays open; only what follows « OK Next Agent » goes further.">
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={prefs.voice} onchange={(on) => flowStore.setPrefsAndRefresh({ voice: on })} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Parakeet, on your computer" description="Hears « OK Next Agent » here, without the internet.">
    {#snippet children()}<VoiceModel onready={() => void flowStore.refresh()} />{/snippet}
  </SettingsRow>
  {#snippet footer()}
    {#if prefs.voice && !status.hasClaudeKey}
      <Banner tone="warn">Next Agent answers with Claude: add your Claude key in Overlay → Chat.</Banner>
    {:else if prefs.voice && !ready}
      <Banner tone="warn">Download the voice model so Next Agent can hear you.</Banner>
    {/if}
  {/snippet}
</SettingsSection>

<SettingsSection id="answers" title="Answers">
  <SettingsRow label="Next Agent answers out loud">
    {#snippet children(ctl)}
      <SegmentedControl labelledby={ctl.labelId} size="sm" options={outputs} value={prefs.voiceOutput} onchange={(v) => flowStore.setPrefsAndRefresh({ voiceOutput: v })} />
    {/snippet}
  </SettingsRow>
  {#if prefs.voiceOutput === "elevenlabs"}
    <SettingsRow label="Voice">
      {#snippet children(ctl)}
        <SegmentedControl labelledby={ctl.labelId} size="sm" options={genders} value={prefs.voiceGender} onchange={(g) => void flowStore.setPrefs({ voiceGender: g })} />
      {/snippet}
    </SettingsRow>
    <SettingsRow label="ElevenLabs key" description={elevenPresent ? `✓ Key saved in the ${KEY_STORE}.` : undefined} error={elevenFailed ? "Could not save the key." : undefined}>
      {#snippet children(ctl)}
        {#if elevenPresent}
          <Button size="sm" onclick={() => void overlay.secretClear(ELEVEN).then(readEleven)}>Remove</Button>
        {:else}
          <TextField
            id={ctl.id}
            type="password"
            mono
            autocomplete="off"
            spellcheck="false"
            placeholder="ElevenLabs key"
            bind:value={elevenKey}
            onkeydown={(e) => { if (e.key === "Enter") saveEleven(); }}
          />
          <Button size="sm" variant="primary" onclick={saveEleven}>Save key</Button>
        {/if}
      {/snippet}
    </SettingsRow>
  {/if}
</SettingsSection>

<SettingsSection id="voice-privacy" title="Privacy">
  {#snippet footer()}
    <p>Your voice never leaves this computer: Parakeet turns it into text here. Only the words after « OK Next Agent » go to Anthropic with your Claude key, and to ElevenLabs if you picked it for the answers.</p>
    <p>An email is never sent by Next Agent: your mail app opens with it written, and you send it.</p>
  {/snippet}
</SettingsSection>

<style>
  p { margin: 0 0 var(--space-2); }
</style>
