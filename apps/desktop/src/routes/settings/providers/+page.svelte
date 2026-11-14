<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import {
    ROLES,
    defaultSettings,
    parseList,
    type CavemanLevel,
    type Combo,
    type ComboStrategy,
    DEFAULT_THRESHOLD,
    effectiveStrategy,
    THRESHOLDS,
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
  let message = $state("");

  // Connection form. The key field is write-only: it is never filled from the backend.
  const blankConnection = () => ({
    id: "", provider: "openai", name: "", baseUrl: "", models: "", priority: 100, isActive: true, apiKey: "", clearKey: false,
  });
  let form = $state(blankConnection());
  const formPreset = $derived(presets.find((p) => p.id === form.provider));

  // Combo form.
  const blankCombo = () => ({ id: "", name: "", models: "", strategy: "" as "" | ComboStrategy, stickyLimit: 1, threshold: DEFAULT_THRESHOLD });
  let comboForm = $state(blankCombo());
  const comboIsSmart = $derived(effectiveStrategy(comboForm.strategy, settings.comboStrategy) === "smart");

  async function load() {
    [presets, connections, combos, targets, settings] = await Promise.all([
      api.gatewayPresets(),
      api.gatewayConnections(),
      api.gatewayCombos(),
      api.gatewayTargets(),
      api.gatewaySettingsGet(),
    ]);
  }
  onMount(() => void load().catch((e) => (message = String(e))));

  async function attempt(work: () => Promise<void>, done: string) {
    try {
      await work();
      message = done;
      await load();
    } catch (e) {
      message = String(e);
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
      tests = { ...tests, [conn.id]: { ok: false, detail: String(e), models: [] } };
    }
  }

  function saveSettings(event: SubmitEvent, done: string) {
    event.preventDefault();
    const clean: GatewaySettings = {
      ...settings,
      defaultModel: settings.defaultModel?.trim() || null,
      roles: Object.fromEntries(Object.entries(settings.roles).filter(([, v]) => v && v.trim())),
    };
    void attempt(() => api.gatewaySettingsSet(clean), done);
  }

  function editCombo(combo: Combo) {
    comboForm = {
      id: combo.id, name: combo.name, models: combo.models.join("\n"),
      strategy: combo.strategy ?? "", stickyLimit: combo.stickyLimit ?? 1,
      threshold: combo.confidenceThreshold ?? DEFAULT_THRESHOLD,
    };
  }

  function saveCombo(event: SubmitEvent) {
    event.preventDefault();
    const input: Combo = {
      id: comboForm.id, name: comboForm.name.trim(), models: parseList(comboForm.models),
      strategy: comboForm.strategy || null, stickyLimit: Number(comboForm.stickyLimit) || null,
      // Kept for "use the global strategy" too, so it survives a later switch to smart.
      confidenceThreshold: comboForm.strategy === "" || comboForm.strategy === "smart" ? Number(comboForm.threshold) : null,
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

  const levelOptions: Array<{ value: CavemanLevel | null; label: string }> = [
    { value: null, label: "Off" },
    { value: "lite", label: "Lite" },
    { value: "full", label: "Full" },
    { value: "ultra", label: "Ultra" },
  ];
  const ROLE_HELP: Record<string, string> = {
    chat: "Agent tasks and chat (falls back to the default model)",
    design: "Design Studio generation",
    summary: "Summaries and compaction",
    review: "Code review",
  };
</script>

<div class="page">
  <h1>Providers</h1>
  <p><a href="/settings">← Settings</a> · <a href="/usage">Usage →</a></p>
  <p class="muted">
    Next Agent talks to model providers directly through its built-in gateway. No external router is needed.
    API keys are saved in the OS keychain and are never shown again.
  </p>
  {#if message}<p role="status">{message}</p>{/if}

  <section aria-labelledby="h-conns">
    <h2 id="h-conns">Connections</h2>
    {#if connections.length === 0}
      <p class="muted">No providers yet. Add one below. The local Ollama and LM Studio presets need no key.</p>
    {:else}
      <table>
        <thead><tr><th>Name</th><th>Provider</th><th>Endpoint</th><th>Key</th><th>Priority</th><th></th></tr></thead>
        <tbody>
          {#each connections as conn (conn.id)}
            {@const result = tests[conn.id]}
            <tr class:inactive={!conn.isActive}>
              <td>{conn.name}{#if !conn.isActive} <span class="muted">(off)</span>{/if}</td>
              <td>{conn.provider}</td>
              <td><code>{conn.baseUrl}</code></td>
              <td>{conn.hasKey ? "stored" : conn.needsKey ? "missing" : "not needed"}</td>
              <td>{conn.priority}</td>
              <td class="actions">
                <button type="button" onclick={() => editConnection(conn)} aria-label="Edit {conn.name}">Edit</button>
                <button type="button" onclick={() => testConnection(conn)} aria-label="Test {conn.name}">Test</button>
                <button type="button" class="danger" onclick={() => removeConnection(conn)} aria-label="Remove {conn.name}">Remove</button>
              </td>
            </tr>
            {#if result}
              <tr><td colspan="6" class={result === "running" ? "muted" : result.ok ? "ok" : "bad"}>
                <span role="status">{result === "running" ? "Testing…" : result.detail}</span>
                {#if result !== "running" && result.models.length}
                  <span class="muted"> · {result.models.slice(0, 8).join(", ")}{result.models.length > 8 ? "…" : ""}</span>
                {/if}
              </td></tr>
            {/if}
          {/each}
        </tbody>
      </table>
    {/if}

    <h3>{form.id ? "Edit connection" : "Add a connection"}</h3>
    <form class="grid" onsubmit={saveConnection}>
      <label for="c-provider">Provider</label>
      <select id="c-provider" bind:value={form.provider}>
        {#each presets as preset (preset.id)}<option value={preset.id}>{preset.label}</option>{/each}
      </select>
      <label for="c-name">Name</label>
      <input id="c-name" bind:value={form.name} placeholder={formPreset?.label ?? ""} />
      <label for="c-url">Base URL</label>
      <input id="c-url" bind:value={form.baseUrl} placeholder={formPreset?.baseUrl ?? ""} />
      <label for="c-models">Models</label>
      <input id="c-models" bind:value={form.models} placeholder={formPreset?.models.join(", ") || "model-a, model-b"} />
      <label for="c-key">API key</label>
      <input
        id="c-key" type="password" autocomplete="off" bind:value={form.apiKey}
        placeholder={form.id ? "Leave blank to keep the stored key" : formPreset?.needsKey ? "Required" : "Optional"}
      />
      <label for="c-priority">Priority</label>
      <input id="c-priority" type="number" min="0" bind:value={form.priority} />
      <span></span>
      <div class="checks">
        <label><input type="checkbox" bind:checked={form.isActive} /> Active</label>
        {#if form.id}<label><input type="checkbox" bind:checked={form.clearKey} /> Remove the stored key</label>{/if}
      </div>
      <span></span>
      <div class="buttons">
        <button type="submit" class="primary">{form.id ? "Save changes" : "Add connection"}</button>
        {#if form.id}<button type="button" onclick={() => (form = blankConnection())}>Cancel</button>{/if}
      </div>
    </form>
    <p class="muted">
      Several connections for the same provider act as accounts: the lowest priority is tried first and the
      next takes over after auth errors or rate limits.
    </p>
  </section>

  <datalist id="targets">
    {#each targets as target (target.value)}<option value={target.value}>{target.label}</option>{/each}
  </datalist>

  <section aria-labelledby="h-roles">
    <h2 id="h-roles">Default model and roles</h2>
    <form class="grid" onsubmit={(e) => saveSettings(e, "Models saved.")}>
      <label for="r-default">Default model</label>
      <input id="r-default" list="targets" placeholder="provider/model or combo" bind:value={settings.defaultModel} />
      {#each ROLES as role (role)}
        <label for="r-{role}">{role}</label>
        <input id="r-{role}" list="targets" placeholder={ROLE_HELP[role]} bind:value={settings.roles[role]} />
      {/each}
      <span></span>
      <div class="buttons"><button type="submit" class="primary">Save models</button></div>
    </form>
    <p class="muted">Write <code>provider/model</code> (for example <code>anthropic/claude-sonnet-4-5</code>) or a combo name. Empty roles use the chat role, then the default model.</p>
  </section>

  <section aria-labelledby="h-combos">
    <h2 id="h-combos">Combos</h2>
    {#if combos.length === 0}
      <p class="muted">A combo is a list of models tried in order (fallback), in rotation (round-robin), or kept per task until it fails or its answers get worse (smart).</p>
    {:else}
      <table>
        <thead><tr><th>Name</th><th>Models</th><th>Strategy</th><th></th></tr></thead>
        <tbody>
          {#each combos as combo (combo.id)}
            <tr>
              <td>{combo.name}</td>
              <td>{combo.models.join(" → ")}</td>
              <td>{combo.strategy ?? `global (${settings.comboStrategy})`}{#if effectiveStrategy(combo.strategy, settings.comboStrategy) === "smart"}<span class="muted">&nbsp;(switch below {combo.confidenceThreshold ?? DEFAULT_THRESHOLD})</span>{/if}</td>
              <td class="actions">
                <button type="button" onclick={() => editCombo(combo)} aria-label="Edit combo {combo.name}">Edit</button>
                <button type="button" class="danger" onclick={() => removeCombo(combo)} aria-label="Remove combo {combo.name}">Remove</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <form class="grid" onsubmit={saveCombo}>
      <label for="k-name">Name</label>
      <input id="k-name" bind:value={comboForm.name} placeholder="fast" />
      <label for="k-models">Models (one per line)</label>
      <textarea id="k-models" rows="3" bind:value={comboForm.models} placeholder={"groq/llama-3.3-70b-versatile\nopenai/gpt-4o-mini"}></textarea>
      <label for="k-strategy">Strategy</label>
      <select id="k-strategy" bind:value={comboForm.strategy}>
        <option value="">Use the global strategy</option>
        <option value="fallback">Fallback (in order)</option>
        <option value="round-robin">Round-robin</option>
        <option value="smart">Smart (one model per task)</option>
      </select>
      {#if comboForm.strategy === "round-robin"}
        <label for="k-sticky">Uses before rotating</label>
        <input id="k-sticky" type="number" min="1" bind:value={comboForm.stickyLimit} />
      {/if}
      {#if comboIsSmart}
        <label for="k-threshold">Switch when quality drops below</label>
        <div>
          <select id="k-threshold" bind:value={comboForm.threshold} aria-describedby="k-threshold-help">
            {#each THRESHOLDS as t (t)}<option value={t}>{t}{t === DEFAULT_THRESHOLD ? " (default)" : ""}</option>{/each}
          </select>
          <p id="k-threshold-help" class="muted">Quality is the share of good answers over the last 10. Empty, refused, truncated answers and broken tool-call JSON count as bad. Higher switches sooner.</p>
        </div>
      {/if}
      <span></span>
      <div class="buttons">
        <button type="submit" class="primary">{comboForm.id ? "Save combo" : "Add combo"}</button>
        {#if comboForm.id}<button type="button" onclick={() => (comboForm = blankCombo())}>Cancel</button>{/if}
      </div>
    </form>
  </section>

  <section aria-labelledby="h-savers">
    <h2 id="h-savers">Token savers</h2>
    <form class="grid" onsubmit={(e) => saveSettings(e, "Token savers saved.")}>
      <span></span>
      <label><input type="checkbox" bind:checked={settings.tokenSaver} /> Enable token savers</label>
      <label for="s-caveman">Caveman (terse answers)</label>
      <select id="s-caveman" bind:value={settings.caveman} disabled={!settings.tokenSaver}>
        {#each levelOptions as option (option.label)}<option value={option.value}>{option.label}</option>{/each}
      </select>
      <label for="s-ponytail">Ponytail (minimal code)</label>
      <select id="s-ponytail" bind:value={settings.ponytail} disabled={!settings.tokenSaver}>
        {#each levelOptions as option (option.label)}<option value={option.value}>{option.label}</option>{/each}
      </select>
      <span></span>
      <label><input type="checkbox" bind:checked={settings.rtk} disabled={!settings.tokenSaver} /> Compress tool output (RTK)</label>
      <span></span>
      <label><input type="checkbox" bind:checked={settings.cacheControl} disabled={!settings.tokenSaver} /> Anthropic prompt caching</label>
      <label for="s-strategy">Global combo strategy</label>
      <select id="s-strategy" bind:value={settings.comboStrategy}>
        <option value="fallback">Fallback</option>
        <option value="round-robin">Round-robin</option>
        <option value="smart">Smart</option>
      </select>
      <span></span>
      <div class="buttons"><button type="submit" class="primary">Save token savers</button></div>
    </form>
    <p class="muted">Savers fail open: if one errors, the request is sent unchanged.</p>
  </section>
</div>

<style>
  .page { padding: 20px 28px; overflow: auto; max-width: 900px; display: grid; gap: 14px; align-content: start; }
  section { border: 1px solid var(--border); border-radius: 8px; padding: 12px 14px; background: var(--panel); }
  h3 { font-size: 14px; margin: 14px 0 4px; }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid var(--border); vertical-align: top; }
  td code { font-size: 12px; word-break: break-all; }
  tr.inactive td { opacity: 0.6; }
  .actions { white-space: nowrap; display: flex; gap: 4px; }
  .grid { display: grid; grid-template-columns: minmax(140px, auto) 1fr; gap: 8px; align-items: center; margin-top: 8px; }
  .checks, .buttons { display: flex; gap: 12px; align-items: center; }
  textarea { font: inherit; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 6px; padding: 6px 8px; }
  select { font: inherit; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 6px; padding: 5px 6px; }
  .ok { color: var(--ok); } .bad { color: var(--danger); }
</style>
