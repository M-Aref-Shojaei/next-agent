<script lang="ts">
  // Settings → Overlay → Chat: the island's chat (settings/main.ts
  // subscriptionsSection, apiSection, chatProvidersSection, localSection;
  // settings/claude-models.ts). Keys go to the system keychain and never come
  // back to the page: it only learns whether one is stored.
  import { log } from "$lib/log.ts";
  import { onMount } from "svelte";
  import { Banner, Button, Select, SettingsRow, SettingsSection, StatusDot, Switch, TextField } from "$lib/ds/index.ts";
  import AccentDot from "$lib/components/overlay/AccentDot.svelte";
  import { apiState, errorText, exposureNotice, keyStoreName, overlay } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import { CUSTOM_SERVER_KEY, providerDef, urlExposure } from "$coucou/core/providers";
  import { CLAUDE_MODELS } from "$coucou/core/claude-models";
  import type { ModelInfo, SubscriptionStatus } from "$coucou/core/bridge";

  const KEY_STORE = keyStoreName(navigator.userAgent);
  const STORED = "••••••••  (stored)";
  const s = $derived(store.settings);

  // ── Your AI subscriptions ───────────────────────────────────────────────────
  /** The plan people know, by provider. */
  const SUBSCRIPTION_NAMES: Record<string, string> = {
    anthropic: "Claude", google: "Google Gemini", openai: "ChatGPT", cursor: "Cursor", copilot: "GitHub Copilot",
  };
  let subscriptions = $state<SubscriptionStatus[]>([]);
  function setSubscription(provider: string, on: boolean) {
    void overlay.subscriptionSet(provider, on).then((next) => {
      if (next) subscriptions = next;
    });
  }

  // ── Claude ──────────────────────────────────────────────────────────────────
  let hasKey = $state(false);
  let claudeReady = $state(false);
  let apiKey = $state("");
  let apiFeedback = $state<{ tone: "ok" | "danger"; text: string } | null>(null);

  async function refreshClaude() {
    hasKey = (await overlay.secretPresent("anthropic-api-key")) ?? false;
    claudeReady = (await overlay.claudeReady()) ?? false;
    void reloadModels(hasKey);
  }
  async function saveApiKey() {
    const value = apiKey.trim();
    if (!value) return;
    apiFeedback = null;
    try {
      await overlay.secretSet("anthropic-api-key", value);
      apiKey = "";
      apiFeedback = { tone: "ok", text: "Saved. It never touches disk." };
      await refreshClaude();
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_chat" });
      apiFeedback = { tone: "danger", text: `Could not save: ${String(err)}` };
    }
  }
  async function removeApiKey() {
    apiFeedback = null;
    try {
      await overlay.secretClear("anthropic-api-key");
      apiFeedback = { tone: "ok", text: "Key removed." };
      await refreshClaude();
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_chat" });
      apiFeedback = { tone: "danger", text: `Could not remove: ${String(err)}` };
    }
  }

  // The model (claude-models.ts): account discovery replaces the choices when a
  // key is present; a saved model is never replaced automatically.
  let models = $state<ModelInfo[]>(CLAUDE_MODELS);
  let modelsKey = $state(false);
  let loadingModels = $state(false);
  let savingModel = $state(false);
  let modelLoadError = $state("");
  let modelSaveError = $state("");
  let generation = 0;
  const modelOptions = $derived.by(() => {
    const choices = new Map(models.map((m) => [m.id, m.label]));
    if (s.model && !choices.has(s.model)) choices.set(s.model, s.model);
    return [...choices].map(([value, label]) => ({ value, label }));
  });
  const modelFeedback = $derived(modelSaveError || modelLoadError || (loadingModels ? "Loading models…" : ""));

  async function reloadModels(present = modelsKey) {
    modelsKey = present;
    const request = ++generation;
    loadingModels = present;
    modelLoadError = "";
    if (!present) {
      models = CLAUDE_MODELS;
      return;
    }
    try {
      // Explicit API discovery, never redirected by a Claude Code subscription switch.
      const received = await overlay.claudeApiModels();
      if (request !== generation) return;
      models = received;
    } catch (error) {
      log.warn("ui_error", { kind: "overlay_chat" });
      if (request !== generation) return;
      modelLoadError = errorText(error);
    } finally {
      if (request === generation) loadingModels = false;
    }
  }
  async function chooseModel(next: string) {
    if (savingModel || !next || next === s.model) return;
    savingModel = true;
    modelSaveError = "";
    try {
      await overlay.saveSettings({ ...$state.snapshot(store.settings), model: next } as typeof store.settings);
      store.settings = { ...store.settings, model: next };
    } catch (error) {
      log.warn("ui_error", { kind: "overlay_chat" });
      modelSaveError = errorText(error);
    } finally {
      savingModel = false;
    }
  }

  // ── Chat providers ──────────────────────────────────────────────────────────
  const CLOUD = [
    { id: "google", name: "Google AI", placeholder: "AIza…", where: "aistudio.google.com" },
    { id: "openai", name: "OpenAI", placeholder: "sk-…", where: "platform.openai.com" },
    { id: "openrouter", name: "OpenRouter", placeholder: "sk-or-…", where: "openrouter.ai/keys" },
  ].map((d) => ({ ...d, def: providerDef(d.id), key: providerDef(d.id).key! }));
  let present = $state<Record<string, boolean>>({});
  let cloudInput = $state<Record<string, string>>({});
  let cloudFailed = $state<Record<string, boolean>>({});
  async function saveCloud(key: string) {
    const value = (cloudInput[key] ?? "").trim();
    if (!value) return;
    try {
      await overlay.secretSet(key, value);
      present = { ...present, [key]: true };
      cloudInput = { ...cloudInput, [key]: "" };
      cloudFailed = { ...cloudFailed, [key]: false };
    } catch {
      cloudFailed = { ...cloudFailed, [key]: true };
    }
  }
  async function removeCloud(key: string) {
    try {
      await overlay.secretClear(key);
      present = { ...present, [key]: false };
      cloudFailed = { ...cloudFailed, [key]: false };
    } catch {
      cloudFailed = { ...cloudFailed, [key]: true };
    }
  }

  // ── Local models ────────────────────────────────────────────────────────────
  type LocalId = "ollama" | "lmstudio" | "custom";
  const LOCAL: { id: LocalId; name: string; usual: string }[] = [
    { id: "ollama", name: "Ollama", usual: "http://127.0.0.1:11434" },
    { id: "lmstudio", name: "LM Studio", usual: "http://127.0.0.1:1234" },
    // No usual address: any server that speaks the OpenAI API.
    { id: "custom", name: "OpenAI-compatible", usual: "" },
  ];
  let customKey = $state(false);
  let localUrl = $state<Record<string, string>>({});
  let localKey = $state("");
  let connecting = $state<Record<string, boolean>>({});
  let localStatus = $state<Record<string, string>>({});

  const field = (id: LocalId) => providerDef(id).urlField!;
  function exposureFor(id: LocalId, url: string, connected: boolean) {
    const withKey = id === "custom" && (customKey || (!connected && localKey.trim() !== ""));
    return exposureNotice(urlExposure(url), url, withKey);
  }

  async function useInChat(id: LocalId) {
    await store.set({ chatProvider: id });
  }
  async function disconnect(id: LocalId) {
    const change: Partial<typeof store.settings> = { [field(id)]: "" };
    if (s.chatProvider === id) change.chatProvider = "anthropic";
    if (id === "custom") {
      await overlay.secretClear(CUSTOM_SERVER_KEY).catch(() => {});
      customKey = false;
    }
    await store.set(change);
  }
  async function connect(id: LocalId, usual: string, name: string) {
    connecting = { ...connecting, [id]: true };
    localStatus = { ...localStatus, [id]: "" };
    const url = localUrl[id] ?? "";
    try {
      if (id === "custom" && localKey.trim()) {
        // Stored with this address: the key is only ever sent there.
        await overlay.localSetKey(url || usual, localKey.trim());
        localKey = "";
        customKey = true;
      }
      const server = await overlay.localConnect(id, url);
      if (!server.models.length) {
        localStatus = { ...localStatus, [id]: `No models yet. Download one in ${name} first.` };
      } else {
        const change: Partial<typeof store.settings> = { [field(id)]: server.url };
        if (!server.models.includes(s.chatModels[id] ?? "")) change.chatModels = { ...s.chatModels, [id]: server.models[0] };
        await store.set(change);
      }
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_chat" });
      localStatus = { ...localStatus, [id]: errorText(err) };
    }
    connecting = { ...connecting, [id]: false };
  }

  onMount(async () => {
    void overlay.subscriptionStatus().then((rows) => (subscriptions = rows ?? []));
    await refreshClaude();
    const next: Record<string, boolean> = {};
    for (const c of CLOUD) next[c.key] = (await overlay.secretPresent(c.key)) ?? false;
    present = next;
    customKey = (await overlay.secretPresent(CUSTOM_SERVER_KEY)) ?? false;
  });
