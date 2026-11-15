<script lang="ts">
  // Settings → Overlay → Shortcuts: the island's global shortcuts
  // (settings/main.ts shortcutsSection). Coucou's own recorder (with its AltGr
  // rule) records; global shortcuts are let go while it does.
  import { onMount } from "svelte";
  import { Badge, Banner, Button, SettingsRow, SettingsSection, ShortcutField, Switch } from "$lib/ds/index.ts";
  import Keycap from "$lib/components/overlay/Keycap.svelte";
  import { onOverlayEvent, overlay, shortcutTag } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import type { ShortcutsReport } from "$coucou/core/bridge";
  import {
    ISLAND_SHORTCUTS, SHORTCUTS, SHORTCUT_TEXT, activeKeys, duplicates, effective, recordPress, type Binding,
  } from "$coucou/core/shortcuts";

  let report = $state<ShortcutsReport | null>(null);
  let recording = $state(false);
  const ported = SHORTCUTS.filter((d) => d.ported);
  const dups = $derived(duplicates(activeKeys(store.settings.shortcuts)));
  const commands = (command: string) => ported.map((d) => `${command} ${d.id}`);

  function storeBinding(id: string, binding: Binding) {
    void store.set({ shortcuts: { ...store.settings.shortcuts, [id]: binding } });
  }
  function recorded(id: string, binding: Binding, keys: string) {
    // A recorded chord turns the shortcut on; Backspace clears it and keeps the switch.
    storeBinding(id, keys ? { keys, enabled: true } : { keys: "", enabled: binding.enabled });
  }
  function onrecording(on: boolean) {
    recording = on;
    // Takes the global shortcuts back, from what is saved by now.
    void overlay.shortcutsSuspend(on);
  }
  function reset() {
    void store.set({ shortcuts: {} });
  }

  onMount(() => {
    void overlay.shortcutsStatus().then((r) => (report = r));
    const stop = onOverlayEvent<ShortcutsReport>("shortcuts-status", (fresh) => {
      if (!recording) report = fresh;
    });
    return () => void stop.then((f) => f());
  });
</script>

<SettingsSection
  id="global"
  title="From anywhere"
  description="Work from any app. Click a shortcut to change it, then press the new keys — Esc cancels, Backspace removes it."
>
  {#snippet actions()}<Button size="sm" onclick={reset}>Reset to defaults</Button>{/snippet}
  {#each ported as d (d.id)}
    {@const binding = effective(d, store.settings.shortcuts)}
    {@const tag = binding.enabled ? shortcutTag(dups.has(d.id), report?.actions.find((a) => a.id === d.id)) : null}
    <SettingsRow label={SHORTCUT_TEXT[d.id]} disabled={!binding.enabled}>
      {#snippet children(ctl)}
        {#if tag}<Badge tone={tag.tone} size="sm">{tag.text}</Badge>{/if}
        <ShortcutField
          id={ctl.id}
          value={binding.keys}
          placeholder="None"
          clearable={false}
          requireModifier
          disabled={!binding.enabled}
          record={recordPress}
          {onrecording}
          onchange={(keys) => recorded(d.id, binding, keys)}
        />
        <Switch
          aria-label="{SHORTCUT_TEXT[d.id]}: on"
          checked={binding.enabled}
          onchange={(on) => storeBinding(d.id, { keys: binding.keys, enabled: on })}
        />
      {/snippet}
    </SettingsRow>
  {/each}
  {#snippet footer()}
    {#if report?.portal}
      <!-- Wayland, through the desktop's GlobalShortcuts portal; the commands stay as a way around. -->
      <Banner tone={report.portal === "active" ? "ok" : "info"}>
        {report.portal === "active"
          ? "These shortcuts are registered with your desktop. It may ask you to confirm them or to pick other keys; when it says which keys it uses, they show next to each shortcut."
          : "Asking your desktop to register the shortcuts. It may show its own window to confirm them."}
      </Banner>
      <p>If one doesn't work, you can also add it in your system's keyboard settings with these commands:</p>
      <pre>{commands(report.command).join("\n")}</pre>
    {:else if report?.blocked === "wayland"}
      <Banner tone="warn">Your Wayland desktop doesn't let apps listen for keys outside their own windows. Add the shortcuts in your system's keyboard settings instead, with these commands:</Banner>
      <pre>{commands(report.command).join("\n")}</pre>
    {:else if report?.blocked}
      <Banner tone="warn">No display server was found, so global shortcuts are off.</Banner>
    {/if}
  {/snippet}
</SettingsSection>

<SettingsSection id="island" title="In the open island">
  {#each ISLAND_SHORTCUTS as row (row.description)}
    <SettingsRow label={row.description}>
      {#snippet children()}<Keycap text={row.keys} />{/snippet}
    </SettingsRow>
  {/each}
</SettingsSection>

<style>
  p { margin: var(--space-2) 0; }
  pre {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-sunken);
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
    overflow: auto;
  }
</style>
