<script lang="ts">
  import { onMount } from "svelte";
  import { upstreams } from "$lib/upstreams.svelte.ts";
  import { formatIso, formatUnix, rateLimitNote, reportFor, summary } from "$lib/upstreams.ts";

  onMount(() => void upstreams.init());

  const view = $derived(upstreams.view);
  const limited = $derived(rateLimitNote(view, Math.floor(Date.now() / 1000)));
</script>

<div class="settings">
  <h1>Upstreams</h1>
  <p><a href="/settings">← Back to settings</a></p>
  <p class="muted">
    What the projects we vendor or port from published since the commit pinned in <code>upstream.lock.json</code>.
    Checked once a day while the app runs, through GitHub's public API, with no token and no data about you.
    This page only reports: nothing is downloaded or applied, and bringing a change in stays your decision.
  </p>

  <div class="row">
    <button type="button" onclick={() => upstreams.checkNow()} disabled={view?.checking || !!limited}>
      {view?.checking ? "Checking…" : "Check now"}
    </button>
    <button type="button" onclick={() => upstreams.markSeen()} disabled={!upstreams.unseen}>Mark all as seen</button>
    <span class="muted">Last checked: {formatUnix(view?.lastCheck)}</span>
  </div>
  {#if limited}<p role="status">{limited}</p>{/if}
  {#if upstreams.error}<p role="alert">{upstreams.error}</p>{/if}
  {#if view?.error}<p role="alert">{view.error}</p>{/if}

  {#each view?.watched ?? [] as watched (watched.id)}
    {@const report = reportFor(view, watched.id)}
    <section aria-labelledby="h-{watched.id}">
      <h2 id="h-{watched.id}">
        {watched.id}
        {#if view?.unseen.includes(watched.id)}<span class="badge">new</span>{/if}
      </h2>
      <p>
        <button type="button" class="link" onclick={() => upstreams.open(`https://github.com/${watched.repo}/`)}>{watched.repo}</button>
        · pinned at <code>{watched.commit.slice(0, 7)}</code>{#if report?.pinnedDate} ({formatIso(report.pinnedDate)}){/if}
        {#if report?.branch} · compared with <code>{report.branch}</code>{/if}
      </p>
      <p role="status">{summary(report)}</p>
      {#if report?.error}<p role="alert">Last check failed: {report.error}</p>{/if}

      {#if report && report.releases.length > 0}
        <h3>Releases</h3>
        <ul>
          {#each report.releases as release (release.tag)}
            <li>
              <button type="button" class="link" onclick={() => upstreams.open(release.url)}>{release.name}</button>
              {#if release.prerelease}<span class="muted">(pre-release)</span>{/if}
              <span class="muted">{formatIso(release.date)}</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if report && report.commits.length > 0}
        <h3>Commits</h3>
        {#if report.commits.length < report.aheadBy}
          <p class="muted">Showing the newest {report.commits.length} of {report.aheadBy}.</p>
        {/if}
        <table>
          <thead><tr><th>Commit</th><th>Title</th><th>Date</th><th>Areas</th></tr></thead>
          <tbody>
            {#each report.commits as commit (commit.sha)}
              <tr>
                <td><button type="button" class="link" onclick={() => upstreams.open(commit.url)}><code>{commit.short}</code></button></td>
                <td>{commit.title}</td>
                <td class="nowrap">{formatIso(commit.date)}</td>
                <td>{commit.areas.join(", ")}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      {#if report && report.areas.length > 0}
        <h3>Changed areas</h3>
        <p>
          {#each report.areas.slice(0, 12) as area, i (area.path)}{i > 0 ? " · " : ""}<code>{area.path}</code> ({area.files}){/each}
        </p>
        <p><button type="button" class="link" onclick={() => upstreams.open(report.compareUrl)}>Full comparison on GitHub</button></p>
      {/if}
    </section>
  {:else}
    <p class="muted">No upstream is watched. Add a <code>watch</code> entry to <code>upstream.lock.json</code>.</p>
  {/each}
</div>

<style>
  .settings { padding: 20px 28px; overflow: auto; max-width: 900px; display: grid; gap: 14px; align-content: start; }
  section { border: 1px solid var(--border); border-radius: 8px; padding: 12px 14px; background: var(--panel); }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid var(--border); vertical-align: top; }
  ul { margin: 4px 0; padding-left: 18px; }
  .row { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .badge { margin-left: 8px; padding: 0 6px; border-radius: 8px; background: var(--accent); color: #fff; font-size: 11px; vertical-align: middle; }
  .link { border: none; background: none; padding: 0; color: var(--accent); cursor: pointer; text-decoration: underline; font: inherit; }
  .nowrap { white-space: nowrap; }
</style>
