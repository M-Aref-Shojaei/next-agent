<script lang="ts">
  import { log } from "$lib/log.ts";
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import {
    Badge, Banner, Button, List, ListRow, Select, SettingsLayout, SettingsRow, SettingsSection, Switch, TextArea,
    TextField, type Option, type SettingsNavItem,
  } from "$lib/ds/index.ts";
  import {
    ROLES,
    defaultSettings,
    parseList,
    type CavemanLevel,
    type Combo,
    type ComboStrategy,
    DEFAULT_THRESHOLD,
    EFFORTS,
    STRATEGIES,
    effectiveStrategy,
    parseWeights,
    THRESHOLDS,
    type CatalogStatus,
    type ConnectionTest,
    type GatewaySettings,
    type GatewayTarget,
    type ProviderConnection,
    type ProviderPreset,
  } from "$lib/gateway.ts";

  let presets = $state<ProviderPreset[]>([]);
  let connections = $state<ProviderConnection[]>([]);
  let combos = $state<Combo[]>([]);
  let targets = $state<GatewayTarget[]>([]);
  let settings = $state<GatewaySettings>(defaultSettings());
  let tests = $state<Record<string, ConnectionTest | "running">>({});
  let catalog = $state<CatalogStatus>({ models: 0, updatedAt: null });
  let cacheEntries = $state(0);
  let refreshing = $state(false);
  let message = $state("");
  let failed = $state(false);

  // Connection form. The key field is write-only: it is never filled from the backend.
  const blankConnection = () => ({
    id: "", provider: "openai", name: "", baseUrl: "", models: "", priority: 100, isActive: true, apiKey: "", clearKey: false,
  });
  let form = $state(blankConnection());
  const formPreset = $derived(presets.find((p) => p.id === form.provider));

  // Combo form.
  const blankCombo = () => ({
    id: "", name: "", models: "", strategy: "" as "" | ComboStrategy, stickyLimit: 1, threshold: DEFAULT_THRESHOLD, weights: "",
  });
  let comboForm = $state(blankCombo());
  const comboStrategy = $derived(effectiveStrategy(comboForm.strategy, settings.comboStrategy));
  const comboIsSmart = $derived(comboStrategy === "smart");

  async function load() {
    [presets, connections, combos, targets, settings, catalog, cacheEntries] = await Promise.all([
      api.gatewayPresets(),
      api.gatewayConnections(),
      api.gatewayCombos(),
      api.gatewayTargets(),
      api.gatewaySettingsGet(),
      api.gatewayCatalogStatus(),
      api.gatewayCacheCount(),
    ]);
    // Every role gets an editable entry; "off" ones are dropped on save.
    for (const role of ROLES) settings.reasoning[role] ??= { effort: "off", maxTokens: null };
  }
  onMount(() => void load().catch(fail));

  function fail(e: unknown) {
    log.warn("ui_error", { kind: "providers", error: String(e) });
    message = String(e);
    failed = true;
  }

  async function attempt(work: () => Promise<void>, done: string) {
    failed = false;
    try {
      await work();
    } catch (e) {
      fail(e);
      return;
    }
    // The action worked; a failed reload must not read as a failed save.
    try {
      message = done;
      await load();
    } catch (e) {
      fail(e);
    }
  }

  function editConnection(conn: ProviderConnection) {
    form = {
      id: conn.id, provider: conn.provider, name: conn.name, baseUrl: conn.baseUrlOverride,
      models: conn.models.join(", "), priority: conn.priority, isActive: conn.isActive, apiKey: "", clearKey: false,
    };
  }

  function saveConnection(event: SubmitEvent) {
    event.preventDefault();
    const input = {
      id: form.id || undefined, provider: form.provider, name: form.name, baseUrl: form.baseUrl,
      models: parseList(form.models), priority: Number(form.priority), isActive: form.isActive,
      apiKey: form.apiKey, clearKey: form.clearKey,
    };
    void attempt(async () => {
      await api.gatewayConnectionSave(input);
      form = blankConnection();
    }, "Connection saved. Keys are stored in the OS keychain.");
  }

  function removeConnection(conn: ProviderConnection) {
    void attempt(async () => {
      await api.gatewayConnectionDelete(conn.id);
    }, `Removed ${conn.name} and its stored key.`);
  }

  async function testConnection(conn: ProviderConnection) {
    tests = { ...tests, [conn.id]: "running" };
    try {
      tests = { ...tests, [conn.id]: await api.gatewayConnectionTest(conn.id) };
    } catch (e) {
      log.warn("ui_error", { kind: "providers", error: String(e) });
      tests = { ...tests, [conn.id]: { ok: false, detail: String(e), models: [] } };
    }
  }

  function saveSettings(event: SubmitEvent, done: string) {
    event.preventDefault();
    const clean: GatewaySettings = {
      ...settings,
      defaultModel: settings.defaultModel?.trim() || null,
      roles: Object.fromEntries(Object.entries(settings.roles).filter(([, v]) => v && v.trim())),
      reasoning: Object.fromEntries(
        Object.entries(settings.reasoning).filter(([, r]) => r && (r.effort !== "off" || (r.maxTokens ?? 0) > 0)),
      ),
      budgets: settings.budgets.filter((b) => Number(b.limitUsd) > 0).map((b) => ({ ...b, limitUsd: Number(b.limitUsd) })),
    };
    void attempt(() => api.gatewaySettingsSet(clean), done);
  }

  function editCombo(combo: Combo) {
    comboForm = {
      id: combo.id, name: combo.name, models: combo.models.join("\n"),
      strategy: combo.strategy ?? "", stickyLimit: combo.stickyLimit ?? 1,
      threshold: combo.confidenceThreshold ?? DEFAULT_THRESHOLD,
      weights: combo.weights.join(", "),
    };
  }

  function saveCombo(event: SubmitEvent) {
    event.preventDefault();
    const input: Combo = {
      id: comboForm.id, name: comboForm.name.trim(), models: parseList(comboForm.models),
      strategy: comboForm.strategy || null, stickyLimit: Number(comboForm.stickyLimit) || null,
      // Kept for "use the global strategy" too, so it survives a later switch to smart.
      confidenceThreshold: comboForm.strategy === "" || comboForm.strategy === "smart" ? Number(comboForm.threshold) : null,
      weights: comboStrategy === "weighted" ? parseWeights(comboForm.weights) : [],
    };
    void attempt(async () => {
      await api.gatewayComboSave(input);
      comboForm = blankCombo();
    }, `Combo ${input.name} saved.`);
  }

  function removeCombo(combo: Combo) {
    void attempt(async () => {
      await api.gatewayComboDelete(combo.id);
    }, `Combo ${combo.name} removed.`);
  }

  function refreshCatalog() {
    refreshing = true;
    void attempt(async () => {
      const status = await api.gatewayCatalogRefresh();
      message = `Imported ${status.models} models.`;
    }, "Model catalog refreshed.").finally(() => (refreshing = false));
  }

  function clearCache() {
    void attempt(async () => {
      await api.gatewayCacheClear();
    }, "Response cache cleared.");
  }

  function addBudget() {
    settings.budgets = [...settings.budgets, { role: null, period: "daily", limitUsd: 5 }];
  }

  const updatedText = (status: CatalogStatus) =>
    status.updatedAt ? new Date(status.updatedAt * 1000).toLocaleString() : "never";

  // <Select> values are strings; "off" stands for null.
  const levelOptions: Option<"off" | CavemanLevel>[] = [
    { value: "off", label: "Off" },
    { value: "lite", label: "Lite" },
    { value: "full", label: "Full" },
    { value: "ultra", label: "Ultra" },
  ];
  const comboStrategyOptions: Option<"" | ComboStrategy>[] = [{ value: "", label: "Use the global strategy" }, ...STRATEGIES];
  const effortOptions: Option[] = EFFORTS.map((e) => ({ value: e, label: e === "off" ? "Off (provider default)" : e }));
  const thresholdOptions: Option[] = THRESHOLDS.map((t) => ({ value: String(t), label: `${t}${t === DEFAULT_THRESHOLD ? " (default)" : ""}` }));
  const roleOptions: Option[] = [{ value: "", label: "All roles" }, ...ROLES.map((r) => ({ value: r, label: r }))];
  const periodOptions: Option<"daily" | "monthly">[] = [
    { value: "daily", label: "per day" },
    { value: "monthly", label: "per month" },
  ];
  const budgetActionOptions: Option[] = [
    { value: "block", label: "Stop with a message" },
    { value: "cheapest", label: "Use the cheapest model in the combo" },
  ];
  const cap = (word: string) => word[0].toUpperCase() + word.slice(1);

  const nav: SettingsNavItem[] = [
    { id: "conns", label: "Connections", href: "#conns" },
    { id: "roles", label: "Default model and roles", href: "#roles" },
    { id: "combos", label: "Combos", href: "#combos" },
    { id: "savers", label: "Token savers", href: "#savers" },
    { id: "reasoning", label: "Reasoning", href: "#reasoning" },
    { id: "cache", label: "Routing and caching", href: "#cache" },
    { id: "budgets", label: "Budgets", href: "#budgets" },
    { id: "catalog", label: "Model catalog", href: "#catalog" },
  ];
  const ROLE_HELP: Record<string, string> = {
    chat: "Agent tasks and chat (falls back to the default model)",
    design: "Design Studio generation",
    summary: "Summaries and compaction",
    review: "Code review",
  };
