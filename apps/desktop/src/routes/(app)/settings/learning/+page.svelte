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
  import {
    Badge, Banner, Button, Dialog, EmptyState, FormActions, List, ListRow, Select, SettingsLayout, SettingsRow,
    SettingsSection, Switch, TextArea, TextField, type SettingsNavItem,
  } from "$lib/ds/index.ts";

  let message = $state("");
  let reviewOn = $state(true);
  let memory = $state<MemoryState | null>(null);
  let drafts = $state({ memory: "", user: "" });
  let skills = $state<SkillInfo[]>([]);
  let viewing = $state<{ name: string; text: string } | null>(null);
  let viewOpen = $state(false);
  let jobs = $state<CronJob[]>([]);
  let form = $state<CronInput | null>(null);
  let skillsText = $state("");

  const ws = $derived(app.activeWorkspaceId);
  const targets = [
    { target: "memory", label: "MEMORY.md: the agent's notes" },
    { target: "user", label: "USER.md: about you" },
  ] as const;
  const nav = $derived<SettingsNavItem[]>([
    { id: "review", label: "Background review", href: "#review" },
    ...(memory ? [{ id: "memory", label: "Memory", href: "#memory" }] : []),
    ...(ws
      ? [
          { id: "skills", label: "Skills", href: "#skills" },
          { id: "jobs", label: "Scheduled jobs", href: "#jobs" },
        ]
      : []),
  ]);

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
      if (action === "view") {
        viewing = { name: skill.name, text: await api.skillRead(ws, skill.name) };
        viewOpen = true;
      }
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


<SettingsLayout
  title="Memory and skills"
  description="Curated memory, the skill library, the background review and scheduled jobs (ported from Hermes Agent, MIT)."
  {nav}
  current="review"
