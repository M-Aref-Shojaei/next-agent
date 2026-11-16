<script lang="ts">
  // Settings → Overlay → Phone link: Next Agent for Android (Coucou's
  // settings/phone.ts). Off until the user turns it on; each thing the phone
  // may see or do has its own switch, off by default, and Rust re-reads them
  // for every connection. Only this window (the main one) may change them.
  import { log } from "$lib/log.ts";
  import { onMount } from "svelte";
  import { Banner, Button, Checkbox, SettingsRow, SettingsSection, Switch, TextField } from "$lib/ds/index.ts";
  import { errorText, overlay, relayStateText } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import { modelId, modelsOf, toggleModel, usableProviders } from "$coucou/core/phone-chat";
  import { PROVIDERS, type ProviderDef } from "$coucou/core/providers";
  import type { PhoneLinkPairing, PhoneLinkStatus, PhoneRelayStatus } from "$coucou/core/bridge";

  // ── The link ────────────────────────────────────────────────────────────────
  let link = $state<PhoneLinkStatus | null>(null);
  let linkOn = $state(store.settings.phoneLink ?? false);
  let linkNote = $state("");
  let pairing = $state<PhoneLinkPairing | null>(null);
  let copied = $state(false);
  let copyLabel = $state("Copy link");
  const showError = (message: string) => (linkNote = `Could not turn it on: ${message}`);

  function drawLink(status: PhoneLinkStatus | null) {
    link = status;
    // The switch shows what Rust says, not what the page remembers.
    if (status) linkOn = status.enabled;
    pairing = null;
    linkNote = "";
    if (status?.error) showError(status.error);
  }
  async function setLink(next: boolean) {
    try {
      drawLink(await overlay.phoneLinkSetEnabled(next));
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      // It could not start: the switch goes back off and says why.
      linkOn = false;
      link = null;
      showError(String(err));
    }
  }
  /** The code itself, which lets a phone in: shown only when asked for. */
  async function reveal(newCode: boolean) {
    try {
      pairing = newCode ? await overlay.phoneLinkNewPairing() : await overlay.phoneLinkPairing();
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      showError(String(err));
    }
  }
  /** After the relay settings change, a code on screen would be stale: show the current one. */
  async function refreshPairing() {
    if (!pairing) return;
    try {
      pairing = await overlay.phoneLinkPairing();
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      showError(String(err));
    }
  }
  function copyLink() {
    if (!pairing) return;
    void navigator.clipboard?.writeText(pairing.link).then(() => {
      copied = true; // stays after the button text is restored
      copyLabel = "Copied";
      window.setTimeout(() => (copyLabel = "Copy link"), 1500);
    });
  }
  const address = $derived(link?.host ? `${link.host}:${link.port}` : `:${link?.port ?? ""}`);

  // ── What the phone may see or do: one switch each ───────────────────────────
  interface Flag {
    id: string;
    label: string;
    hint: string;
    read: () => Promise<{ enabled: boolean } | null>;
    write: (on: boolean) => Promise<unknown>;
  }
  const FLAGS: Flag[] = [
    {
      id: "details",
      label: "Show session details on the phone",
      hint: "Sends the last steps (as shown on the island, so they can include commands), the final line, the project's folder name and the colour. Never a path.",
      read: overlay.phoneDetailsStatus,
      write: overlay.phoneDetailsSetEnabled,
    },
    {
      id: "answers",
      label: "Let the phone answer Claude Code's questions",
      hint: "The phone shows each question with its options. Every answer is confirmed with the phone's fingerprint or screen lock.",
      read: overlay.phoneAnswersStatus,
      write: overlay.phoneAnswersSetEnabled,
    },
    {
      id: "diffs",
      label: "Show the files an agent changed on the phone",
      hint: "The phone lists the files each agent changed and, when you tap one, shows what changed in it (200 lines at most, long lines cut). File names and lines go only to your paired phone, over the pinned encrypted link, and only when it asks.",
      read: overlay.phoneDiffsStatus,
      write: overlay.phoneDiffsSetEnabled,
    },
    {
      id: "usage",
      label: "Show my plan usage on the phone",
      hint: "The phone shows how much of your Claude and Codex plans is used and when they reset: the same percentages as the pills here, nothing else.",
      read: overlay.phoneUsageStatus,
      write: overlay.phoneUsageSetEnabled,
    },
    {
      id: "recap",
      label: "Show my weekly recap on the phone",
      hint: "The phone shows last week's recap: time coding, sessions, files, lines, commands, permissions and the agent you used most. Counts only. The project's name is added only if you also show session details.",
      read: overlay.phoneRecapStatus,
      write: overlay.phoneRecapSetEnabled,
    },
  ];
  let flags = $state<Record<string, boolean>>({});
  let flagNotes = $state<Record<string, string>>({});
  async function setFlag(flag: Flag, next: boolean) {
    flagNotes = { ...flagNotes, [flag.id]: "" };
    try {
      await flag.write(next);
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      flags = { ...flags, [flag.id]: false };
      flagNotes = { ...flagNotes, [flag.id]: String(err) };
    }
  }

  // ── Services: read-only cards, none ticked until the user does it ───────────
  const PHONE_SERVICES: [string, string][] = [
    ["integration_stripe", "Stripe"], ["integration_github", "GitHub"], ["integration_vercel", "Vercel"],
    ["integration_n8n", "n8n"], ["integration_resend", "Resend"], ["integration_notion", "Notion"], ["integration_calcom", "Cal.com"],
  ];
  let ticked = $state<string[]>([]);
  let servicesNote = $state("");
  async function tick(id: string, on: boolean) {
    servicesNote = "";
    const next = on ? [...new Set([...ticked, id])] : ticked.filter((x) => x !== id);
    try {
      ticked = (await overlay.phoneServicesSet(next)).services;
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      ticked = [...ticked];
      servicesNote = String(err);
    }
  }

  // ── Chat from the phone: only the models ticked here ─────────────────────────
  let chatOn = $state(false);
  let chatNote = $state("");
  let allowed = $state<string[]>([]);
  let providers = $state<ProviderDef[] | null>(null);
  let offered = $state<Record<string, { loading: boolean; models: string[]; error: string } | undefined>>({});

  async function drawChat(status: { enabled: boolean; models: string[] }) {
    allowed = status.models;
    providers = null;
    if (!status.enabled) return;
    const hasKey: Record<string, boolean> = {};
    for (const p of PROVIDERS.filter((x) => x.key !== null)) hasKey[p.id] = (await overlay.keyReady(p.key!)) ?? false;
    providers = usableProviders(store.settings, hasKey);
  }
  async function setChat(next: boolean) {
    chatNote = "";
    try {
      await drawChat(await overlay.phoneChatSetEnabled(next));
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      chatOn = false;
      providers = null;
      chatNote = String(err);
    }
  }
  /** Asks the provider for its models only now, on the click. */
  function chooseModels(p: ProviderDef) {
    offered = { ...offered, [p.id]: { loading: true, models: [], error: "" } };
    overlay.chatModels(p.id).then(
      (models) => (offered = { ...offered, [p.id]: { loading: false, models: models.map((m) => m.id), error: "" } }),
      (err) => (offered = { ...offered, [p.id]: { loading: false, models: [], error: errorText(err) } }),
    );
  }
  async function allow(id: string, on: boolean) {
    try {
      allowed = (await overlay.phoneChatSetModels(toggleModel(allowed, id, on))).models;
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      chatNote = String(err);
    }
  }

  // ── Away from home Wi-Fi: a relay the user deployed ─────────────────────────
  let relay = $state<PhoneRelayStatus | null>(null);
  let relayUrl = $state("");
  let editingUrl = $state(false);
  let relayAccess = $state("");
  let relayNote = $state("");
  function showRelay(status: PhoneRelayStatus | null) {
    if (!status) return;
    relay = status;
    if (!editingUrl) relayUrl = status.url;
  }
  async function applyRelay(enabled: boolean) {
    relayNote = "";
    try {
      showRelay(await overlay.phoneRelaySet(enabled, relayUrl));
      void refreshPairing();
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      relayNote = String(err);
      if (relay) relay = { ...relay, enabled: false };
    }
  }
  async function saveAccess() {
    relayNote = "";
    try {
      const next = await overlay.phoneRelaySetAccess(relayAccess);
      relayAccess = ""; // never kept on the page
      showRelay(next);
      void refreshPairing();
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_phone", error: String(err) });
      relayNote = String(err);
    }
  }

  // Read again when the link turns on: the window drew these under the switch then.
  const running = $derived(link?.running ?? false);
  $effect(() => {
    if (!running) return;
    for (const f of FLAGS) void f.read().then((st) => st && (flags = { ...flags, [f.id]: st.enabled }));
    void overlay.phoneServicesStatus().then((st) => st && (ticked = st.services));
    void overlay.phoneChatStatus().then((st) => {
      if (!st) return;
      chatOn = st.enabled;
      void drawChat(st);
    });
    // The relay block is hidden until a relay is ready to use (testers, or one already on).
    void overlay.phoneRelayStatus().then((st) => st?.shown && showRelay(st));
  });

  onMount(() => {
    void overlay.phoneLinkStatus().then(drawLink);
    // Kept up to date while the page is open and the link is on; nothing runs otherwise.
    const watch = window.setInterval(() => {
      if (document.visibilityState !== "visible") return;
      if (link?.running) {
        void overlay.phoneLinkStatus().then((st) => {
          if (st?.running && link) link = { ...link, clients: st.clients };
        });
      }
      if (relay) void overlay.phoneRelayStatus().then(showRelay);
    }, 5000);
    return () => window.clearInterval(watch);
  });
