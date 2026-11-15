<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
  import {
    Banner, Button, Checkbox, EmptyState, FormActions, List, ListRow, Select, SettingsLayout, SettingsRow,
    SettingsSection, StatusDot, Switch, TextField, type SettingsNavItem, type Tone,
  } from "$lib/ds/index.ts";
  import {
    SELECTABLE_TOOLS,
    expiresIn,
    formErrors,
    healthLabel,
    newChannel,
    parseIds,
    toggleTool,
    type AllowEntry,
    type BotIdentity,
    type ChannelActivity,
    type ChannelConfig,
    type ChannelInfo,
    type PairingRequest,
  } from "$lib/channels.ts";

  let channels = $state<ChannelInfo[]>([]);
  let selectedId = $state<string | null>(null);
  let form = $state<ChannelConfig | null>(null);
  // Write-only: never filled from the backend, cleared after saving.
  let token = $state("");
  let groupsText = $state("");
  let testResult = $state<BotIdentity | string | null>(null);
  let pairings = $state<PairingRequest[]>([]);
  let allow = $state<AllowEntry[]>([]);
  let activity = $state<ChannelActivity[]>([]);
  let newSender = $state("");
  let pairCode = $state("");
  let newLabel = $state("");
  let message = $state("");
  let busy = $state(false);
  let now = $state(Math.floor(Date.now() / 1000));

  const selected = $derived(channels.find((c) => c.config.id === selectedId) ?? null);
  const nav = $derived<SettingsNavItem[]>([
    { id: "channels", label: "Your channels", href: "#channels" },
    ...(form ? [{ id: "bot", label: selected ? "Bot settings" : "New bot", href: "#bot" }] : []),
    ...(selected
      ? [
          { id: "pairing", label: "Pending pairings", href: "#pairing", badge: pairings.length || undefined },
          { id: "allow", label: "Allow-list", href: "#allow" },
          { id: "activity", label: "Activity", href: "#activity" },
        ]
      : []),
  ]);
  const healthTone: Record<"ok" | "info" | "error", Tone> = { ok: "ok", info: "neutral", error: "danger" };
  const workspaceName = (id: string) => app.workspaces.find((w) => w.id === id)?.name ?? id;
  const arrow: Record<string, string> = { in: "←", out: "→" };
  const errors = $derived(form ? formErrors(form, allow, token, selected?.hasToken ?? false) : []);

  async function loadList() {
    channels = await api.channelList();
    if (selectedId && !channels.some((c) => c.config.id === selectedId)) selectedId = null;
  }

  async function loadDetails() {
    if (!selectedId) {
      pairings = [];
      allow = [];
      activity = [];
      return;
    }
    const id = selectedId;
    [pairings, allow, activity] = await Promise.all([
      api.channelPairings(id),
      api.channelAllowList(id),
      api.channelActivity(id, 50),
    ]);
  }

  function edit(info: ChannelInfo) {
    selectedId = info.config.id;
    form = { ...info.config, groups: [...info.config.groups], tools: [...info.config.tools] };
    groupsText = info.config.groups.join(", ");
    token = "";
    testResult = null;
    void loadDetails().catch((e) => (message = String(e)));
  }

  function add() {
    selectedId = null;
    form = newChannel(app.activeWorkspaceId ?? app.workspaces[0]?.id ?? "");
    groupsText = "";
    token = "";
    testResult = null;
    void loadDetails();
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!form || errors.length) return;
    busy = true;
    try {
      const saved = await api.channelSave({ ...form, groups: parseIds(groupsText) }, token.trim() || undefined);
      token = "";
      await loadList();
      const fresh = channels.find((c) => c.config.id === saved.config.id) ?? saved;
      edit(fresh);
      message = `Saved ${saved.config.name}.`;
    } catch (e) {
      message = String(e);
    } finally {
      busy = false;
    }
  }

  async function test() {
    testResult = "Testing…";
    try {
      testResult = await api.channelTest(selectedId, token.trim() || undefined);
    } catch (e) {
      testResult = String(e);
    }
  }

  /** Run an action; on success set `done` as the message, on failure show the error. */
  async function run(work: () => Promise<unknown>, done?: string) {
    try {
      await work();
      if (done) message = done;
    } catch (e) {
      message = String(e);
    }
  }

  async function remove() {
    if (!selected) return;
    if (!confirm(`Delete ${selected.config.name}? Its token, pairings, allow-list and activity are removed.`)) return;
    const id = selected.config.id;
    await run(async () => {
      await api.channelDelete(id);
      form = null;
      selectedId = null;
      await loadList();
    }, "Channel deleted.");
  }

  async function restart() {
    const id = selectedId;
    if (!id) return;
    await run(async () => {
      await api.channelRestart(id);
      await loadList();
    });
  }

  /** Approve the request whose code the owner typed (never one picked from a list). */
  async function approveCode(event: SubmitEvent) {
    event.preventDefault();
    const id = selectedId;
    if (!id || !pairCode.trim()) return;
    await run(async () => {
      const request = await api.channelPairingResolve(id, pairCode.trim(), true);
      pairCode = "";
      message = `Paired ${request.label || request.senderId} (id ${request.senderId}).`;
      await loadDetails();
    });
  }

  async function deny(request: PairingRequest) {
    const id = selectedId;
    if (!id) return;
    await run(async () => {
      await api.channelPairingResolve(id, request.code, false);
      message = `Denied ${request.label || request.senderId}.`;
      await loadDetails();
    });
  }

  async function addAllowed(event: SubmitEvent) {
    event.preventDefault();
    const id = selectedId;
    if (!id || !newSender.trim()) return;
    await run(async () => {
      await api.channelAllowAdd(id, newSender.trim(), newLabel.trim());
      newSender = "";
      newLabel = "";
      await loadDetails();
    });
  }

  async function removeAllowed(entry: AllowEntry) {
    const id = selectedId;
    if (!id) return;
    await run(async () => {
      await api.channelAllowRemove(id, entry.senderId);
      await loadDetails();
    });
  }

  onMount(() => {
    const offs: (() => void)[] = [];
    let disposed = false;
    // Cleanup runs synchronously, but listeners arrive after awaits: drop late ones at once.
    const keep = (off: () => void) => (disposed ? off() : offs.push(off));
    void (async () => {
      try {
        await loadList();
        if (channels[0]) edit(channels[0]);
        keep(
          await api.onChannelHealth((id, health) => {
            channels = channels.map((c) => (c.config.id === id ? { ...c, health } : c));
          }),
        );
        keep(
          await api.onChannelChange((id) => {
            if (id === selectedId) void loadDetails().catch((e) => (message = String(e)));
          }),
        );
      } catch (e) {
        message = String(e);
      }
    })();
    const tick = setInterval(() => (now = Math.floor(Date.now() / 1000)), 30_000);
    return () => {
      disposed = true;
      clearInterval(tick);
      offs.forEach((off) => off());
    };
  });
