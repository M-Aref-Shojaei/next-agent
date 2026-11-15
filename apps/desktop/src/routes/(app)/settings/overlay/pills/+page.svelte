<script lang="ts">
  // Settings → Overlay → Pills and colours: Coucou's Active pills section
  // (settings/main.ts activePillsSection): the main tool, the declared pills
  // (at most four) and each pill's colour. Services are declared under
  // Integrations, next to their keys.
  import { onMount } from "svelte";
  import { Badge, Select, SettingsRow, SettingsSection, Switch } from "$lib/ds/index.ts";
  import PillColor from "$lib/components/overlay/PillColor.svelte";
  import { overlay } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import {
    MAX_DECLARED, PILL_CATEGORIES, availablePills, chooseMainPill, isComingSoon, mainPillChoices, toggleDeclared,
    type PillDefinition,
  } from "$coucou/core/pills";

  const s = $derived(store.settings);
  const used = $derived(s.activeIntegrations.length);
  const mainOptions = mainPillChoices().map((d) => ({ value: d.id, label: d.name }));
  const groups = PILL_CATEGORIES.filter((c) => c.id !== "service")
    .map((c) => ({ ...c, pills: availablePills().filter((p) => p.category === c.id) }))
    .filter((g) => g.pills.length > 0);

  // Integration keys the window read before drawing (settings/main.ts render()).
  const INTEGRATION_KEYS = [
    "stripe-api-key", "github-token", "gitlab-url", "gitlab-token", "gitea-url", "gitea-token", "vercel-token",
    "n8n-url", "n8n-api-key", "resend-api-key", "notion-api-key", "calcom-api-key",
  ];
  let connected = $state<Record<string, boolean>>({});

  onMount(async () => {
    const present: Record<string, boolean> = {};
    for (const k of INTEGRATION_KEYS) present[k] = (await overlay.secretPresent(k)) ?? false;
    const next: Record<string, boolean> = { ...((await overlay.agentHooksStatus()) ?? {}) };
    for (const def of availablePills()) {
      if (def.connect.kind === "key") next[def.id] = present[def.connect.key] ?? (await overlay.keyReady(def.connect.key)) ?? false;
    }
    connected = next;
  });

  /** Why a pill would show nothing yet, as on the Mac's row. */
  function hint(def: PillDefinition): string | null {
    if (isComingSoon(def.id)) return "Coming soon";
    if (def.connect.kind === "hooks" && !connected[def.id]) return "Hooks not installed";
    if (def.connect.kind === "key" && !connected[def.id]) return "Key not configured";
    if (def.connect.kind === "server" && !s[def.connect.field]) return "Not connected";
    return null;
  }

  function chooseMain(id: string) {
    const next = chooseMainPill(s, id);
    if (!next) return;
    void store.set({ mainPill: next.mainPill, activeIntegrations: next.activeIntegrations });
  }

  function toggle(id: string) {
    const next = toggleDeclared(s, id);
    if (!next) return;
    void store.set({ activeIntegrations: next });
  }
</script>

<SettingsSection
  id="pills"
  title="Active pills"
  description="Choose the tools you use. Next Agent only shows what you declare here."
>
  <SettingsRow label="Main tool" description="{used}/{MAX_DECLARED} slots in use — the main tool doesn't take one.">
    {#snippet children(ctl)}
      <Select id={ctl.id} aria-describedby={ctl.describedby} options={mainOptions} value={s.mainPill} onchange={(e) => chooseMain(e.currentTarget.value)} />
    {/snippet}
  </SettingsRow>
</SettingsSection>

{#each groups as group (group.id)}
  <SettingsSection id="pills-{group.id}" title={group.title}>
    {#each group.pills as def (def.id)}
      {@const isMain = def.id === s.mainPill}
      {@const on = s.activeIntegrations.includes(def.id)}
      {@const full = !isMain && !on && used >= MAX_DECLARED}
      {@const why = isMain ? null : hint(def)}
      <SettingsRow label={def.name} description={why ?? undefined} disabled={full}>
        {#snippet children(ctl)}
          <PillColor {def} />
          {#if isMain}
            <Badge tone="accent">Main</Badge>
          {:else}
            <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={on} disabled={full} onchange={() => toggle(def.id)} />
          {/if}
        {/snippet}
      </SettingsRow>
    {/each}
  </SettingsSection>
{/each}
