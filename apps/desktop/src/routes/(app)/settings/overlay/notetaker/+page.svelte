<script lang="ts">
  // Settings → Overlay → Notetaker: its summaries (Coucou's Flow → Settings →
  // Notetaker card, hub/pages/flowsettings.ts). The recordings themselves stay
  // in the Notetaker window.
  import { onMount } from "svelte";
  import { Select, SettingsRow, SettingsSection } from "$lib/ds/index.ts";
  import { flowStore } from "$lib/overlay-store.svelte.ts";

  const options: { value: "claude" | "off"; label: string }[] = [
    { value: "claude", label: "Claude (your key)" },
    { value: "off", label: "No summary" },
  ];

  onMount(() => void flowStore.refresh());
</script>

<SettingsSection id="summaries" title="Summaries">
  <SettingsRow label="Summaries" description="With Claude, the transcript (never the audio) goes to Anthropic with your key from Overlay → Chat.">
    {#snippet children(ctl)}
      <Select
        id={ctl.id}
        aria-describedby={ctl.describedby}
        {options}
        value={flowStore.status.prefs.summaryEngine}
        onchange={(e) => void flowStore.setPrefs({ summaryEngine: e.currentTarget.value as "claude" | "off" })}
      />
    {/snippet}
  </SettingsRow>
  {#snippet footer()}Everything stays on this computer. Tell the people in the call that you are recording.{/snippet}
</SettingsSection>