</script>

<SettingsLayout
  title="Providers"
  description="Next Agent talks to model providers directly through its built-in gateway. No external router is needed. API keys are saved in the OS keychain and are never shown again."
  {nav}
>
  {#snippet top()}<a href="/settings">← Settings</a> · <a href="/usage">Usage →</a>{/snippet}

  {#if message}<Banner tone={failed ? "danger" : "info"}>{message}</Banner>{/if}

  <SettingsSection id="conns" title="Connections" description="Several connections for the same provider act as accounts: the lowest priority is tried first and the next takes over after auth errors or rate limits.">
    {#if connections.length === 0}
      <p class="note">No providers yet. Add one below. The local Ollama and LM Studio presets need no key.</p>
    {:else}
      <List bordered={false} label="Connections">
        {#each connections as conn (conn.id)}
          {@const result = tests[conn.id]}
          <ListRow
            title={conn.name}
            description="{conn.provider} · key {conn.hasKey ? 'stored' : conn.needsKey ? 'missing' : 'not needed'} · priority {conn.priority}"
            disabled={!conn.isActive}
          >
            <code>{conn.baseUrl}</code>
            {#if !conn.isActive}<Badge size="sm">off</Badge>{/if}
            {#if result}
              <span class="result {result === 'running' ? '' : result.ok ? 'ok' : 'bad'}" role="status">
                {result === "running" ? "Testing…" : result.detail}
                {#if result !== "running" && result.models.length}
                  <span class="muted"> · {result.models.slice(0, 8).join(", ")}{result.models.length > 8 ? "…" : ""}</span>
                {/if}
              </span>
            {/if}
            {#snippet actions()}
              <Button size="sm" variant="ghost" onclick={() => editConnection(conn)} aria-label="Edit {conn.name}">Edit</Button>
              <Button size="sm" variant="ghost" onclick={() => testConnection(conn)} aria-label="Test {conn.name}">Test</Button>
              <Button size="sm" variant="danger" onclick={() => removeConnection(conn)} aria-label="Remove {conn.name}">Remove</Button>
            {/snippet}
          </ListRow>
        {/each}
      </List>
    {/if}
  </SettingsSection>

  <form onsubmit={saveConnection}>
    <SettingsSection title={form.id ? "Edit connection" : "Add a connection"}>
      {#snippet actions()}
        {#if form.id}<Button onclick={() => (form = blankConnection())}>Cancel</Button>{/if}
        <Button type="submit" variant="primary">{form.id ? "Save changes" : "Add connection"}</Button>
      {/snippet}
      <SettingsRow label="Provider">
        {#snippet children(c)}<Select id={c.id} bind:value={form.provider} options={presets.map((p) => ({ value: p.id, label: p.label }))} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Name">
        {#snippet children(c)}<TextField id={c.id} bind:value={form.name} placeholder={formPreset?.label ?? ""} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Base URL">
        {#snippet children(c)}<TextField id={c.id} mono bind:value={form.baseUrl} placeholder={formPreset?.baseUrl ?? ""} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Models">
        {#snippet children(c)}<TextField id={c.id} bind:value={form.models} placeholder={formPreset?.models.join(", ") || "model-a, model-b"} />{/snippet}
      </SettingsRow>
      <SettingsRow label="API key">
        {#snippet children(c)}
          <TextField
            id={c.id} type="password" autocomplete="off" mono bind:value={form.apiKey}
            placeholder={form.id ? "Leave blank to keep the stored key" : formPreset?.needsKey ? "Required" : "Optional"}
          />
        {/snippet}
      </SettingsRow>
      <SettingsRow label="Priority">
        {#snippet children(c)}<TextField id={c.id} type="number" min="0" bind:value={form.priority} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Active">
        {#snippet children(c)}<Switch id={c.id} bind:checked={form.isActive} />{/snippet}
      </SettingsRow>
      {#if form.id}
        <SettingsRow label="Remove the stored key">
          {#snippet children(c)}<Switch id={c.id} bind:checked={form.clearKey} />{/snippet}
        </SettingsRow>
      {/if}
    </SettingsSection>
  </form>

  <datalist id="targets">
    {#each targets as target (target.value)}<option value={target.value}>{target.label}</option>{/each}
  </datalist>

  <form onsubmit={(e) => saveSettings(e, "Models saved.")}>
    <SettingsSection id="roles" title="Default model and roles" description="Write provider/model (for example anthropic/claude-sonnet-4-5) or a combo name. Empty roles use the chat role, then the default model.">
      {#snippet actions()}<Button type="submit" variant="primary">Save models</Button>{/snippet}
      <SettingsRow label="Default model">
        {#snippet children(c)}<TextField id={c.id} list="targets" placeholder="provider/model or combo" bind:value={settings.defaultModel} />{/snippet}
      </SettingsRow>
      {#each ROLES as role (role)}
        <SettingsRow label={cap(role)}>
          {#snippet children(c)}<TextField id={c.id} list="targets" placeholder={ROLE_HELP[role]} bind:value={settings.roles[role]} />{/snippet}
        </SettingsRow>
      {/each}
    </SettingsSection>
  </form>

  {#if combos.length > 0}
    <SettingsSection title="Combos">
      <List bordered={false} label="Combos">
        {#each combos as combo (combo.id)}
          <ListRow
            title={combo.name}
            description={combo.models.join(" → ")}
            meta="{combo.strategy ?? `global (${settings.comboStrategy})`}{effectiveStrategy(combo.strategy, settings.comboStrategy) === 'smart' ? ` · switch below ${combo.confidenceThreshold ?? DEFAULT_THRESHOLD}` : ''}"
          >
            {#snippet actions()}
              <Button size="sm" variant="ghost" onclick={() => editCombo(combo)} aria-label="Edit combo {combo.name}">Edit</Button>
              <Button size="sm" variant="danger" onclick={() => removeCombo(combo)} aria-label="Remove combo {combo.name}">Remove</Button>
            {/snippet}
          </ListRow>
        {/each}
      </List>
    </SettingsSection>
  {/if}

  <form onsubmit={saveCombo}>
    <SettingsSection
      id="combos"
      title={comboForm.id ? "Edit combo" : "Add a combo"}
      description="A combo is a list of models tried in order (fallback), in rotation (round-robin), or kept per task until it fails or its answers get worse (smart)."
    >
      {#snippet actions()}
        {#if comboForm.id}<Button onclick={() => (comboForm = blankCombo())}>Cancel</Button>{/if}
        <Button type="submit" variant="primary">{comboForm.id ? "Save combo" : "Add combo"}</Button>
      {/snippet}
      <SettingsRow label="Name">
        {#snippet children(c)}<TextField id={c.id} bind:value={comboForm.name} placeholder="fast" />{/snippet}
      </SettingsRow>
      <SettingsRow label="Models (one per line)" stacked>
        {#snippet children(c)}<TextArea id={c.id} mono rows={3} bind:value={comboForm.models} placeholder={"groq/llama-3.3-70b-versatile\nopenai/gpt-4o-mini"} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Strategy">
        {#snippet children(c)}<Select id={c.id} bind:value={comboForm.strategy} options={comboStrategyOptions} />{/snippet}
      </SettingsRow>
      {#if comboStrategy === "weighted"}
        <SettingsRow label="Weights (one per model)">
          {#snippet children(c)}<TextField id={c.id} bind:value={comboForm.weights} placeholder="3, 1 (empty = equal)" />{/snippet}
        </SettingsRow>
      {/if}
      {#if comboForm.strategy === "round-robin"}
        <SettingsRow label="Uses before rotating">
          {#snippet children(c)}<TextField id={c.id} type="number" min="1" bind:value={comboForm.stickyLimit} />{/snippet}
        </SettingsRow>
      {/if}
      {#if comboIsSmart}
        <SettingsRow
          label="Switch when quality drops below"
          description="Quality is the share of good answers over the last 10. Empty, refused, truncated answers and broken tool-call JSON count as bad. Higher switches sooner."
        >
          {#snippet children(c)}
            <Select
              id={c.id} aria-describedby={c.describedby} options={thresholdOptions}
              bind:value={() => String(comboForm.threshold), (v) => (comboForm.threshold = Number(v) as typeof comboForm.threshold)}
            />
          {/snippet}
        </SettingsRow>
      {/if}
    </SettingsSection>
  </form>

  <form onsubmit={(e) => saveSettings(e, "Token savers saved.")}>
    <SettingsSection id="savers" title="Token savers" description="Savers fail open: if one errors, the request is sent unchanged.">
      {#snippet actions()}<Button type="submit" variant="primary">Save token savers</Button>{/snippet}
      <SettingsRow label="Enable token savers">
        {#snippet children(c)}<Switch id={c.id} bind:checked={settings.tokenSaver} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Caveman (terse answers)" disabled={!settings.tokenSaver}>
        {#snippet children(c)}
          <Select id={c.id} options={levelOptions} disabled={!settings.tokenSaver}
            bind:value={() => settings.caveman ?? "off", (v) => (settings.caveman = v === "off" ? null : v)} />
        {/snippet}
      </SettingsRow>
      <SettingsRow label="Ponytail (minimal code)" disabled={!settings.tokenSaver}>
        {#snippet children(c)}
          <Select id={c.id} options={levelOptions} disabled={!settings.tokenSaver}
            bind:value={() => settings.ponytail ?? "off", (v) => (settings.ponytail = v === "off" ? null : v)} />
        {/snippet}
      </SettingsRow>
      <SettingsRow label="Compress tool output (RTK)" disabled={!settings.tokenSaver}>
        {#snippet children(c)}<Switch id={c.id} bind:checked={settings.rtk} disabled={!settings.tokenSaver} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Anthropic prompt caching" disabled={!settings.tokenSaver}>
        {#snippet children(c)}<Switch id={c.id} bind:checked={settings.cacheControl} disabled={!settings.tokenSaver} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Global combo strategy">
        {#snippet children(c)}<Select id={c.id} bind:value={settings.comboStrategy} options={STRATEGIES} />{/snippet}
      </SettingsRow>
    </SettingsSection>
  </form>

  <form onsubmit={(e) => saveSettings(e, "Reasoning saved.")}>
    <SettingsSection id="reasoning" title="Reasoning" description="One setting per role, mapped per provider: OpenAI reasoning_effort, Anthropic thinking budget, Gemini thinking level or budget. A token budget wins over the effort.">
      {#snippet actions()}<Button type="submit" variant="primary">Save reasoning</Button>{/snippet}
      {#each ROLES as role (role)}
        <SettingsRow label={cap(role)}>
          {#snippet children(c)}
            <div class="compact">
              <Select id={c.id} bind:value={settings.reasoning[role].effort} options={effortOptions} />
              <TextField aria-label="Token budget for {role}" type="number" min="0" step="1024" placeholder="or tokens" bind:value={settings.reasoning[role].maxTokens} />
            </div>
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsSection>
  </form>

  <form onsubmit={(e) => saveSettings(e, "Caching saved.")}>
    <SettingsSection id="cache" title="Routing and caching" description="Only byte-identical requests reuse an answer. Off by default.">
      {#snippet actions()}
        <Button onclick={clearCache} disabled={cacheEntries === 0}>Clear cache ({cacheEntries})</Button>
        <Button type="submit" variant="primary">Save caching</Button>
      {/snippet}
      <SettingsRow label="Prompt-cache affinity" description="Keep a conversation on one account.">
        {#snippet children(c)}<Switch id={c.id} aria-describedby={c.describedby} bind:checked={settings.cacheAffinity} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Response cache" description="Reuse identical answers.">
        {#snippet children(c)}<Switch id={c.id} aria-describedby={c.describedby} bind:checked={settings.responseCache.enabled} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Also for requests with a temperature above 0" disabled={!settings.responseCache.enabled}>
        {#snippet children(c)}<Switch id={c.id} bind:checked={settings.responseCache.anyTemperature} disabled={!settings.responseCache.enabled} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Keep answers for (minutes)" disabled={!settings.responseCache.enabled}>
        {#snippet children(c)}
          <TextField id={c.id} type="number" min="1" disabled={!settings.responseCache.enabled}
            value={Math.round(settings.responseCache.ttlSecs / 60)}
            oninput={(e) => (settings.responseCache.ttlSecs = Math.max(1, Number(e.currentTarget.value)) * 60)} />
        {/snippet}
      </SettingsRow>
      <SettingsRow label="Most answers kept" disabled={!settings.responseCache.enabled}>
        {#snippet children(c)}<TextField id={c.id} type="number" min="1" bind:value={settings.responseCache.maxEntries} disabled={!settings.responseCache.enabled} />{/snippet}
      </SettingsRow>
    </SettingsSection>
  </form>

  <form onsubmit={(e) => saveSettings(e, "Budgets saved.")}>
    <SettingsSection id="budgets" title="Budgets" description="Uses the estimated costs on the Usage page, plus the estimated cost of requests still running. You get a warning at 80%. If spend cannot be read, requests are stopped.">
      {#snippet actions()}
        <Button onclick={addBudget}>Add budget</Button>
        <Button type="submit" variant="primary">Save budgets</Button>
      {/snippet}
      {#each settings.budgets as budget, i (i)}
        <SettingsRow label="Budget {i + 1}">
          {#snippet children(c)}
            <div class="compact">
              <Select aria-label="Role for budget {i + 1}" options={roleOptions}
                bind:value={() => budget.role ?? "", (v) => (budget.role = v === "" ? null : (v as (typeof ROLES)[number]))} />
              <Select aria-label="Period for budget {i + 1}" bind:value={budget.period} options={periodOptions} />
              <TextField id={c.id} aria-label="Limit in dollars for budget {i + 1}" type="number" min="0" step="0.5" bind:value={budget.limitUsd}>
                {#snippet prefix()}$&nbsp;{/snippet}
              </TextField>
            </div>
            <Button variant="danger" onclick={() => (settings.budgets = settings.budgets.filter((_, j) => j !== i))}>Remove</Button>
          {/snippet}
        </SettingsRow>
      {/each}
      <SettingsRow label="When a budget is reached">
        {#snippet children(c)}<Select id={c.id} bind:value={settings.budgetAction} options={budgetActionOptions} />{/snippet}
      </SettingsRow>
      <SettingsRow label="Refuse models without a known price while a budget is set">
        {#snippet children(c)}<Switch id={c.id} bind:checked={settings.budgetRefuseUnpriced} />{/snippet}
      </SettingsRow>
    </SettingsSection>
  </form>

  <SettingsSection id="catalog" title="Model catalog" description="Downloads OpenRouter's public model list once (no API key is sent) to fill in prices, context windows and capabilities. It is kept only on this computer. Your own pricing.json wins over it.">
    <SettingsRow label="{catalog.models} models imported" description="Last refresh: {updatedText(catalog)}">
      {#snippet children()}
        <Button variant="primary" onclick={refreshCatalog} loading={refreshing}>{refreshing ? "Refreshing…" : "Refresh from OpenRouter"}</Button>
      {/snippet}
    </SettingsRow>
  </SettingsSection>
</SettingsLayout>

<style>
  .note { margin: 0; padding: var(--space-3) var(--space-4); color: var(--color-text-secondary); font-size: var(--font-size-sm); }
  .compact { --ds-control-width: 180px; display: flex; flex-wrap: wrap; gap: var(--space-2); justify-content: flex-end; }
  .muted { color: var(--color-text-tertiary); }
  code { font-family: var(--font-mono); font-size: var(--font-size-xs); word-break: break-all; }
  .result { font-size: var(--font-size-sm); color: var(--color-text-secondary); }
  .result.ok { color: var(--color-ok-text); }
  .result.bad { color: var(--color-danger-text); }
</style>
