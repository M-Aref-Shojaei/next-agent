<script lang="ts">
  // Settings → Overlay → Updates: Coucou's update rows (settings/updates.ts),
  // with Next Agent's update source (owner request): empty by default, and
  // then nothing is checked.
  import { onMount } from "svelte";
  import { Button, SettingsRow, SettingsSection, Switch, TextField } from "$lib/ds/index.ts";
  import { onOverlayEvent, overlay, updateFeedback } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import { NO_UPDATE, isBusy, type UpdateStatus } from "$coucou/core/update";

  const OFF_NOTE = "Empty: update checking is off.";

  let status = $state<UpdateStatus>({ ...NO_UPDATE, current: store.version });
  let source = $state("");
  let editingSource = $state(false);
  let sourceError = $state("");
  let checking = $state(false);

  const off = $derived(!status.source);
  $effect(() => {
    if (!editingSource) source = status.source;
  });

  async function saveSource() {
    try {
      status = await overlay.updateSetSource(source.trim());
      sourceError = "";
    } catch {
      sourceError = "Not a GitHub owner/repo";
    }
  }

  async function check() {
    checking = true;
    const fresh = await overlay.updateCheck();
    if (fresh) status = fresh;
    checking = false;
  }

  onMount(() => {
    const stop = onOverlayEvent<UpdateStatus>("update-status", (fresh) => (status = fresh));
    void overlay.updateStatus().then((fresh) => {
      if (fresh) status = fresh;
    });
    return () => void stop.then((f) => f());
  });
</script>

<SettingsSection id="updates" title="Updates">
  <SettingsRow label="Update source" description={sourceError || (off ? OFF_NOTE : "The GitHub repository whose releases are looked at.")}>
    {#snippet children(ctl)}
      <TextField
        id={ctl.id}
        aria-describedby={ctl.describedby}
        mono
        placeholder="GitHub owner/repo"
        autocomplete="off"
        spellcheck="false"
        bind:value={source}
        onfocus={() => (editingSource = true)}
        onblur={() => (editingSource = false)}
      />
      <Button onclick={saveSource}>Save</Button>
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Tell me when a new version is out">
    {#snippet children(ctl)}
      <Switch id={ctl.id} checked={status.consent === true} onchange={(on) => void overlay.updateSetConsent(on)} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="You have {status.current || store.version}" description={updateFeedback(status) || undefined}>
    {#snippet children()}
      {#if status.latest}
        <Button variant="primary" onclick={() => void (status.canInstall ? overlay.updateShow() : overlay.updateOpenPage())}>
          {status.canInstall ? `Install ${status.latest.version}` : `Get ${status.latest.version} on GitHub`}
        </Button>
      {/if}
      <Button loading={checking} disabled={off || status.check === "checking" || isBusy(status.phase)} onclick={check}>Check now</Button>
    {/snippet}
  </SettingsRow>
  {#snippet footer()}
    {status.platform === "linux"
      ? "Next Agent looks at its own releases on GitHub a few times a day and tells you when one is out. Update it the way you installed it: AppImage, .deb, .rpm or your package manager."
      : "Next Agent looks at its own releases on GitHub a few times a day. An update is installed only after your click, once its SHA-256 matches the one GitHub publishes."}
  {/snippet}
</SettingsSection>
