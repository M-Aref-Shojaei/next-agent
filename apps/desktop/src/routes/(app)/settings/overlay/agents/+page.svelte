<script lang="ts">
  // Settings → Overlay → Agents: Claude Code's hooks, the other agents' hooks
  // and the plan usage relay (settings/main.ts claudeSection, planSection;
  // settings/agents.ts). Nothing is written until the user has seen the exact
  // diff and where the dated backup goes, and confirmed it; the write then
  // carries that preview's fingerprint, so a file that changed since is not
  // written over.
  import { log } from "$lib/log.ts";
  import { onMount } from "svelte";
  import { Banner, Button, Checkbox, SettingsRow, SettingsSection, StatusDot, Switch } from "$lib/ds/index.ts";
  import ConfigReview, { type Review } from "$lib/components/overlay/ConfigReview.svelte";
  import { agentDoneText, errorText, overlay } from "$lib/overlay-settings.ts";
  import { overlayStore as store } from "$lib/overlay-store.svelte.ts";
  import type { AgentHookStatus, AgyApprovalsStatus, HookPreview, HookStatus } from "$coucou/core/bridge";

  // ── One kind of change to ~/.claude/settings.json, with its words ──────────
  interface Change {
    preview: (install: boolean) => Promise<HookPreview>;
    apply: (install: boolean, fingerprint: string) => Promise<string>;
    title: string;
    installText: string;
    removeText: string;
    /** The note once written; `backup` is "" when there was no file to back up. */
    done: (backup: string) => string;
  }
  const HOOKS: Change = {
    preview: overlay.hooksPreview,
    apply: overlay.hooksApply,
    title: "Claude Code hooks",
    installText: "This is exactly what will change in your settings.json. Your own hooks are left untouched.",
    removeText: "This removes Next Agent's entries only. Your own hooks are left untouched.",
    done: (backup) => backup
      ? `Done. Previous settings saved as ${backup}. Open a new Claude Code session to pick the hooks up.`
      : "Done. Open a new Claude Code session to pick the hooks up.",
  };
  const STATUS_LINE: Change = {
    preview: overlay.statusLinePreview,
    apply: overlay.statusLineApply,
    title: "Plan usage relay",
    installText: "This is exactly what will change: only the status line. If you already have one it keeps working, Next Agent's relay runs it for you.",
    removeText: "This puts your previous status line back, or removes the entry if there was none.",
    done: (backup) => backup
      ? `Done. Previous settings saved as ${backup}. The numbers appear after the next reply of a Claude Code session.`
      : "Done. The numbers appear after the next reply of a Claude Code session.",
  };

  let status = $state<HookStatus>({ installed: false, planRelayInstalled: false, settingsPath: "", hookPath: "", hookReady: false });
  let agents = $state<AgentHookStatus[] | null>(null);
  let agy = $state<AgyApprovalsStatus | null>(null);
  let agyNote = $state("");

  /** The change being reviewed, and what to do once it is closed. */
  let review = $state<Review | null>(null);
  let afterCancel: (() => void) | null = null;
  /** Per block ("claude", "plan", an agent id): the last note, ok or not. */
  let notes = $state<Record<string, { tone: "ok" | "danger"; text: string; back?: boolean }>>({});
  const note = (block: string, tone: "ok" | "danger", text: string, back = false) => (notes = { ...notes, [block]: { tone, text, back } });
  const clearNote = (block: string) => {
    const { [block]: _, ...rest } = notes;
    notes = rest;
  };

  /**
   * Shows the diff of a change and writes it only after an explicit click, and
   * only if the file still matches the diff that was shown. `back` runs when
   * the review is left; `applied` a moment after a successful write.
   */
  async function reviewChange(block: string, change: Change, install: boolean, back: () => void, applied: () => void) {
    clearNote(block);
    let preview: HookPreview;
    try {
      preview = await change.preview(install);
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_agents", error: String(err) });
      // An unreadable or invalid settings.json stops here rather than being
      // treated as empty and written over.
      note(block, "danger", errorText(err), true);
      back();
      return;
    }
    afterCancel = back;
    review = {
      title: change.title,
      text: install ? change.installText : change.removeText,
      diff: preview.diff,
      backup: preview.backup ? `Backup → ${preview.backup}` : "No settings.json yet — nothing to back up.",
      confirmLabel: install ? "Back up and write" : "Back up and remove",
      danger: !install,
      apply: async () => {
        const backup = await change.apply(install, preview.fingerprint);
        window.setTimeout(applied, 2600);
        return change.done(backup ?? "");
      },
      failed: (err) => `Could not write: ${String(err)}`,
    };
    reviewBlock = block;
  }
  let reviewBlock = "";

  async function rebuildHooks() {
    const fresh = await overlay.hooksStatus();
    if (fresh) status = { ...status, ...fresh };
  }

  // ── Plan usage ──────────────────────────────────────────────────────────────
  /** The switch shows "on" while the install it asked for is being reviewed. */
  let planPending = $state<"notch" | "compact" | null>(null);

  async function rebuildPlan() {
    await rebuildHooks();
    planPending = null;
    const change: Partial<typeof store.settings> = { planRelayInstalled: status.planRelayInstalled };
    // Cancelled or failed: a switch that was waiting for the install falls back.
    if (!status.planRelayInstalled) {
      change.showPlanInNotch = false;
      change.showPlanInCompactIsland = false;
    }
    store.settings = { ...store.settings, ...change };
  }

  function planSwitch(which: "notch" | "compact", on: boolean) {
    const field = which === "notch" ? "showPlanInNotch" : "showPlanInCompactIsland";
    if (!on) {
      void store.set({ [field]: false });
    } else if (status.planRelayInstalled) {
      void store.set({ [field]: true });
    } else {
      // Turned on before the relay is in: install it first; it stays on once confirmed.
      planPending = which;
      void reviewChange("plan", STATUS_LINE, true, () => void rebuildPlan(), () => {
        void store.set({ planRelayInstalled: true, [field]: true }).then(rebuildPlan);
      });
    }
  }

  // ── Other agents ────────────────────────────────────────────────────────────
  async function refreshAgent(id: string) {
    const fresh = (await overlay.agentHooksList())?.find((a) => a.id === id);
    if (fresh && agents) agents = agents.map((a) => (a.id === id ? fresh : a));
    if (id === "antigravity") agy = await overlay.agyApprovalsStatus();
  }

  async function agentReview(agent: AgentHookStatus, install: boolean) {
    clearNote(agent.id);
    let plan;
    try {
      plan = await overlay.agentHooksPreview(agent.id, install);
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_agents", error: String(err) });
      // An unreadable or unexpected config stops here rather than being treated as empty and written over.
      note(agent.id, "danger", errorText(err), true);
      return;
    }
    afterCancel = null;
    reviewBlock = agent.id;
    review = {
      title: agent.name,
      text: install ? `This is exactly what changes for ${agent.name}. Nothing else is touched.` : "This removes Next Agent's entries only. Everything else stays.",
      diff: plan.diff,
      backup: plan.backup ? `Backup → ${plan.backup}` : "No existing file — nothing to back up.",
      confirmLabel: install ? "Back up and write" : "Back up and remove",
      danger: !install,
      apply: async () => {
        const backups = await overlay.agentHooksApply(agent.id, install, plan.fingerprint);
        window.setTimeout(() => void refreshAgent(agent.id), 2600);
        return agentDoneText(backups, install ? agent.note : "");
      },
      failed: (err) => `Could not write: ${errorText(err)}`,
    };
  }

  /**
   * Antigravity's commands waiting for Allow / Deny on the island: off by
   * default, shown once the hooks are installed. Hooks written by an older
   * version give Antigravity 10 s, too short to wait for a click.
   */
  async function setAgy(enabled: boolean) {
    agyNote = "";
    try {
      agy = await overlay.agyApprovalsSetEnabled(enabled);
    } catch (err) {
      log.warn("ui_error", { kind: "overlay_agents", error: String(err) });
      agy = agy ? { ...agy, enabled: !enabled } : agy;
      agyNote = errorText(err);
    }
  }

  onMount(async () => {
    status = (await overlay.hooksStatus()) ?? status;
    agents = await overlay.agentHooksList();
    if (agents?.some((a) => a.id === "antigravity")) agy = await overlay.agyApprovalsStatus();
  });
