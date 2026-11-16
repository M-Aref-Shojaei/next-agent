<script lang="ts">
  import { log } from "$lib/log.ts";
  import { onMount } from "svelte";
  import { api, type AuditStatus, type Rule } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
  import { upstreams } from "$lib/upstreams.svelte.ts";
  import {
    Badge, Banner, Button, EmptyState, List, ListRow, SettingsLayout, SettingsRow, SettingsSection, StatusDot, TextField,
    type SettingsNavItem,
  } from "$lib/ds/index.ts";
  import { OVERLAY_ROOT, OVERLAY_SECTIONS } from "$lib/overlay-settings.ts";

  let routerUrl = $state("");
  let vaultPath = $state("");
  let rules = $state<Rule[]>([]);
  let audit = $state<AuditStatus | null>(null);
  let message = $state("");

  const nav: SettingsNavItem[] = $derived([
    { id: "router", label: "Models and providers", href: "#router" },
    { id: "channels", label: "Channels", href: "#channels" },
    { id: "overlay", label: "Overlay", href: "#overlay" },
    { id: "upstreams", label: "Upstreams", href: "#upstreams", badge: upstreams.unseen > 0 ? "new" : undefined },
    { id: "design", label: "Design system", href: "#design" },
    { id: "diagnostics", label: "Diagnostics", href: "#diagnostics" },
    { id: "vault", label: "Vault", href: "#vault" },
    { id: "rules", label: "Permission rules", href: "#rules" },
    { id: "audit", label: "Audit log", href: "#audit" },
  ]);

  async function load() {
    routerUrl = (await api.settingGet("router.url")) ?? "";
    vaultPath = (await api.vaultGet()) ?? "";
    rules = app.activeWorkspaceId ? await api.rulesList(app.activeWorkspaceId) : [];
    audit = await api.auditVerify();
  }
  onMount(() => void load().catch((e) => {
    log.warn("ui_error", { kind: "settings", error: String(e) });
    message = String(e);
  }));

  async function saveRouter(event: SubmitEvent) {
    event.preventDefault();
    try {
      await api.settingSet("router.url", routerUrl);
      message = "Router URL saved.";
    } catch (e) {
      log.warn("ui_error", { kind: "settings", error: String(e) });
      message = String(e);
    }
  }
  async function saveVault(event: SubmitEvent) {
    event.preventDefault();
    try {
      vaultPath = await api.vaultSet(vaultPath);
      message = "Vault path saved.";
    } catch (e) {
      log.warn("ui_error", { kind: "vault" });
      message = String(e);
    }
  }
  async function revoke(id: string) {
    try {
      await api.ruleDelete(id);
      rules = rules.filter((r) => r.id !== id);
    } catch (e) {
      log.warn("ui_error", { kind: "settings", error: String(e) });
      message = String(e);
    }
  }
</script>

