<script lang="ts">
  // Settings → Memory and skills: curated memory, the skill library, the
  // background review and cron jobs (ported from Hermes Agent, MIT).
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
  import {
    cronInputOf,
    emptyCronInput,
    memoryUsage,
    relativeTime,
    sortSkills,
    usagePercent,
    type CronInput,
    type CronJob,
    type MemoryState,
    type SkillInfo,
  } from "$lib/learning.ts";
  import { parseList } from "$lib/gateway.ts";

  let message = $state("");
  let reviewOn = $state(true);
  let memory = $state<MemoryState | null>(null);
  let drafts = $state({ memory: "", user: "" });
  let skills = $state<SkillInfo[]>([]);
  let viewing = $state<{ name: string; text: string } | null>(null);
  let jobs = $state<CronJob[]>([]);
  let form = $state<CronInput | null>(null);
  let skillsText = $state("");

  const ws = $derived(app.activeWorkspaceId);

  async function reloadSkills() {
    if (ws) skills = sortSkills(await api.skillsOverview(ws));
  }

  function fail(e: unknown) {
    message = String(e);
  }

  async function load() {
    reviewOn = (await api.learningSettingsGet()).backgroundReview;
    if (!ws) return;
    memory = await api.memoryGet(ws);
    drafts = { memory: memory.memory.text, user: memory.user.text };
    await reloadSkills();
    jobs = await api.cronJobs(ws);
  }
  onMount(() => void load().catch(fail));

  async function toggleReview(on: boolean) {
    try {
      await api.learningSettingsSet({ backgroundReview: on });
      reviewOn = on;
      message = on ? "Background review on." : "Background review off.";
    } catch (e) {
      fail(e);
    }
  }

  async function saveMemory(target: "memory" | "user") {
    if (!ws) return;
    try {
      memory = await api.memorySave(ws, target, drafts[target]);
      drafts = { memory: memory.memory.text, user: memory.user.text };
      message = `${target === "memory" ? "MEMORY.md" : "USER.md"} saved.`;
    } catch (e) {
      fail(e);
    }
  }

  async function skillAction(skill: SkillInfo, action: "view" | "pin" | "archive" | "restore") {
    if (!ws) return;
    try {
      if (action === "view") viewing = { name: skill.name, text: await api.skillRead(ws, skill.name) };
      if (action === "pin") await api.skillPin(ws, skill.root, skill.name, !skill.pinned);
      if (action === "archive") await api.skillArchive(ws, skill.name);
      if (action === "restore") await api.skillRestore(ws, skill.root, skill.name);
      if (action !== "view") await reloadSkills();
    } catch (e) {
      fail(e);
    }
  }

  async function curate() {
    try {
      const r = await api.curatorRun();
      message = `Curator checked ${r.checked} agent-created skills: ${r.markedStale.length} marked stale, ${r.archived.length} archived, ${r.reactivated.length} reactivated.`;
      await reloadSkills();
    } catch (e) {
      fail(e);
    }
  }

  function edit(job: CronJob | null) {
    if (!ws) return;
    form = job ? cronInputOf(job) : emptyCronInput(ws);
    skillsText = form.skills.join(", ");
  }

  async function saveJob(event: SubmitEvent) {
    event.preventDefault();
    if (!form || !ws) return;
    try {
      await api.cronSave({ ...form, skills: parseList(skillsText) });
      form = null;
      jobs = await api.cronJobs(ws);
      message = "Job saved.";
    } catch (e) {
      fail(e);
    }
  }

  async function jobAction(job: CronJob, action: "pause" | "resume" | "delete" | "run") {
    if (!ws) return;
    try {
      if (action === "pause" || action === "resume") await api.cronPause(job.id, action === "pause");
      if (action === "delete") await api.cronDelete(job.id);
      if (action === "run") message = `Started task ${await api.cronRunNow(job.id)}.`;
      jobs = await api.cronJobs(ws);
    } catch (e) {
      fail(e);
    }
  }
</script>