</script>

<SettingsSection
  id="subscriptions"
  title="Your AI subscriptions"
  description="Chat on the plan you already pay for. Next Agent asks your AI's official terminal tool, signed in with your own account: it never sees your password or your key."
>
  {#each subscriptions as row (row.provider)}
    {@const p = providerDef(row.provider)}
    <SettingsRow
      label={SUBSCRIPTION_NAMES[row.provider] ?? p.name}
      description={[
        row.installed ? `${row.tool} found ✓ Not signed in yet? Run this once in a terminal: ${row.signIn}` : `${row.tool} not found.`,
        !row.subscriptionOnly && !row.on ? "Off: the chat uses your API key below." : "",
      ].filter(Boolean).join(" ")}
    >
      {#snippet children(ctl)}
        <AccentDot color={p.accent} />
        {#if !row.installed}<Button size="sm" onclick={() => void overlay.openUrl(row.installUrl)}>Install it</Button>{/if}
        <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={row.on} onchange={(on) => setSubscription(row.provider, on)} />
      {/snippet}
    </SettingsRow>
  {/each}
</SettingsSection>

<SettingsSection id="claude" title="Claude">
  {#snippet actions()}
    <StatusDot tone={hasKey || claudeReady ? "ok" : "danger"} text={apiState(hasKey, claudeReady && !hasKey, KEY_STORE)} />
  {/snippet}
  <SettingsRow label="API key">
    {#snippet children(ctl)}
      <TextField
        id={ctl.id}
        type="password"
        mono
        autocomplete="off"
        spellcheck="false"
        placeholder={hasKey ? `••••••••••••  (stored)` : "sk-ant-..."}
        bind:value={apiKey}
      />
      <Button variant="primary" onclick={saveApiKey}>Save key</Button>
      {#if hasKey}<Button variant="danger" onclick={removeApiKey}>Remove</Button>{/if}
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Model" description={modelFeedback || undefined}>
    {#snippet children(ctl)}
      <Select id={ctl.id} aria-describedby={ctl.describedby} options={modelOptions} value={s.model} disabled={savingModel} onchange={(e) => void chooseModel(e.currentTarget.value)} />
      <Button disabled={!modelsKey || loadingModels || savingModel} onclick={() => void reloadModels()}>Refresh</Button>
    {/snippet}
  </SettingsRow>
  {#snippet footer()}
    {#if apiFeedback}<Banner tone={apiFeedback.tone}>{apiFeedback.text}</Banner>{/if}
  {/snippet}
</SettingsSection>

<SettingsSection
  id="providers"
  title="Chat providers"
  description="Chat with Google AI, OpenAI or OpenRouter instead of Claude: add a key here, then click the model name above the chat box to switch provider and model. Keys stay in the system keychain. These providers get no web search and no tools: they can answer, never act on this computer."
>
  {#each CLOUD as c (c.id)}
    <SettingsRow label={c.name} description="Key from {c.where}">
      {#snippet children(ctl)}
        <AccentDot color={c.def.accent} />
        <TextField
          id={ctl.id}
          aria-describedby={ctl.describedby}
          type="password"
          mono
          autocomplete="off"
          spellcheck="false"
          placeholder={present[c.key] ? STORED : c.placeholder}
          bind:value={() => cloudInput[c.key] ?? "", (v) => (cloudInput = { ...cloudInput, [c.key]: String(v ?? "") })}
        />
        <Button onclick={() => void saveCloud(c.key)}>Save</Button>
        {#if present[c.key]}<Button variant="danger" onclick={() => void removeCloud(c.key)}>Remove</Button>{/if}
        <StatusDot
          tone={cloudFailed[c.key] ? "warn" : present[c.key] ? "ok" : "danger"}
          label={cloudFailed[c.key] ? "Could not reach the keychain" : present[c.key] ? "Key stored" : "No key"}
        />
      {/snippet}
    </SettingsRow>
  {/each}
</SettingsSection>

<SettingsSection
  id="local"
  title="Local models"
  description="Chat with a model you run yourself: Ollama or LM Studio (leave the address empty for the usual one on this computer), or any server that speaks the OpenAI API, such as vLLM or llama.cpp. Once connected, pick it above the chat box."
>
  {#each LOCAL as l (l.id)}
    {@const p = providerDef(l.id)}
    {@const url = s[field(l.id)]}
    {@const connected = url !== ""}
    {@const notice = exposureFor(l.id, connected ? url : localUrl[l.id] || l.usual, connected)}
    <SettingsRow label={l.name} description={connected ? url : undefined} error={localStatus[l.id] || undefined} stacked={!connected}>
      {#snippet children(ctl)}
        <AccentDot color={p.accent} />
        {#if connected}
          <StatusDot tone="ok" label="Connected" />
          <Button variant={s.chatProvider === l.id ? "secondary" : "primary"} disabled={s.chatProvider === l.id} onclick={() => void useInChat(l.id)}>
            {s.chatProvider === l.id ? "In use" : "Use in chat"}
          </Button>
          <Button variant="danger" onclick={() => void disconnect(l.id)}>Disconnect</Button>
        {:else}
          <TextField
            id={ctl.id}
            aria-describedby={ctl.describedby}
            mono
            spellcheck="false"
            autocomplete="off"
            placeholder={l.usual || "https://llm.example.com"}
            bind:value={() => localUrl[l.id] ?? "", (v) => (localUrl = { ...localUrl, [l.id]: String(v ?? "") })}
          />
          {#if l.id === "custom"}
            <TextField
              aria-label="API key (optional)"
              type="password"
              mono
              spellcheck="false"
              autocomplete="off"
              placeholder={customKey ? STORED : "API key (optional)"}
              bind:value={localKey}
            />
          {/if}
          <Button variant="primary" loading={connecting[l.id]} onclick={() => void connect(l.id, l.usual, l.name)}>
            {connecting[l.id] ? "Connecting…" : "Connect"}
          </Button>
        {/if}
      {/snippet}
    </SettingsRow>
    {#if notice}
      <div class="notice">
        {#if notice.tone === "hint"}<p>{notice.text}</p>{:else}<Banner tone={notice.tone}>{notice.text}</Banner>{/if}
      </div>
    {/if}
  {/each}
</SettingsSection>

<style>
  .notice { padding: var(--space-2) var(--space-4); }
  .notice p { margin: 0; font-size: var(--font-size-sm); color: var(--color-text-secondary); }
</style>