</script>

<SettingsLayout
  title="Channels"
  description="Message the agent from a chat app. Each message runs as a task in its workspace, in manual or plan mode only, with a tool set you choose (everything else is denied). Approvals happen here or in the overlay, never in the chat; no answer in time means deny. Bot tokens are kept in the system keychain. Design from OpenClaw (MIT)."
  {nav}
  current="channels"
>
  {#snippet top()}<a href="/settings">← Settings</a> · <a href="/">Back to chat</a>{/snippet}

  {#if message}<Banner tone="info" ondismiss={() => (message = "")}>{message}</Banner>{/if}

  <SettingsSection id="channels" title="Your channels">
    {#snippet actions()}<Button variant="primary" onclick={add}>Add Telegram</Button>{/snippet}
    {#if channels.length === 0}
      <EmptyState compact title="No channel yet" description="Add a Telegram bot to message the agent from your phone." />
    {:else}
      <List label="Channels" bordered={false}>
        {#each channels as c (c.config.id)}
          {@const h = healthLabel(c.health, c.config.enabled)}
          <ListRow
            title={c.config.name}
            description="{workspaceName(c.config.workspaceId)} · {c.config.mode}"
            selected={c.config.id === selectedId}
            onclick={() => edit(c)}
          >
            {#snippet leading()}<StatusDot tone={healthTone[h.tone]} label={h.text} />{/snippet}
            <span>{h.text}{#if c.health?.restarts} · {c.health.restarts} restarts{/if}</span>
          </ListRow>
        {/each}
      </List>
    {/if}
  </SettingsSection>

  {#if form}
    <form onsubmit={save}>
      <SettingsSection id="bot" title={selected ? selected.config.name : "New Telegram bot"}>
        {#snippet footer()}
          {#if !selected}
            <ol class="steps">
              <li>In Telegram, message <strong>@BotFather</strong>, send <code>/newbot</code> and copy the token it gives you.</li>
              <li>Paste it below, pick the workspace and press <em>Test connection</em>, then <em>Save</em>.</li>
              <li>Message your bot: it answers with a pairing code. Approve it under <em>Pending pairings</em>.</li>
            </ol>
          {/if}
          {#if testResult}
            {#if typeof testResult !== "string"}
              <Banner tone="ok">Connected to @{testResult.username} ({testResult.name})</Banner>
            {:else}
              <Banner tone={testResult === "Testing…" ? "info" : "danger"}>{testResult}</Banner>
            {/if}
          {/if}
          {#if errors.length}
            <Banner tone="warn" title="Fix before saving">
              <ul>{#each errors as e (e)}<li>{e}</li>{/each}</ul>
            </Banner>
          {/if}
          <FormActions label="Channel actions">
            {#if selected}
              <Button variant="danger" onclick={remove}>Delete</Button>
              <Button onclick={restart}>Restart</Button>
            {/if}
            <Button type="submit" variant="primary" loading={busy} disabled={errors.length > 0}>Save</Button>
          </FormActions>
        {/snippet}

        <SettingsRow label="Name">
          {#snippet children(ctl)}<TextField id={ctl.id} bind:value={form!.name} />{/snippet}
        </SettingsRow>

        <SettingsRow label="Bot token" description="Write-only: a saved token is never shown here.">
          {#snippet children(ctl)}
            <TextField id={ctl.id} aria-describedby={ctl.describedby} type="password" autocomplete="off" bind:value={token}
              placeholder={selected?.hasToken ? "Saved in the keychain (paste to replace)" : "123456:ABC… from @BotFather"} />
            <Button onclick={test} disabled={!token.trim() && !selected?.hasToken}>Test connection</Button>
          {/snippet}
        </SettingsRow>

        <SettingsRow label="Workspace">
          {#snippet children(ctl)}
            <Select id={ctl.id} bind:value={form!.workspaceId} placeholder="Pick a workspace"
              options={app.workspaces.map((w) => ({ value: w.id, label: w.name }))} />
          {/snippet}
        </SettingsRow>

        <SettingsRow label="Mode" description="Manual: reads run, changes wait for approval here. Plan: read-only.">
          {#snippet children(ctl)}
            <Select id={ctl.id} aria-describedby={ctl.describedby} bind:value={form!.mode}
              options={[{ value: "manual", label: "Manual" }, { value: "plan", label: "Plan" }]} />
          {/snippet}
        </SettingsRow>

        <SettingsRow label="Direct messages" description="Pairing: unknown people get a code you approve here. Anyone needs * on the allow-list.">
          {#snippet children(ctl)}
            <Select id={ctl.id} aria-describedby={ctl.describedby} bind:value={form!.dmPolicy}
              options={[
                { value: "pairing", label: "Pairing" },
                { value: "allowlist", label: "Allow-list only" },
                { value: "open", label: "Anyone" },
                { value: "disabled", label: "Off" },
              ]} />
          {/snippet}
        </SettingsRow>

        <SettingsRow label="Groups">
          {#snippet children(ctl)}
            <Select id={ctl.id} bind:value={form!.groupPolicy}
              options={[
                { value: "disabled", label: "Off" },
                { value: "allowlist", label: "Only the group ids below" },
                { value: "open", label: "Any group the bot is in" },
              ]} />
          {/snippet}
        </SettingsRow>
        {#if form.groupPolicy !== "disabled"}
          <SettingsRow label="Group ids">
            {#snippet children(ctl)}<TextField id={ctl.id} bind:value={groupsText} placeholder="-1001234567890, …" />{/snippet}
          </SettingsRow>
          <SettingsRow label="Require a mention" description="Only answer when mentioned or replied to.">
            {#snippet children(ctl)}<Switch id={ctl.id} aria-describedby={ctl.describedby} bind:checked={form!.requireMention} />{/snippet}
          </SettingsRow>
        {/if}

        <SettingsRow label="Approval timeout" description="Seconds to wait for an approval, then deny.">
          {#snippet children(ctl)}
            <TextField id={ctl.id} aria-describedby={ctl.describedby} type="number" min="10" max="3600" bind:value={form!.approvalTimeoutSecs} />
          {/snippet}
        </SettingsRow>

        <SettingsRow stacked label="Tools" description="Tools channel tasks may use. Subagents, scheduling, messaging other sessions and gateway control are always off.">
          {#snippet children(ctl)}
            <div class="tools" role="group" aria-labelledby={ctl.labelId}>
              {#each SELECTABLE_TOOLS as tool (tool.name)}
                <Checkbox label={tool.label} checked={form!.tools.includes(tool.name)}
                  onchange={(on) => form && (form.tools = toggleTool(form.tools, tool.name, on))} />
              {/each}
            </div>
          {/snippet}
        </SettingsRow>

        <SettingsRow label="Enabled">
          {#snippet children(ctl)}<Switch id={ctl.id} bind:checked={form!.enabled} />{/snippet}
        </SettingsRow>
      </SettingsSection>
    </form>

    {#if selected}
      <SettingsSection
        id="pairing"
        title="Pending pairings"
        description={pairings.length === 0
          ? "Unknown people who message the bot appear here (at most 3, each for 1 hour)."
          : "To approve, ask the person for the code the bot sent them and type it here. The code is not shown on this page, so a look-alike name cannot be approved by mistake."}
      >
        {#if pairings.length === 0}
          <EmptyState compact title="No pending pairings" />
        {:else}
          <form onsubmit={approveCode}>
            <SettingsRow label="Pairing code" description="Typed by you, from their chat.">
              {#snippet children(ctl)}
                <TextField id={ctl.id} aria-describedby={ctl.describedby} bind:value={pairCode} autocomplete="off" placeholder="Pairing code from their chat" />
                <Button type="submit" variant="primary" disabled={!pairCode.trim()}>Approve</Button>
              {/snippet}
            </SettingsRow>
          </form>
          <List label="Pending pairing requests" bordered={false}>
            {#each pairings as p (p.code)}
              <ListRow title={p.label || "unknown"} description="id {p.senderId}" meta={expiresIn(p, now)}>
                {#snippet actions()}
                  <Button size="sm" onclick={() => deny(p)} aria-label="Deny {p.label || p.senderId}">Deny</Button>
                {/snippet}
              </ListRow>
            {/each}
          </List>
        {/if}
      </SettingsSection>

      <SettingsSection id="allow" title="Allow-list" description={allow.length === 0 ? "Nobody yet. Approved pairings land here." : undefined}>
        {#if allow.length > 0}
          <List label="Allowed senders" bordered={false}>
            {#each allow as a (a.senderId)}
              <ListRow title={a.senderId} description={a.label || undefined} meta="added by {a.source}">
                {#snippet actions()}
                  <Button size="sm" onclick={() => removeAllowed(a)} aria-label="Remove {a.senderId}">Remove</Button>
                {/snippet}
              </ListRow>
            {/each}
          </List>
        {/if}
        <form onsubmit={addAllowed}>
          <SettingsRow label="Add someone" description="A Telegram user id, or * for anyone.">
            {#snippet children(ctl)}
              <TextField id={ctl.id} aria-describedby={ctl.describedby} bind:value={newSender} placeholder="Telegram user id, or *" />
              <TextField aria-label="Name" bind:value={newLabel} placeholder="Name (optional)" />
              <Button type="submit" variant="primary" disabled={!newSender.trim()}>Add</Button>
            {/snippet}
          </SettingsRow>
        </form>
      </SettingsSection>

      <SettingsSection id="activity" title="Activity">
        {#if activity.length === 0}
          <EmptyState compact title="Nothing yet" />
        {:else}
          <List label="Recent activity" bordered={false}>
            {#each activity as a, i (i)}
              <ListRow title={a.text || a.kind} description="{a.peer} · {a.kind}" meta={a.ts.replace("T", " ").slice(0, 19)}>
                {#snippet leading()}
                  <span role="img" aria-label={a.direction === "in" ? "Received" : a.direction === "out" ? "Sent" : "Event"}>{arrow[a.direction] ?? "·"}</span>
                {/snippet}
              </ListRow>
            {/each}
          </List>
        {/if}
      </SettingsSection>
    {/if}
  {/if}
</SettingsLayout>

<style>
  .steps { margin: 0 0 var(--space-3); padding-left: var(--space-5); display: grid; gap: var(--space-1); }
  ul { margin: 0; padding-left: var(--space-5); }
  .tools { display: grid; gap: var(--space-2); }
</style>