>
  {#snippet top()}<a href="/settings">← Settings</a>{/snippet}

  {#if message}<Banner tone="info" ondismiss={() => (message = "")}>{message}</Banner>{/if}
  {#if !ws}<Banner tone="warn">Open a workspace first: memory, skills and jobs belong to a workspace.</Banner>{/if}

  <SettingsSection id="review" title="Background review">
    <SettingsRow
      label="Review each completed task"
      description="After each completed task, review it and save what is worth keeping to memory and skills. Runs on the Summary model role, at most 16 turns. It can only add memory entries and create or patch skills; every write is audited and shown in the task's timeline."
    >
      {#snippet children(ctl)}
        <Switch id={ctl.id} aria-describedby={ctl.describedby} checked={reviewOn} onchange={toggleReview} />
      {/snippet}
    </SettingsRow>
  </SettingsSection>

  {#if memory}
    <SettingsSection
      id="memory"
      title="Memory"
      description="Shown to the agent when a task starts (as notes, never as instructions). Entries are separated by a line with a single §."
    >
      {#each targets as t (t.target)}
        {@const file = memory[t.target]}
        {@const used = memoryUsage(drafts[t.target]).used}
        <SettingsRow
          stacked
          label={t.label}
          description="{used}/{file.limit} chars ({usagePercent(used, file.limit)}%)"
          error={used > file.limit ? "Over the limit: shorten it to save." : undefined}
        >
          {#snippet children(ctl)}
            <div class="stack">
              <TextArea id={ctl.id} aria-describedby={ctl.describedby} mono rows={6} bind:value={drafts[t.target]} />
              <div><Button variant="primary" onclick={() => saveMemory(t.target)} disabled={used > file.limit}>Save</Button></div>
            </div>
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsSection>
  {/if}

  {#if ws}
    <SettingsSection
      id="skills"
      title="Skills"
      description="Workspace skills live in .next-agent/skills/, user-wide ones in the vault's skills/. Archived skills move to .archive/ and can be restored; nothing is deleted."
    >
      {#snippet actions()}<Button onclick={curate}>Run curator now</Button>{/snippet}
      {#snippet footer()}Archives agent-created skills idle for 30 days (stale after 14); pinned skills are never touched.{/snippet}
      {#if skills.length === 0}
        <EmptyState compact title="No skills yet" description="The agent creates them as it learns." />
      {:else}
        <List label="Skills" bordered={false}>
          {#each skills as skill (skill.root + skill.name)}
            <ListRow
              title={skill.name}
              description={skill.description}
              meta="{skill.useCount}× · {relativeTime(skill.lastActivityAt)}"
              disabled={skill.state === "archived"}
            >
              <Badge>{skill.scope}{skill.category ? ` / ${skill.category}` : ""}</Badge>
              <Badge tone={skill.state === "active" ? "ok" : "neutral"}>{skill.state}</Badge>
              {#if skill.pinned}<Badge tone="accent">Pinned</Badge>{/if}
              {#if skill.createdBy === "agent"}<Badge tone="info">by agent</Badge>{/if}
              {#snippet actions()}
                {#if skill.state === "archived"}
                  <Button size="sm" variant="ghost" onclick={() => skillAction(skill, "restore")}>Restore</Button>
                {:else}
                  <Button size="sm" variant="ghost" onclick={() => skillAction(skill, "view")}>View</Button>
                  <Button size="sm" variant="ghost" onclick={() => skillAction(skill, "pin")}>{skill.pinned ? "Unpin" : "Pin"}</Button>
                  <Button size="sm" variant="ghost" onclick={() => skillAction(skill, "archive")}>Archive</Button>
                {/if}
              {/snippet}
            </ListRow>
          {/each}
        </List>
      {/if}
    </SettingsSection>

    <Dialog bind:open={viewOpen} title={viewing?.name ?? "Skill"} hideConfirm cancelLabel="Close">
      <pre>{viewing?.text}</pre>
    </Dialog>

    <SettingsSection
      id="jobs"
      title="Scheduled jobs"
      description="Each run is its own task and fires only while the app is open. A run missed by more than its catch-up window is skipped."
    >
      {#snippet actions()}
        {#if !form}<Button variant="primary" onclick={() => edit(null)}>Add job</Button>{/if}
      {/snippet}
      {#if jobs.length > 0}
        <List label="Scheduled jobs" bordered={false}>
          {#each jobs as job (job.id)}
            <ListRow
              title={job.name}
              description="{job.scheduleLabel}{job.timezone ? ` (${job.timezone})` : ''} · next: {job.state === 'scheduled' ? relativeTime(job.nextRunAt) : job.state} · last: {job.lastStatus ?? '—'}{job.lastError ? `: ${job.lastError}` : ''}"
              meta="{job.mode}{job.skills.length ? ` · skills: ${job.skills.join(', ')}` : ''}"
            >
              {#snippet actions()}
                <Button size="sm" variant="ghost" onclick={() => jobAction(job, "run")}>Run now</Button>
                <Button size="sm" variant="ghost" onclick={() => edit(job)}>Edit</Button>
                {#if job.state === "paused"}
                  <Button size="sm" variant="ghost" onclick={() => jobAction(job, "resume")}>Resume</Button>
                {:else if job.state === "scheduled"}
                  <Button size="sm" variant="ghost" onclick={() => jobAction(job, "pause")}>Pause</Button>
                {/if}
                <Button size="sm" variant="danger" onclick={() => jobAction(job, "delete")}>Delete</Button>
              {/snippet}
            </ListRow>
          {/each}
        </List>
      {:else if !form}
        <EmptyState compact title="No jobs yet" description="Add a job to run a prompt on a schedule." />
      {/if}
      {#if form}
        <form onsubmit={saveJob}>
          <SettingsRow label="Name">
            {#snippet children(ctl)}<TextField id={ctl.id} bind:value={form!.name} placeholder="(first words of the prompt)" />{/snippet}
          </SettingsRow>
          <SettingsRow stacked label="Prompt">
            {#snippet children(ctl)}<TextArea id={ctl.id} rows={3} bind:value={form!.prompt} />{/snippet}
          </SettingsRow>
          <SettingsRow stacked label="Schedule" description="30m · every monday 9am · 0 9 * * 1-5 · 2026-12-01T09:00">
            {#snippet children(ctl)}<TextField id={ctl.id} aria-describedby={ctl.describedby} bind:value={form!.schedule} />{/snippet}
          </SettingsRow>
          <SettingsRow label="Skills">
            {#snippet children(ctl)}<TextField id={ctl.id} bind:value={skillsText} placeholder="deploy, notes" />{/snippet}
          </SettingsRow>
          <SettingsRow label="Mode">
            {#snippet children(ctl)}
              <Select id={ctl.id} bind:value={form!.mode}
                options={[{ value: "auto", label: "Auto (worktree)" }, { value: "manual", label: "Manual" }, { value: "plan", label: "Plan" }]} />
            {/snippet}
          </SettingsRow>
          <SettingsRow label="Model role">
            {#snippet children(ctl)}<TextField id={ctl.id} bind:value={form!.modelRole} placeholder="chat" />{/snippet}
          </SettingsRow>
          <SettingsRow label="Folder">
            {#snippet children(ctl)}<TextField id={ctl.id} bind:value={form!.workdir} placeholder="(workspace root)" />{/snippet}
          </SettingsRow>
          <SettingsRow label="Time zone">
            {#snippet children(ctl)}<TextField id={ctl.id} bind:value={form!.timezone} placeholder="(system) e.g. Europe/Paris" />{/snippet}
          </SettingsRow>
          <FormActions inset label="Job actions">
            <Button type="button" onclick={() => (form = null)}>Cancel</Button>
            <Button type="submit" variant="primary">Save job</Button>
          </FormActions>
        </form>
      {/if}
    </SettingsSection>
  {/if}
</SettingsLayout>

<style>
  .stack { display: grid; gap: var(--space-2); min-width: 0; }
  pre {
    margin: 0;
    white-space: pre-wrap;
    max-height: 300px;
    overflow: auto;
    padding: var(--space-2);
    border-radius: var(--radius-md);
    background: var(--color-surface-sunken);
    font-family: var(--font-mono);
    font-size: var(--font-size-sm);
  }
</style>