<SettingsLayout title="Settings" {nav}>
  {#snippet top()}<a href="/">← Back to chat</a>{/snippet}
  {#if message}<Banner tone="info" ondismiss={() => (message = "")}>{message}</Banner>{/if}

  <SettingsSection
    id="router"
    title="Models and providers"
    description="Providers, API keys, the default model, combos, token savers and model roles are on the Providers page. See token and cost reports on Usage."
  >
    {#snippet actions()}
      <a class="go" href="/settings/providers">Providers →</a>
      <a class="go" href="/usage">Usage →</a>
    {/snippet}
    <form onsubmit={saveRouter}>
      <SettingsRow
        label="External router (optional)"
        description="Only needed for an external OpenAI-compatible router such as 9Router; the built-in gateway works without it."
      >
        {#snippet children(ctl)}
          <TextField id={ctl.id} aria-describedby={ctl.describedby} mono placeholder="Base URL" bind:value={routerUrl} />
          <Button type="submit">Save</Button>
        {/snippet}
      </SettingsRow>
    </form>
  </SettingsSection>

  <SettingsSection id="channels" title="Channels" description="Message the agent from Telegram. Bots, pairings, allow-lists and health are on the Channels page.">
    {#snippet actions()}<a class="go" href="/settings/channels">Channels →</a>{/snippet}
  </SettingsSection>

  <SettingsSection
    id="overlay"
    title="Overlay"
    description="The island at the top of the screen (around the notch on a MacBook) is built in (vendored MIT code, credited in NOTICE.md; ADR-0011). It shows Next Agent's tasks and their approvals, which you can answer there; Next Agent's permission engine still decides."
  >
    <List label="Overlay settings" bordered={false}>
      {#each OVERLAY_SECTIONS as section (section.id)}
        <ListRow title={section.label} href="{OVERLAY_ROOT}/{section.id}" />
      {/each}
    </List>
    {#snippet footer()}
      Claude Code sessions reach the island only once you install the overlay's hooks under <a href="{OVERLAY_ROOT}/agents">Agents</a>:
      it shows the exact change to <code>~/.claude/settings.json</code>, saves a dated backup, and writes nothing until you confirm. Off until then.
    {/snippet}
  </SettingsSection>

  <SettingsSection
    id="upstreams"
    title="Upstreams"
    description="See what the projects we vendor or port from published since their pinned commit. Nothing is applied automatically."
  >
    {#snippet actions()}
      {#if upstreams.unseen > 0}<Badge tone="accent">new</Badge>{/if}
      <a class="go" href="/settings/upstreams">Upstreams →</a>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="design" title="Design system" description="Every token and component the app's screens are built from, in light and dark.">
    {#snippet actions()}<a class="go" href="/settings/design-system">Design system →</a>{/snippet}
  </SettingsSection>

  <SettingsSection id="diagnostics" title="Diagnostics" description="A local log of how the app behaves, never of what you work on. Set the level, browse recent entries and export a copy for a bug report.">
    {#snippet actions()}<a class="go" href="/settings/diagnostics">Diagnostics →</a>{/snippet}
  </SettingsSection>

  <SettingsSection id="vault" title="Vault">
    <form onsubmit={saveVault}>
      <SettingsRow label="Vault folder">
        {#snippet children(ctl)}
          <TextField id={ctl.id} mono placeholder="/path/to/vault" bind:value={vaultPath} />
          <Button type="submit">Save</Button>
        {/snippet}
      </SettingsRow>
    </form>
  </SettingsSection>

  <SettingsSection id="rules" title="Permission rules">
    {#if rules.length === 0}
      <EmptyState compact title="No rules" description={'"Always for this workspace" approvals appear here.'} />
    {:else}
      <List label="Permission rules" bordered={false}>
        {#each rules as rule (rule.id)}
          <ListRow title="{rule.effect} · {rule.kind}" description={rule.pattern} meta={rule.scope}>
            {#snippet actions()}
              <Button size="sm" variant="danger" aria-label="Revoke rule {rule.id}" onclick={() => revoke(rule.id)}>Revoke</Button>
            {/snippet}
          </ListRow>
        {/each}
      </List>
    {/if}
  </SettingsSection>

  <SettingsSection id="audit" title="Audit log (read-only)">
    {#if audit}
      <SettingsRow label="Hash chain" description="{audit.checked} entries checked.">
        {#snippet children()}
          <StatusDot tone={audit!.ok ? "ok" : "danger"} text={audit!.ok ? "Verified" : `Broken at #${audit!.broken_at}`} />
        {/snippet}
      </SettingsRow>
    {/if}
    {#snippet footer()}Entry listing is stubbed: core exposes verification but no audit query yet.{/snippet}
  </SettingsSection>
</SettingsLayout>

<style>
  code { font-family: var(--font-mono); font-size: var(--font-size-xs); }
  .go { font-size: var(--font-size-sm); font-weight: var(--font-weight-medium); color: var(--color-accent-text); text-decoration: none; white-space: nowrap; }
  .go:hover { text-decoration: underline; }
</style>
