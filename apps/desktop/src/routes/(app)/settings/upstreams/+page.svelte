<script lang="ts">
  import { onMount } from "svelte";
  import { Badge, Banner, Button, SettingsLayout, SettingsRow, SettingsSection, Table, type SettingsNavItem, type TableColumn } from "$lib/ds/index.ts";
  import { upstreams } from "$lib/upstreams.svelte.ts";
  import type { UpstreamCommit } from "$lib/upstreams.ts";
  import { formatIso, formatUnix, rateLimitNote, reportFor, summary } from "$lib/upstreams.ts";

  // The clock moves on its own, so a rate limit's note goes when it ends.
  let now = $state(Math.floor(Date.now() / 1000));
  onMount(() => {
    void upstreams.init();
    const tick = setInterval(() => (now = Math.floor(Date.now() / 1000)), 15_000);
    return () => clearInterval(tick);
  });

  const commitColumns: TableColumn<UpstreamCommit>[] = [
    { key: "commit", label: "Commit", value: (c) => c.short },
    { key: "title", label: "Title", value: (c) => c.title },
    { key: "date", label: "Date", nowrap: true, value: (c) => formatIso(c.date), sortValue: (c) => c.date },
    { key: "areas", label: "Areas", value: (c) => c.areas.join(", ") },
  ];

  const view = $derived(upstreams.view);
  const limited = $derived(rateLimitNote(view, now));
  const nav = $derived<SettingsNavItem[]>([
    { id: "status", label: "Status", href: "#status" },
    ...(view?.watched ?? []).map((w) => ({
      id: w.id, label: w.id, href: `#h-${w.id}`, badge: view?.unseen.includes(w.id) ? "new" : undefined,
    })),
  ]);
</script>

<SettingsLayout
  title="Upstreams"
  description="What the projects we vendor or port from published since the commit pinned in upstream.lock.json. Checked once a day while the app runs, through GitHub's public API, with no token and no data about you. This page only reports: nothing is downloaded or applied, and bringing a change in stays your decision."
  {nav}
>
  {#snippet top()}<a href="/settings">← Back to settings</a>{/snippet}

  {#if limited}<Banner tone="info">{limited}</Banner>{/if}
  {#if upstreams.error}<Banner tone="danger">{upstreams.error}</Banner>{/if}
  {#if view?.error}<Banner tone="danger">{view.error}</Banner>{/if}

  <SettingsSection id="status" title="Status">
    {#snippet actions()}
      <Button onclick={() => upstreams.markSeen()} disabled={!upstreams.unseen}>Mark all as seen</Button>
      <Button variant="primary" onclick={() => upstreams.checkNow()} disabled={view?.checking || !!limited} loading={view?.checking}>
        {view?.checking ? "Checking…" : "Check now"}
      </Button>
    {/snippet}
    <SettingsRow label="Last checked">
      {#snippet children()}<span class="muted">{formatUnix(view?.lastCheck)}</span>{/snippet}
    </SettingsRow>
  </SettingsSection>

  {#each view?.watched ?? [] as watched (watched.id)}
    {@const report = reportFor(view, watched.id)}
    <SettingsSection id="h-{watched.id}" title={watched.id}>
      {#snippet actions()}
        {#if view?.unseen.includes(watched.id)}<Badge tone="accent" variant="solid">new</Badge>{/if}
      {/snippet}
      <div class="block">
        <p>
          <Button variant="ghost" size="sm" onclick={() => upstreams.open(`https://github.com/${watched.repo}/`)}>{watched.repo}</Button>
          pinned at <code>{watched.commit.slice(0, 7)}</code>{#if report?.pinnedDate} ({formatIso(report.pinnedDate)}){/if}
          {#if report?.branch} · compared with <code>{report.branch}</code>{/if}
        </p>
        <p role="status">{summary(report)}</p>
        {#if report?.error}<p role="alert" class="bad">Last check failed: {report.error}</p>{/if}
      </div>

      {#if report && report.releases.length > 0}
        <div class="block">
          <h3>Releases</h3>
          <ul>
            {#each report.releases as release (release.tag)}
              <li>
                <Button variant="ghost" size="sm" onclick={() => upstreams.open(release.url)}>{release.name}</Button>
                {#if release.prerelease}<span class="muted">(pre-release)</span>{/if}
                <span class="muted">{formatIso(release.date)}</span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if report && report.commits.length > 0}
        <div class="block">
          <h3>Commits</h3>
          {#if report.commits.length < report.aheadBy}
            <p class="muted">Showing the newest {report.commits.length} of {report.aheadBy}.</p>
          {/if}
          <Table label="Commits" columns={commitColumns} rows={report.commits} rowKey={(c) => c.sha}>
            {#snippet cell(commit, col)}
              {#if col.key === "commit"}<Button variant="ghost" size="sm" onclick={() => upstreams.open(commit.url)}><code>{commit.short}</code></Button>
              {:else if col.key === "date"}{formatIso(commit.date)}
              {:else}{col.value?.(commit)}{/if}
            {/snippet}
          </Table>
        </div>
      {/if}

      {#if report && report.areas.length > 0}
        <div class="block">
          <h3>Changed areas</h3>
          <p>
            {#each report.areas.slice(0, 12) as area, i (area.path)}{i > 0 ? " · " : ""}<code>{area.path}</code> ({area.files}){/each}
          </p>
          <p><Button variant="ghost" size="sm" onclick={() => upstreams.open(report.compareUrl)}>Full comparison on GitHub</Button></p>
        </div>
      {/if}
    </SettingsSection>
  {:else}
    <SettingsSection title="Nothing watched">
      <p class="block">No upstream is watched. Add a <code>watch</code> entry to <code>upstream.lock.json</code>.</p>
    </SettingsSection>
  {/each}
</SettingsLayout>

<style>
  .block { display: grid; gap: var(--space-2); padding: var(--space-3) var(--space-4); }
  .block p { margin: 0; }
  h3 { font-size: var(--font-size-md); }
  ul { margin: 0; padding: 0; list-style: none; display: grid; gap: var(--space-1); }
  .muted { color: var(--color-text-tertiary); }
  .bad { color: var(--color-danger-text); }
  code { font-family: var(--font-mono); font-size: var(--font-size-xs); }
</style>