<div class="settings">
  <h1>Memory and skills</h1>
  <p><a href="/settings">← Settings</a></p>
  {#if message}<p role="status">{message}</p>{/if}
  {#if !ws}<p class="muted">Open a workspace first: memory, skills and jobs belong to a workspace.</p>{/if}

  <section aria-labelledby="h-review">
    <h2 id="h-review">Background review</h2>
    <label>
      <input type="checkbox" checked={reviewOn} onchange={(e) => toggleReview(e.currentTarget.checked)} />
      After each completed task, review it and save what is worth keeping to memory and skills
    </label>
    <p class="muted">Runs on the Summary model role, at most 16 turns. It can only add memory entries and create or patch skills; every write is audited and shown in the task's timeline.</p>
  </section>

  {#if memory}
    <section aria-labelledby="h-memory">
      <h2 id="h-memory">Memory</h2>
      <p class="muted">Shown to the agent when a task starts (as notes, never as instructions). Entries are separated by a line with a single §.</p>
      {#each [["memory", "MEMORY.md — the agent's notes"], ["user", "USER.md — about you"]] as [target, label] (target)}
        {@const file = memory[target as "memory" | "user"]}
        {@const used = memoryUsage(drafts[target as "memory" | "user"]).used}
        <label for="mem-{target}">{label}</label>
        <textarea id="mem-{target}" rows="6" bind:value={drafts[target as "memory" | "user"]}></textarea>
        <div class="row">
          <span class:over={used > file.limit}>{used}/{file.limit} chars ({usagePercent(used, file.limit)}%)</span>
          <button onclick={() => saveMemory(target as "memory" | "user")} disabled={used > file.limit}>Save</button>
        </div>
      {/each}
    </section>
  {/if}

  {#if ws}
    <section aria-labelledby="h-skills">
      <h2 id="h-skills">Skills</h2>
      <p class="muted">Workspace skills live in <code>.next-agent/skills/</code>, user-wide ones in the vault's <code>skills/</code>. Archived skills move to <code>.archive/</code> and can be restored; nothing is deleted.</p>
      {#if skills.length === 0}
        <p class="muted">No skills yet. The agent creates them as it learns.</p>
      {:else}
        <table>
          <thead><tr><th>Skill</th><th>Where</th><th>State</th><th>Used</th><th></th></tr></thead>
          <tbody>
            {#each skills as skill (skill.root + skill.name)}
              <tr class:archived={skill.state === "archived"}>
                <td><strong>{skill.name}</strong>{#if skill.pinned} 📌{/if}<br /><span class="muted">{skill.description}</span></td>
                <td>{skill.scope}{skill.category ? ` / ${skill.category}` : ""}{skill.createdBy === "agent" ? " · by agent" : ""}</td>
                <td>{skill.state}</td>
                <td>{skill.useCount}× · {relativeTime(skill.lastActivityAt)}</td>
                <td class="actions">
                  {#if skill.state === "archived"}
                    <button onclick={() => skillAction(skill, "restore")}>Restore</button>
                  {:else}
                    <button onclick={() => skillAction(skill, "view")}>View</button>
                    <button onclick={() => skillAction(skill, "pin")}>{skill.pinned ? "Unpin" : "Pin"}</button>
                    <button onclick={() => skillAction(skill, "archive")}>Archive</button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <button onclick={curate}>Run curator now</button>
      <span class="muted">Archives agent-created skills idle for 30 days (stale after 14); pinned skills are never touched.</span>
      {#if viewing}
        <h3>{viewing.name}</h3>
        <pre>{viewing.text}</pre>
        <button onclick={() => (viewing = null)}>Close</button>
      {/if}
    </section>

    <section aria-labelledby="h-cron">
      <h2 id="h-cron">Scheduled jobs</h2>
      <p class="muted">Each run is its own task and fires only while the app is open. A run missed by more than its catch-up window is skipped.</p>
      {#if jobs.length > 0}
        <table>
          <thead><tr><th>Job</th><th>Schedule</th><th>Next</th><th>Last</th><th></th></tr></thead>
          <tbody>
            {#each jobs as job (job.id)}
              <tr>
                <td><strong>{job.name}</strong><br /><span class="muted">{job.mode}{job.skills.length ? ` · skills: ${job.skills.join(", ")}` : ""}</span></td>
                <td>{job.scheduleLabel}{job.timezone ? ` (${job.timezone})` : ""}</td>
                <td>{job.state === "scheduled" ? relativeTime(job.nextRunAt) : job.state}</td>
                <td>{job.lastStatus ?? "—"}{job.lastError ? `: ${job.lastError}` : ""}</td>
                <td class="actions">
                  <button onclick={() => jobAction(job, "run")}>Run now</button>
                  <button onclick={() => edit(job)}>Edit</button>
                  {#if job.state === "paused"}
                    <button onclick={() => jobAction(job, "resume")}>Resume</button>
                  {:else if job.state === "scheduled"}
                    <button onclick={() => jobAction(job, "pause")}>Pause</button>
                  {/if}
                  <button onclick={() => jobAction(job, "delete")}>Delete</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      {#if form}
        <form class="job" onsubmit={saveJob}>
          <label for="j-name">Name</label><input id="j-name" bind:value={form.name} placeholder="(first words of the prompt)" />
          <label for="j-prompt">Prompt</label><textarea id="j-prompt" rows="3" bind:value={form.prompt}></textarea>
          <label for="j-sched">Schedule</label><input id="j-sched" bind:value={form.schedule} placeholder="30m · every monday 9am · 0 9 * * 1-5 · 2026-12-01T09:00" />
          <label for="j-skills">Skills</label><input id="j-skills" bind:value={skillsText} placeholder="deploy, notes" />
          <label for="j-mode">Mode</label>
          <select id="j-mode" bind:value={form.mode}>
            <option value="auto">Auto (worktree)</option><option value="manual">Manual</option><option value="plan">Plan</option>
          </select>
          <label for="j-role">Model role</label><input id="j-role" bind:value={form.modelRole} placeholder="chat" />
          <label for="j-dir">Folder</label><input id="j-dir" bind:value={form.workdir} placeholder="(workspace root)" />
          <label for="j-tz">Time zone</label><input id="j-tz" bind:value={form.timezone} placeholder="(system) e.g. Europe/Paris" />
          <div class="row">
            <button type="submit">Save job</button>
            <button type="button" onclick={() => (form = null)}>Cancel</button>
          </div>
        </form>
      {:else}
        <button onclick={() => edit(null)}>Add job</button>
      {/if}
    </section>
  {/if}
</div>

<style>
  .settings { padding: 20px 28px; overflow: auto; max-width: 860px; display: grid; gap: 14px; align-content: start; }
  section { border: 1px solid var(--border); border-radius: 8px; padding: 12px 14px; background: var(--panel); display: grid; gap: 8px; }
  textarea { width: 100%; font-family: var(--mono, monospace); }
  .row { display: flex; gap: 8px; align-items: center; justify-content: space-between; }
  .over { color: var(--danger); }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid var(--border); vertical-align: top; }
  .actions { white-space: nowrap; }
  tr.archived { opacity: 0.6; }
  pre { white-space: pre-wrap; max-height: 300px; overflow: auto; background: var(--sidebar); padding: 8px; border-radius: 6px; }
  .job { display: grid; grid-template-columns: auto 1fr; gap: 6px 10px; align-items: center; }
  .job .row { grid-column: 1 / -1; justify-content: flex-start; }
</style>
