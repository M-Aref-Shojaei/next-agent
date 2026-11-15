<script lang="ts">
  // Settings → Overlay → About: what used to be the Coucou window's brand and
  // version line (owner request: one design system, no Coucou artwork). The
  // credit and licence lines are NOTICE.md's.
  import { BrandMark, Card, SettingsRow, SettingsSection } from "$lib/ds/index.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";

  const PIN = "0d7fe58";
</script>

<Card variant="hero" title="About Next Agent" description="The island at the top of the screen, its dictation, Notetaker, voice and phone link are built in from Coucou's code.">
  {#snippet media()}<BrandMark size={48} />{/snippet}
</Card>

<SettingsSection id="versions" title="Versions">
  <SettingsRow label="Overlay" description="The vendored Coucou app, at upstream commit {PIN}.">
    {#snippet children()}<span class="mono">{store.version || "—"}</span>{/snippet}
  </SettingsRow>
</SettingsSection>

<SettingsSection id="credits" title="Credits and licences">
  <SettingsRow
    label="Next Agent"
    description="Licensed under the GNU Affero General Public License v3.0 or later (see LICENSE). NOTICE.md credits every project it ports from."
  />
  <SettingsRow
    label="Coucou"
    description="The whole Tauri app (Rust, TypeScript, hook relay, tests), vendored from github.com/Louis-CFM/coucou. MIT licence, © 2026 Louis Raillé (code only)."
  />
  <SettingsRow label="Coucou phone link" description="The phone link and its Android relay: MIT licence, © 2026 Rhytam Biswas." />
  <SettingsRow
    label="sherpa-onnx, onnxruntime"
    description="On-device speech to text for dictation and the Notetaker. Apache-2.0, © Xiaomi Corporation and contributors; onnxruntime MIT, © Microsoft. The speech models download only on your click."
  />
  {#snippet footer()}
    Not covered by Coucou's licence, and not in Next Agent: the Coucou name in new work, the Mochi character, its icons, sounds
    and other media. A neutral orb stands in for the character.
  {/snippet}
</SettingsSection>

<style>
  .mono { font-family: var(--font-mono); font-size: var(--font-size-sm); color: var(--color-text-secondary); }
</style>
