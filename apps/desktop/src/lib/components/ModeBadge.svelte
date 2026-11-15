<script lang="ts">
  import type { ModeName } from "@next-agent/contracts";
  import { MODE_LABELS, MODE_TONE } from "$lib/modes.ts";
  import { bypassConfirmed } from "$lib/shortcuts.ts";
  import { Dialog, TextField } from "$lib/ds/index.ts";

  let {
    mode,
    workspaceName,
    onselect,
  }: { mode: ModeName; workspaceName: string; onselect: (mode: ModeName) => void } = $props();

  const ALL: ModeName[] = ["plan", "manual", "auto", "bypassPermissions"];
  let open = $state(false);
  let confirming = $state(false);
  let typed = $state("");

  function choose(next: ModeName) {
    open = false;
    if (next === "bypassPermissions") {
      typed = "";
      confirming = true;
    } else onselect(next);
  }
  const matches = $derived(bypassConfirmed(typed, workspaceName));
  /** The dialog stays open (the confirm throws) until the workspace name matches. */
  function confirm() {
    if (!matches) throw new Error("Workspace name does not match");
    onselect("bypassPermissions");
  }
</script>

<span class="wrap">
  <button
    class="badge tone-{MODE_TONE[mode]}"
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label="Permission mode: {MODE_LABELS[mode]}. Shift+Tab in the chat box cycles modes."
    onclick={() => (open = !open)}
    onkeydown={(e) => e.key === "Escape" && (open = false)}
  >
    {MODE_LABELS[mode].toUpperCase()}
  </button>
  {#if open}
    <ul class="menu" role="menu" aria-label="Select permission mode">
      {#each ALL as m (m)}
        <li role="none">
          <button role="menuitemradio" aria-checked={m === mode} class="tone-{MODE_TONE[m]}" onclick={() => choose(m)}>
            {MODE_LABELS[m]}{m === "bypassPermissions" ? " (confirm)" : ""}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <Dialog
    bind:open={confirming}
    tone="danger"
    title="Enable Bypass mode?"
    confirmLabel="Enable Bypass"
    onconfirm={confirm}
  >
    <p class="warning">
      Bypass unlocks the sandbox: every sensitive action runs without asking (and is audited). Type the
      workspace name <strong>{workspaceName || "(none open)"}</strong> to confirm.
    </p>
    <TextField
      label="Workspace name"
      hideLabel
      bind:value={typed}
      onkeydown={(e) => {
        if (e.key === "Enter" && matches) {
          confirm();
          confirming = false;
        }
      }}
    />
  </Dialog>
</span>

<style>
  .wrap { position: relative; display: inline-block; }
  .badge {
    height: var(--control-height-sm);
    padding: 0 var(--space-3);
    color: var(--tone);
    border-color: var(--tone);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-bold);
    letter-spacing: var(--letter-spacing-caps);
  }
  .badge.tone-info { --tone: var(--color-info-text); }
  .badge.tone-neutral { --tone: var(--color-text-secondary); }
  .badge.tone-warn { --tone: var(--color-warn-text); }
  .badge.tone-danger { --tone: var(--color-on-danger); background: var(--color-danger); border-color: var(--color-danger); }
  .badge:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .menu {
    position: absolute;
    bottom: 120%;
    left: 0;
    list-style: none;
    margin: 0;
    padding: var(--space-1);
    min-width: 150px;
    background: var(--color-surface-raised);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    z-index: var(--z-popover);
  }
  .menu .tone-info { --tone: var(--color-info-text); }
  .menu .tone-neutral { --tone: var(--color-text); }
  .menu .tone-warn { --tone: var(--color-warn-text); }
  .menu .tone-danger { --tone: var(--color-danger-text); }
  .menu button { width: 100%; text-align: left; border: 0; background: transparent; color: var(--tone); }
  .menu button:hover { background: var(--color-surface-hover); }
  .menu button[aria-checked="true"] { font-weight: var(--font-weight-bold); }
  .warning { margin: 0 0 var(--space-3); }
</style>
