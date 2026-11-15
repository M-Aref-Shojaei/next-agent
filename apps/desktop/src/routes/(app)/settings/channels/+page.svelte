<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
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

  async function remove() {
    if (!selected) return;
    if (!confirm(`Delete ${selected.config.name}? Its token, pairings, allow-list and activity are removed.`)) return;
    try {
      await api.channelDelete(selected.config.id);
      form = null;
      selectedId = null;
      await loadList();
      message = "Channel deleted.";
    } catch (e) {
      message = String(e);
    }
  }

  async function restart() {
    if (!selectedId) return;
    try {
      await api.channelRestart(selectedId);
      await loadList();
    } catch (e) {
      message = String(e);
    }
  }

  /** Approve the request whose code the owner typed (never one picked from a list). */
  async function approveCode(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedId || !pairCode.trim()) return;
    try {
      const request = await api.channelPairingResolve(selectedId, pairCode.trim(), true);
      pairCode = "";
      message = `Paired ${request.label || request.senderId} (id ${request.senderId}).`;
      await loadDetails();
    } catch (e) {
      message = String(e);
    }
  }

  async function deny(request: PairingRequest) {
    if (!selectedId) return;
    try {
      await api.channelPairingResolve(selectedId, request.code, false);
      message = `Denied ${request.label || request.senderId}.`;
      await loadDetails();
    } catch (e) {
      message = String(e);
    }
  }

  async function addAllowed(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedId || !newSender.trim()) return;
    try {
      await api.channelAllowAdd(selectedId, newSender.trim(), newLabel.trim());
      newSender = "";
      newLabel = "";
      await loadDetails();
    } catch (e) {
      message = String(e);
    }
  }

  async function removeAllowed(entry: AllowEntry) {
    if (!selectedId) return;
    try {
      await api.channelAllowRemove(selectedId, entry.senderId);
      await loadDetails();
    } catch (e) {
      message = String(e);
    }
  }

  onMount(() => {
    const offs: (() => void)[] = [];
    void (async () => {
      try {
        await loadList();
        if (channels[0]) edit(channels[0]);
        offs.push(
          await api.onChannelHealth((id, health) => {
            channels = channels.map((c) => (c.config.id === id ? { ...c, health } : c));
          }),
          await api.onChannelChange((id) => {
            if (id === selectedId) void loadDetails().catch(() => {});
          }),
        );
      } catch (e) {
        message = String(e);
      }
    })();
    const tick = setInterval(() => (now = Math.floor(Date.now() / 1000)), 30_000);
    return () => {
      clearInterval(tick);
      offs.forEach((off) => off());
    };
  });
</script>

