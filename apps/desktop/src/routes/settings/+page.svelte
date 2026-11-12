<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AuditStatus, type Rule } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";

  let routerUrl = $state("");
  let vaultPath = $state("");
  let rules = $state<Rule[]>([]);
  let audit = $state<AuditStatus | null>(null);
  let message = $state("");

  async function load() {
    routerUrl = (await api.settingGet("router.url")) ?? "";
    vaultPath = (await api.vaultGet()) ?? "";
    rules = app.activeWorkspaceId ? await api.rulesList(app.activeWorkspaceId) : [];
    audit = await api.auditVerify();
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
    <h2 id="h-router">Router</h2>
    <form onsubmit={saveRouter}>
      <label for="router">Base URL</label>
      <input id="router" bind:value={routerUrl} />
      <button type="submit">Save</button>
    </form>
    <p class="muted">API key (OS keychain) and model selection arrive with Step 6.</p>
  </section>

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
