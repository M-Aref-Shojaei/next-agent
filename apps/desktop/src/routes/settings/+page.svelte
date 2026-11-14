<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AuditStatus, type OverlayInfo, type OverlaySettings, type Rule } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";

  let routerUrl = $state("");
  let vaultPath = $state("");
  let rules = $state<Rule[]>([]);
  let audit = $state<AuditStatus | null>(null);
  let message = $state("");
  let overlay = $state<OverlayInfo | null>(null);

  async function load() {
    routerUrl = (await api.settingGet("router.url")) ?? "";
    vaultPath = (await api.vaultGet()) ?? "";
    rules = app.activeWorkspaceId ? await api.rulesList(app.activeWorkspaceId) : [];
    audit = await api.auditVerify();
    overlay = await api.overlayInfo().catch(() => null);
  }
  async function saveOverlay(patch: Partial<OverlaySettings>) {
    if (!overlay) return;
    const next = { ...overlay.settings, ...patch };
    try {
      await api.overlaySetSettings(next);
      overlay = { ...overlay, settings: next };
      message = "Overlay settings saved.";
    } catch (e) {
      message = String(e);
    }
  }
  onMount(() => void load().catch((e) => (message = String(e))));

  async function saveRouter(event: SubmitEvent) {
    event.preventDefault();
    try {
      await api.settingSet("router.url", routerUrl);
      message = "Router URL saved.";
    } catch (e) {
      message = String(e);
    }
  }
  async function saveVault(event: SubmitEvent) {
    event.preventDefault();
    try {
      vaultPath = await api.vaultSet(vaultPath);
      message = "Vault path saved.";
    } catch (e) {
      message = String(e);
    }
  }
  async function revoke(id: string) {
    await api.ruleDelete(id);
    rules = rules.filter((r) => r.id !== id);
  }
</script>

<div class="settings">
  <h1>Settings</h1>
  <p><a href="/">← Back to chat</a></p>
  {#if message}<p role="status">{message}</p>{/if}

  <section aria-labelledby="h-router">
    <h2 id="h-router">Models and providers</h2>
    <p>Providers, API keys, the default model, combos, token savers and model roles are on the
      <a href="/settings/providers">Providers</a> page. See token and cost reports on <a href="/usage">Usage</a>.</p>
    <h3>External router (optional)</h3>
    <form onsubmit={saveRouter}>
      <label for="router">Base URL</label>
      <input id="router" bind:value={routerUrl} />
      <button type="submit">Save</button>
    </form>
    <p class="muted">Only needed for an external OpenAI-compatible router such as 9Router; the built-in gateway works without it.</p>
  </section>

  <section aria-labelledby="h-channels">
    <h2 id="h-channels">Channels</h2>
    <p>Message the agent from Telegram. Bots, pairings, allow-lists and health are on the
      <a href="/settings/channels">Channels</a> page.</p>
  </section>

  {#if overlay}
    <section aria-labelledby="h-overlay">
      <h2 id="h-overlay">Overlay</h2>
      <p class="muted">An island around the notch (or at the top of the screen) showing running tasks and pending approvals. Changes apply at once. Behaviour ported from Coucou (MIT).</p>
      <label>
        <input
          type="checkbox"
          checked={overlay.settings.enabled}
          onchange={(e) => saveOverlay({ enabled: e.currentTarget.checked })}
        />
        Show the overlay
      </label>
      <label for="ov-pos">Position</label>
      <select
        id="ov-pos"
        value={overlay.settings.position}
        onchange={(e) => saveOverlay({ position: e.currentTarget.value as OverlaySettings["position"] })}
      >
        <option value="top-center">Top centre</option>
        <option value="top-left">Top left corner</option>
        <option value="top-right">Top right corner</option>
      </select>
      <label for="ov-mon">Monitor</label>
      <select
        id="ov-mon"
        value={overlay.settings.monitor ?? ""}
        onchange={(e) => saveOverlay({ monitor: e.currentTarget.value || null })}
      >
        <option value="">The screen with a notch (else the main one)</option>
        <option value="menuBar">The screen with the menu bar</option>
        <option value="followMouse">Follow the mouse</option>
        {#each overlay.monitors as m (m.key)}<option value={m.key}>{m.label}</option>{/each}
      </select>
    </section>
  {/if}

  <section aria-labelledby="h-vault">
    <h2 id="h-vault">Vault</h2>
    <form onsubmit={saveVault}>
      <label for="vault">Vault folder</label>
      <input id="vault" bind:value={vaultPath} placeholder="/path/to/vault" />
      <button type="submit">Save</button>
    </form>
  </section>

  <section aria-labelledby="h-rules">
    <h2 id="h-rules">Permission rules</h2>
    {#if rules.length === 0}
      <p class="muted">No rules. "Always for this workspace" approvals appear here.</p>
    {:else}
      <table>
        <thead><tr><th>Scope</th><th>Effect</th><th>Tool</th><th>Pattern</th><th></th></tr></thead>
        <tbody>
          {#each rules as rule (rule.id)}
            <tr>
              <td>{rule.scope}</td><td>{rule.effect}</td><td>{rule.kind}</td><td><code>{rule.pattern}</code></td>
              <td><button aria-label="Revoke rule {rule.id}" onclick={() => revoke(rule.id)}>Revoke</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>

  <section aria-labelledby="h-audit">
    <h2 id="h-audit">Audit log (read-only)</h2>
    {#if audit}
      <p role="status">
        Hash chain {audit.ok ? "verified" : `BROKEN at #${audit.broken_at}`} · {audit.checked} entries checked.
      </p>
    {/if}
    <p class="muted">Entry listing is stubbed: core exposes verification but no audit query yet.</p>
  </section>
</div>

<style>
  .settings { padding: 20px 28px; overflow: auto; max-width: 760px; display: grid; gap: 14px; align-content: start; }
  section { border: 1px solid var(--border); border-radius: 8px; padding: 12px 14px; background: var(--panel); }
  form { display: grid; grid-template-columns: auto 1fr auto; gap: 8px; align-items: center; margin-top: 8px; }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid var(--border); }
</style>
