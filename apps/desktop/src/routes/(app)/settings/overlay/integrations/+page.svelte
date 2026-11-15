<script lang="ts">
  // Settings → Overlay → Integrations: the service pills and their keys
  // (settings/main.ts integrationsSection). A service is declared here, next
  // to its key; keys go to the system keychain, never to disk.
  import { onMount } from "svelte";
  import { Button, SettingsRow, SettingsSection, StatusDot, Switch, TextField } from "$lib/ds/index.ts";
  import PillColor from "$lib/components/overlay/PillColor.svelte";
  import { keyStoreName, overlay } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import { MAX_DECLARED, availablePills } from "$coucou/core/pills";

  interface IntegrationDef {
    id: string;
    name: string;
    /** Keychain entries, in the order they are shown. */
    fields: { key: string; label: string; placeholder: string; secret: boolean }[];
    /** What the key needs. */
    hint?: string;
  }

  const INTEGRATIONS: IntegrationDef[] = [
    { id: "integration_stripe", name: "Stripe",
      fields: [{ key: "stripe-api-key", label: "Secret key", placeholder: "sk_live_…", secret: true }] },
    { id: "integration_github", name: "GitHub",
      fields: [{ key: "github-token", label: "Token", placeholder: "ghp_…", secret: true }],
      hint: "Classic token with the repo scope, or fine-grained with read access to Pull requests, Commit statuses and Actions." },
    { id: "integration_gitlab", name: "GitLab",
      fields: [
        { key: "gitlab-url", label: "Instance URL", placeholder: "https://gitlab.com", secret: false },
        { key: "gitlab-token", label: "Token", placeholder: "glpat-…", secret: true },
      ],
      hint: "Personal access token with read_api and read_user scopes." },
    { id: "integration_gitea", name: "Gitea",
      fields: [
        { key: "gitea-url", label: "Instance URL", placeholder: "https://gitea.example.com", secret: false },
        { key: "gitea-token", label: "Token", placeholder: "…", secret: true },
      ] },
    { id: "integration_vercel", name: "Vercel",
      fields: [{ key: "vercel-token", label: "Token", placeholder: "…", secret: true }] },
    { id: "integration_n8n", name: "n8n",
      fields: [
        { key: "n8n-url", label: "Instance URL", placeholder: "https://n8n.example.com", secret: false },
        { key: "n8n-api-key", label: "API key", placeholder: "…", secret: true },
      ] },
    { id: "integration_resend", name: "Resend",
      fields: [{ key: "resend-api-key", label: "API key", placeholder: "re_…", secret: true }] },
    { id: "integration_notion", name: "Notion",
      fields: [{ key: "notion-api-key", label: "Integration token", placeholder: "ntn_…", secret: true }] },
    { id: "integration_calcom", name: "Cal.com",
      fields: [{ key: "calcom-api-key", label: "API key", placeholder: "cal_…", secret: true }] },
    // Next Agent: removed WhatsApp/Spotify (owner request): the Spotify row.
  ];
  // Offered on this build, in the catalog's colour (core/pills.ts, the same as the window's table).
  const shown = INTEGRATIONS.flatMap((d) => {
    const pill = availablePills().find((p) => p.id === d.id);
    return pill ? [{ ...d, color: pill.color }] : [];
  });
  const STORED = "••••••••  (stored)";
  const KEY_STORE = keyStoreName(navigator.userAgent);

  const s = $derived(store.settings);
  let present = $state<Record<string, boolean>>({});
  let failed = $state<Record<string, boolean>>({});
  let input = $state<Record<string, string>>({});

  function toggle(id: string) {
    const on = s.activeIntegrations.includes(id);
    if (on) {
      void store.set({ activeIntegrations: s.activeIntegrations.filter((x) => x !== id) });
    } else {
      if (s.activeIntegrations.length >= MAX_DECLARED) return;
      void store.set({ activeIntegrations: [...s.activeIntegrations, id] });
    }
  }

  /** An empty value clears the entry, as in the window. */
  async function save(key: string) {
    const value = (input[key] ?? "").trim();
    try {
      await overlay.secretSet(key, value);
      present = { ...present, [key]: value.length > 0 };
      input = { ...input, [key]: "" };
      failed = { ...failed, [key]: false };
    } catch {
      failed = { ...failed, [key]: true };
    }
  }

  onMount(async () => {
    const next: Record<string, boolean> = {};
    for (const def of shown) for (const f of def.fields) next[f.key] = (await overlay.secretPresent(f.key)) ?? false;
    present = next;
  });
</script>

<SettingsSection
  id="integrations"
  title="Integrations"
  description="Pick up to {MAX_DECLARED} pills to show next to Next Agent — {s.activeIntegrations.length}/{MAX_DECLARED} in use. Keys are stored in the {KEY_STORE}, never on disk."
/>

{#each shown as def (def.id)}
  {@const active = s.activeIntegrations.includes(def.id)}
  <SettingsSection id={def.id} title={def.name} description={def.hint}>
    {#snippet actions()}
      <PillColor {def} />
      <Switch
        aria-label="Show {def.name} in the island"
        checked={active}
        disabled={!active && s.activeIntegrations.length >= MAX_DECLARED}
        onchange={() => toggle(def.id)}
      />
    {/snippet}
    {#each def.fields as f (f.key)}
      <SettingsRow label={f.label}>
        {#snippet children(ctl)}
          <TextField
            id={ctl.id}
            type={f.secret ? "password" : "text"}
            mono
            autocomplete="off"
            spellcheck="false"
            placeholder={present[f.key] ? STORED : f.placeholder}
            bind:value={() => input[f.key] ?? "", (v) => (input = { ...input, [f.key]: String(v ?? "") })}
          />
          <Button onclick={() => void save(f.key)}>Save</Button>
          <StatusDot
            tone={failed[f.key] ? "warn" : present[f.key] ? "ok" : "danger"}
            label={failed[f.key] ? "Could not reach the keychain" : present[f.key] ? "Stored" : "Not set"}
          />
        {/snippet}
      </SettingsRow>
    {/each}
  </SettingsSection>
{/each}