<div class="page">
  <h1>Channels</h1>
  <p><a href="/settings">← Settings</a> · <a href="/">Back to chat</a></p>
  <p class="muted">
    Message the agent from a chat app. Each message runs as a task in its workspace, in manual or plan mode only, with a
    tool set you choose (everything else is denied). Approvals happen here or in the overlay, never in the chat; no
    answer in time means deny. Bot tokens are kept in the system keychain. Design from OpenClaw (MIT).
  </p>
  {#if message}<p role="status">{message}</p>{/if}

  <section aria-labelledby="h-list">
    <h2 id="h-list">Your channels</h2>
    {#if channels.length === 0}
      <p class="muted">No channel yet.</p>
    {:else}
      <table>
        <thead><tr><th>Name</th><th>Workspace</th><th>Mode</th><th>Health</th><th></th></tr></thead>
        <tbody>
          {#each channels as c (c.config.id)}
            {@const h = healthLabel(c.health, c.config.enabled)}
            <tr aria-current={c.config.id === selectedId}>
              <td>{c.config.name}</td>
              <td>{app.workspaces.find((w) => w.id === c.config.workspaceId)?.name ?? c.config.workspaceId}</td>
              <td>{c.config.mode}</td>
              <td class={h.tone}>{h.text}{#if c.health?.restarts} · {c.health.restarts} restarts{/if}</td>
              <td><button onclick={() => edit(c)}>Open</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <p><button onclick={add}>Add Telegram</button></p>
  </section>

  {#if form}
    <section aria-labelledby="h-form">
      <h2 id="h-form">{selected ? selected.config.name : "New Telegram bot"}</h2>
      {#if !selected}
        <ol class="muted steps">
          <li>In Telegram, message <strong>@BotFather</strong>, send <code>/newbot</code> and copy the token it gives you.</li>
          <li>Paste it below, pick the workspace and press <em>Test connection</em>, then <em>Save</em>.</li>
          <li>Message your bot: it answers with a pairing code. Approve it under <em>Pending pairings</em>.</li>
        </ol>
      {/if}
      <form onsubmit={save} class="grid">
        <label for="ch-name">Name</label>
        <input id="ch-name" bind:value={form.name} />

        <label for="ch-token">Bot token</label>
        <div class="row">
          <input id="ch-token" type="password" autocomplete="off" bind:value={token}
            placeholder={selected?.hasToken ? "Saved in the keychain (paste to replace)" : "123456:ABC… from @BotFather"} />
          <button type="button" onclick={test} disabled={!token.trim() && !selected?.hasToken}>Test connection</button>
        </div>
        {#if testResult}
          <span></span>
          <p role="status" class={typeof testResult === "string" ? (testResult === "Testing…" ? "" : "bad") : "ok"}>
            {typeof testResult === "string" ? testResult : `Connected to @${testResult.username} (${testResult.name})`}
          </p>
        {/if}

        <label for="ch-ws">Workspace</label>
        <select id="ch-ws" bind:value={form.workspaceId}>
          <option value="" disabled>Pick a workspace</option>
          {#each app.workspaces as ws (ws.id)}<option value={ws.id}>{ws.name}</option>{/each}
        </select>

        <label for="ch-mode">Mode</label>
        <select id="ch-mode" bind:value={form.mode}>
          <option value="manual">Manual: reads run, changes wait for approval here</option>
          <option value="plan">Plan: read-only</option>
        </select>

        <label for="ch-dm">Direct messages</label>
        <select id="ch-dm" bind:value={form.dmPolicy}>
          <option value="pairing">Pairing: unknown people get a code you approve here</option>
          <option value="allowlist">Allow-list only</option>
          <option value="open">Anyone (needs * on the allow-list)</option>
          <option value="disabled">Off</option>
        </select>

        <label for="ch-groups">Groups</label>
        <select id="ch-groups" bind:value={form.groupPolicy}>
          <option value="disabled">Off</option>
          <option value="allowlist">Only the group ids below</option>
          <option value="open">Any group the bot is in</option>
        </select>
        {#if form.groupPolicy !== "disabled"}
          <label for="ch-group-ids">Group ids</label>
          <input id="ch-group-ids" bind:value={groupsText} placeholder="-1001234567890, …" />
          <span></span>
          <label><input type="checkbox" bind:checked={form.requireMention} /> Only answer when mentioned or replied to</label>
        {/if}

        <label for="ch-timeout">Approval timeout</label>
        <div class="row"><input id="ch-timeout" type="number" min="10" max="3600" bind:value={form.approvalTimeoutSecs} /> <span class="muted">seconds, then deny</span></div>

        <span>Tools</span>
        <fieldset class="tools">
          <legend class="sr-only">Tools channel tasks may use</legend>
          {#each SELECTABLE_TOOLS as tool (tool.name)}
            <label>
              <input type="checkbox" checked={form.tools.includes(tool.name)}
                onchange={(e) => form && (form.tools = toggleTool(form.tools, tool.name, e.currentTarget.checked))} />
              {tool.label}
            </label>
          {/each}
          <p class="muted">Subagents, scheduling, messaging other sessions and gateway control are always off.</p>
        </fieldset>

        <span></span>
        <label><input type="checkbox" bind:checked={form.enabled} /> Enabled</label>

        {#if errors.length}
          <span></span>
          <ul class="bad">{#each errors as e (e)}<li>{e}</li>{/each}</ul>
        {/if}
        <span></span>
        <div class="row">
          <button type="submit" disabled={busy || errors.length > 0}>Save</button>
          {#if selected}
            <button type="button" onclick={restart}>Restart</button>
            <button type="button" onclick={remove}>Delete</button>
          {/if}
        </div>
      </form>
    </section>

    {#if selected}
      <section aria-labelledby="h-pairing">
        <h2 id="h-pairing">Pending pairings</h2>
        {#if pairings.length === 0}
          <p class="muted">None. Unknown people who message the bot appear here (at most 3, each for 1 hour).</p>
        {:else}
          <p class="muted">
            To approve, ask the person for the code the bot sent them and type it here. The code is not shown on this
            page, so a look-alike name cannot be approved by mistake.
          </p>
          <form onsubmit={approveCode} class="row add">
            <label class="sr-only" for="pair-code">Pairing code from their chat</label>
            <input id="pair-code" bind:value={pairCode} autocomplete="off" placeholder="Pairing code from their chat" />
            <button type="submit" disabled={!pairCode.trim()}>Approve</button>
          </form>
          <table>
            <thead><tr><th>Who</th><th>Expires</th><th></th></tr></thead>
            <tbody>
              {#each pairings as p (p.code)}
                <tr>
                  <td>{p.label || "unknown"} <span class="muted">id {p.senderId}</span></td>
                  <td>{expiresIn(p, now)}</td>
                  <td><button onclick={() => deny(p)} aria-label="Deny {p.label || p.senderId}">Deny</button></td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>

      <section aria-labelledby="h-allow">
        <h2 id="h-allow">Allow-list</h2>
        {#if allow.length === 0}
          <p class="muted">Nobody yet. Approved pairings land here.</p>
        {:else}
          <table>
            <thead><tr><th>User id</th><th>Name</th><th>Added by</th><th></th></tr></thead>
            <tbody>
              {#each allow as a (a.senderId)}
                <tr>
                  <td><code>{a.senderId}</code></td><td>{a.label}</td><td>{a.source}</td>
                  <td><button onclick={() => removeAllowed(a)} aria-label="Remove {a.senderId}">Remove</button></td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
        <form onsubmit={addAllowed} class="row add">
          <label class="sr-only" for="al-id">User id</label>
          <input id="al-id" bind:value={newSender} placeholder="Telegram user id, or *" />
          <label class="sr-only" for="al-label">Name</label>
          <input id="al-label" bind:value={newLabel} placeholder="Name (optional)" />
          <button type="submit">Add</button>
        </form>
      </section>

      <section aria-labelledby="h-activity">
        <h2 id="h-activity">Activity</h2>
        {#if activity.length === 0}
          <p class="muted">Nothing yet.</p>
        {:else}
          <table class="log">
            <thead><tr><th>When</th><th></th><th>Kind</th><th>Who</th><th>Text</th></tr></thead>
            <tbody>
              {#each activity as a, i (i)}
                <tr>
                  <td>{a.ts.replace("T", " ").slice(0, 19)}</td>
                  <td>{a.direction === "in" ? "←" : a.direction === "out" ? "→" : "·"}</td>
                  <td>{a.kind}</td>
                  <td>{a.peer}</td>
                  <td>{a.text}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  .page { padding: 20px 28px; overflow: auto; flex: 1; min-height: 0; max-width: 900px; display: grid; gap: 14px; align-content: start; }
  input[type="checkbox"] { width: auto; margin: 0 6px 0 0; }
  .tools label, .grid > label:has(input[type="checkbox"]) { display: flex; align-items: center; }
  section { border: 1px solid var(--border); border-radius: 8px; padding: 12px 14px; background: var(--panel); }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid var(--border); vertical-align: top; }
  tr[aria-current="true"] td { background: var(--sidebar); }
  .grid { display: grid; grid-template-columns: minmax(150px, auto) 1fr; gap: 8px; align-items: center; margin-top: 8px; }
  .row { display: flex; gap: 8px; align-items: center; }
  .row input { flex: 1; }
  .add { margin-top: 8px; }
  .tools { border: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  .steps { margin: 0 0 8px; padding-left: 18px; }
  select { font: inherit; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 6px; padding: 5px 6px; }
  .log td:last-child { word-break: break-word; }
  .ok { color: var(--ok); } .bad { color: var(--danger); } .error { color: var(--danger); } .info { color: var(--muted); }
  ul.bad { margin: 0; padding-left: 18px; }
</style>