</script>

<SettingsSection
  id="link"
  title="Android phone"
  description="Send your agents' sessions to Next Agent for Android and answer permission requests from the phone. Off by default; it only listens on your local network."
>
  <SettingsRow label="Phone link" description="Allow on the phone asks for the fingerprint or the screen lock. Deny does not." error={linkNote || undefined}>
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} bind:checked={linkOn} onchange={(on) => void setLink(on)} />
    {/snippet}
  </SettingsRow>
  {#if link?.running}
    <SettingsRow label="Phones connected: {link.clients}" description="The phone connects to {address}">
      {#snippet children()}
        {#if !pairing}<Button onclick={() => void reveal(false)}>Show pairing code</Button>{/if}
      {/snippet}
    </SettingsRow>
    {#if pairing}
      <SettingsRow label="Pairing code" description="In Next Agent for Android, scan this code with the camera or paste the link." stacked>
        {#snippet children(ctl)}
          <div class="pairing">
            <!-- The QR comes from Rust (qrcode crate) as an SVG made only of paths. -->
            <div class="qr">{@html pairing!.qrSvg}</div>
            <div class="link-row">
              <TextField id={ctl.id} mono readonly value={pairing!.link} onfocus={(e) => e.currentTarget.select()} />
              <Button onclick={copyLink}>{copyLabel}</Button>
            </div>
            {#if copied}
              <Banner tone="warn">The link you copied contains the pairing code: anyone who gets it can pair a phone with this computer. If it ends up somewhere it should not, press Pair again to make a new code; the old one stops working.</Banner>
            {/if}
            {#if pairing!.relay}
              <p class="hint">This code also carries the access key and the end-to-end key for the relay {pairing!.relay}. Pair again makes a new key and a new room.</p>
            {/if}
            <div class="link-row">
              <Button onclick={() => void reveal(true)}>Pair again</Button>
              <span class="hint">The phone that was paired is disconnected and needs the new code.</span>
            </div>
          </div>
        {/snippet}
      </SettingsRow>
    {/if}
  {/if}
  {#snippet footer()}
    {#if link?.running}
      <p>If Windows asks about the firewall, allow Next Agent on private networks. The phone and this computer must be on the same Wi-Fi.</p>
      <p>Your phone finds this computer again by itself when the network changes. It is announced on your local network only while this switch is on. Some hotspots and guest networks block this.</p>
    {/if}
  {/snippet}
</SettingsSection>

{#if link?.running}
  {#if relay}
    <SettingsSection
      id="relay"
      title="Away from home Wi-Fi"
      description="Reach this computer from your phone on mobile data or another network, through a relay you deployed yourself. Everything is encrypted end to end: the relay only forwards data it cannot read. Off by default."
    >
      <SettingsRow label="Away from home Wi-Fi" description={relay.enabled ? relayStateText(relay.state) : undefined} error={relayNote || undefined}>
        {#snippet children(ctl)}
          <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={relay!.enabled} onchange={(on) => void applyRelay(on)} />
        {/snippet}
      </SettingsRow>
      <SettingsRow label="Relay address">
        {#snippet children(ctl)}
          <TextField id={ctl.id} mono placeholder="wss://" autocomplete="off" spellcheck="false" bind:value={relayUrl} onfocus={() => (editingUrl = true)} onblur={() => (editingUrl = false)} />
          <Button onclick={() => void applyRelay(relay!.enabled)}>Save address</Button>
        {/snippet}
      </SettingsRow>
      <SettingsRow label="Relay access key" description="Paste the access key you set when you deployed the relay. It is kept in the system keystore and never shown again.">
        {#snippet children(ctl)}
          <TextField id={ctl.id} aria-describedby={ctl.describedby} type="password" mono autocomplete="off" spellcheck="false" placeholder={relay!.hasAccess ? "••••••••" : ""} bind:value={relayAccess} />
          <Button onclick={() => void saveAccess()}>Save key</Button>
        {/snippet}
      </SettingsRow>
    </SettingsSection>
  {/if}

  <SettingsSection id="phone-sees" title="What the phone may see">
    {#each FLAGS as flag (flag.id)}
      <SettingsRow label={flag.label} description={flag.hint} error={flagNotes[flag.id] || undefined}>
        {#snippet children(ctl)}
          <Switch
            id={ctl.id}
            aria-describedby={ctl.describedby}
            checked={flags[flag.id] ?? false}
            onchange={(on) => {
              flags = { ...flags, [flag.id]: on };
              void setFlag(flag, on);
            }}
          />
        {/snippet}
      </SettingsRow>
    {/each}
  </SettingsSection>

  <SettingsSection
    id="phone-services"
    title="Show these services on the phone"
    description="Each tick lets your paired phone see that service's card: a headline and up to three short lines, read-only. Nothing is shown until you tick it. No addresses, subjects, links or keys are ever sent."
  >
    {#each PHONE_SERVICES as [id, name] (id)}
      <SettingsRow label={name}>
        {#snippet children(ctl)}
          <Checkbox id={ctl.id} checked={ticked.includes(id)} onchange={(on) => void tick(id, on)} />
        {/snippet}
      </SettingsRow>
    {/each}
    {#snippet footer()}{#if servicesNote}<Banner tone="danger">{servicesNote}</Banner>{/if}{/snippet}
  </SettingsSection>

  <SettingsSection id="phone-chat" title="Chat from the phone">
    <SettingsRow
      label="Let the phone chat with my AI providers"
      description="Chat uses the API keys saved on this computer. The phone never sees them, and each message may cost money."
      error={chatNote || undefined}
    >
      {#snippet children(ctl)}
        <Switch id={ctl.id} aria-describedby={ctl.describedby} bind:checked={chatOn} onchange={(on) => void setChat(on)} />
      {/snippet}
    </SettingsRow>
    {#if providers}
      {#if providers.length === 0}
        <SettingsRow label="Models the phone may use" description="No provider is ready: add an API key or connect a local model first." />
      {/if}
      {#each providers as p (p.id)}
        {@const list = offered[p.id]}
        <SettingsRow label={p.name} description={list?.loading ? "Loading models…" : list?.error || undefined} stacked={!!list?.models.length || modelsOf(allowed, p.id).length > 0}>
          {#snippet children()}
            <div class="models">
              <Button size="sm" onclick={() => chooseModels(p)}>Choose models</Button>
              {#each [...new Set([...(list?.models ?? []), ...modelsOf(allowed, p.id)])] as model (model)}
                {@const id = modelId(p.id, model)}
                <Checkbox label={model} checked={allowed.includes(id)} onchange={(on) => void allow(id, on)} />
              {/each}
            </div>
          {/snippet}
        </SettingsRow>
      {/each}
    {/if}
    {#snippet footer()}
      {#if chatOn && providers && allowed.length === 0}Nothing is allowed yet: the phone cannot chat until you tick a model.{/if}
    {/snippet}
  </SettingsSection>
{/if}

<style>
  .pairing { display: grid; gap: var(--space-3); }
  .qr {
    width: 180px;
    padding: var(--space-2);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    border: var(--border-width) solid var(--color-border);
    color: var(--color-text);
  }
  .qr :global(svg) { display: block; width: 100%; height: auto; }
  .link-row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); --ds-control-width: 100%; }
  .link-row > :global(:first-child) { flex: 1; }
  .hint, p { margin: 0 0 var(--space-2); font-size: var(--font-size-sm); color: var(--color-text-secondary); }
  .models { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-4); }
</style>
