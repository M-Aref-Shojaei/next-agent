<script lang="ts">
  import type { ModeName } from "@next-agent/contracts";
  import { MODE_LABELS, MODE_TONE } from "$lib/modes.ts";
  import { bypassConfirmed } from "$lib/shortcuts.ts";

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
  function confirm() {
    if (bypassConfirmed(typed, workspaceName)) {
      confirming = false;
      onselect("bypassPermissions");
    }
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
  {#if confirming}
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="bypass-title">
      <h2 id="bypass-title">Enable Bypass mode?</h2>
      <p>
        Bypass unlocks the sandbox: every sensitive action runs without asking (and is audited). Type the
        workspace name <strong>{workspaceName || "(none open)"}</strong> to confirm.
      </p>
      <input aria-label="Workspace name" bind:value={typed} onkeydown={(e) => e.key === "Enter" && confirm()} />
      <div class="row">
        <button onclick={() => (confirming = false)}>Cancel</button>
        <button class="danger" disabled={!bypassConfirmed(typed, workspaceName)} onclick={confirm}>Enable Bypass</button>
      </div>
    </div>
  {/if}
</span>

<style>
  .wrap { position: relative; display: inline-block; }
  .badge {
    color: var(--tone); border-color: var(--tone); font-weight: 700; letter-spacing: 0.06em; padding: 1px 10px;
  }
  .badge.tone-danger { background: var(--danger); color: #fff; }
  .menu {
    position: absolute; bottom: 120%; left: 0; list-style: none; margin: 0; padding: 4px; min-width: 150px;
    background: var(--panel); border: 1px solid var(--border); border-radius: 8px; z-index: 5;
  }
  .menu button { width: 100%; text-align: left; border: 0; color: var(--tone); }
  .menu button[aria-checked="true"] { font-weight: 700; }
  .dialog {
    position: fixed; inset: 30% auto auto 50%; transform: translateX(-50%); width: 420px; z-index: 10;
    background: var(--panel); border: 2px solid var(--danger); border-radius: 10px; padding: 16px; display: grid; gap: 10px;
  }
  .row { display: flex; gap: 8px; justify-content: flex-end; }
</style>