</script>

{#snippet blockNote(block: string)}
  {#if notes[block]}
    <Banner tone={notes[block].tone} ondismiss={notes[block].back ? () => clearNote(block) : undefined}>{notes[block].text}</Banner>
  {/if}
{/snippet}

<SettingsSection
  id="claude-code"
  title="Claude Code"
  description={status.installed
    ? "Next Agent is hooked into your Claude Code sessions. Tool calls, questions and permission requests show up in the island, and you can answer them there."
    : "Install the hooks to see your Claude Code sessions in the island and approve permissions without leaving what you are doing."}
>
  {#snippet actions()}
    <StatusDot tone={status.installed ? "ok" : "neutral"} text={status.installed ? "Installed" : "Not installed"} />
  {/snippet}
  <SettingsRow label="settings.json" description={status.settingsPath}>
    {#snippet children()}
      {#if status.installed}
        <Button variant="danger" onclick={() => void reviewChange("claude", HOOKS, false, () => {}, () => void rebuildHooks())}>Uninstall hooks…</Button>
      {/if}
      <Button
        variant="primary"
        disabled={!status.hookReady}
        title={status.hookReady ? undefined : "The relay isn't installed yet."}
        onclick={() => void reviewChange("claude", HOOKS, true, () => {}, () => void rebuildHooks())}
      >{status.installed ? "Reinstall hooks…" : "Install hooks…"}</Button>
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Relay" description={status.hookPath}>
    {#snippet children()}
      <StatusDot tone={status.hookReady ? "ok" : "danger"} text={status.hookReady ? "In place" : "Missing"} />
    {/snippet}
  </SettingsRow>
  {#snippet footer()}
    <div class="notes">
      {#if !status.hookReady}
        <Banner tone="warn">next-agent-hook is not in place yet. Restart Next Agent; if it still fails, build it with <code>cargo build -p next-agent-hook</code>.</Banner>
      {/if}
      {@render blockNote("claude")}
    </div>
  {/snippet}
</SettingsSection>

<SettingsSection
  id="other-agents"
  title="Agents"
  description="Show other coding agents in the island. Next Agent adds its entries to each agent's own config: you see the exact change and where the backup goes before anything is written, and uninstalling removes only what Next Agent added."
>
  {#if agents && agents.length === 0}
    <SettingsRow label="Next Agent could not list the agents." />
  {/if}
  {#each agents ?? [] as agent (agent.id)}
    <SettingsRow label={agent.name} description={agent.path}>
      {#snippet children()}
        <StatusDot
          tone={agent.installed ? "ok" : "neutral"}
          text={agent.approvals || agy?.enabled ? "Sessions, and Allow / Deny from the island" : "Sessions — approvals stay in the agent"}
        />
        {#if agent.installed}
          <Button variant="danger" onclick={() => void agentReview(agent, false)}>Uninstall…</Button>
        {/if}
        <Button
          variant="primary"
          disabled={!agent.hookReady}
          title={agent.hookReady ? undefined : "The relay isn't installed yet. Restart Next Agent."}
          onclick={() => void agentReview(agent, true)}
        >{agent.installed ? "Reinstall…" : "Install…"}</Button>
      {/snippet}
    </SettingsRow>
    {#if agent.id === "antigravity" && agent.installed && agy}
      <SettingsRow
        label="Approve commands from the notch"
        description={agy.enabled
          ? "Before Antigravity runs a shell command that none of its rules covers, the notch asks you first: Allow, Deny or Reply in terminal. Antigravity waits up to 2 min, then asks in its terminal. Next Agent can't see Antigravity's mode, so in Auto mode the notch also asks about commands Antigravity would have run on its own."
          : "Antigravity asks for permission in its terminal, as usual."}
        error={agyNote || undefined}
      >
        {#snippet children(ctl)}
          <Checkbox id={ctl.id} aria-describedby={ctl.describedby} checked={agy?.enabled ?? false} onchange={(on) => void setAgy(on)} />
        {/snippet}
      </SettingsRow>
      {#if agy.enabled && !agy.hooksReady}
        <div class="inline-note">
          <Banner tone="warn">Click Install hooks once more, then restart Antigravity: hooks from an older Next Agent don't let it wait for your answer.</Banner>
        </div>
      {/if}
    {/if}
    {#if notes[agent.id]}<div class="inline-note">{@render blockNote(agent.id)}</div>{/if}
  {/each}
</SettingsSection>

<SettingsSection
  id="plan"
  title="Plan usage"
  description="Shows your Claude plan usage (5-hour and weekly limits) in the island's header. Next Agent adds a status line relay in ~/.claude/settings.json. If you already have a status line, it keeps working as before. Pro and Max plans only."
>
  <SettingsRow
    label="Show in notch"
    description="The numbers come from the Claude Code command-line tool. The Claude desktop app does not report them, so the gauge stays empty if you only use the desktop app."
  >
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={store.settings.showPlanInNotch || planPending === "notch"} onchange={(on) => planSwitch("notch", on)} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Show in compact island">
    {#snippet children(ctl)}
      <Switch id={ctl.id} checked={store.settings.showPlanInCompactIsland || planPending === "compact"} onchange={(on) => planSwitch("compact", on)} />
    {/snippet}
  </SettingsRow>
  <SettingsRow label="Relay">
    {#snippet children()}
      <StatusDot tone={status.planRelayInstalled ? "ok" : "neutral"} text={status.planRelayInstalled ? "installed" : "not installed"} />
      {#if status.planRelayInstalled}
        <Button variant="danger" onclick={() => void reviewChange("plan", STATUS_LINE, false, () => {}, () => void rebuildPlan())}>Uninstall relay…</Button>
      {:else}
        <Button variant="primary" onclick={() => void reviewChange("plan", STATUS_LINE, true, () => {}, () => void rebuildPlan())}>Install relay…</Button>
      {/if}
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Show Codex plan in the notch"
    description="Shows your Codex plan usage (weekly limit and free resets left) in the island's header. Next Agent asks the Codex CLI (codex app-server) when the pill shows; nothing is installed. Codex must be signed in with ChatGPT."
  >
    {#snippet children(ctl)}
      <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={store.settings.showCodexPlanInNotch} onchange={(on) => void store.set({ showCodexPlanInNotch: on })} />
    {/snippet}
  </SettingsRow>
  {#snippet footer()}{@render blockNote("plan")}{/snippet}
</SettingsSection>

<ConfigReview
  bind:review
  ondone={(text) => note(reviewBlock, "ok", text)}
  oncancel={() => {
    afterCancel?.();
    afterCancel = null;
  }}
/>

<style>
  .notes { display: grid; gap: var(--space-2); }
  .inline-note { padding: var(--space-2) var(--space-4); }
  code { font-family: var(--font-mono); font-size: var(--font-size-xs); }
</style>
