<script lang="ts">
  import type { ProjectEntry } from "$lib/api.ts";
  import { app } from "$lib/app.svelte.ts";
  import { Banner, Button } from "$lib/ds/index.ts";

  const discovery = $derived(app.projectDiscovery);
</script>

{#snippet tree(entries: ProjectEntry[])}
  <ul>
    {#each entries as entry (entry.name)}
      <li>
        {#if entry.children}
          <details>
            <summary>{entry.name}/</summary>
            {@render tree(entry.children)}
          </details>
        {:else}
          {entry.name}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<section class="project" aria-label="Project connections" aria-busy={app.projectDiscoveryLoading}>
  <div class="heading">
    <h2>Project connections</h2>
    <Button variant="ghost" size="sm" aria-label="Refresh project connections" disabled={app.projectDiscoveryLoading} onclick={() => void app.refreshProjectDiscovery()}>Refresh</Button>
  </div>

  {#if app.projectDiscoveryError}
    <Banner tone="danger">{app.projectDiscoveryError}</Banner>
  {/if}
  {#if discovery}
    <dl>
      <dt>Agent instructions</dt>
      <dd>{discovery.agentInstructions ?? "None found"}</dd>
      <dt>Design system</dt>
      <dd>
        {#if discovery.designMd || discovery.tokensCss}
          <a href="/design">Open Design</a>
          {#if discovery.designMd}<span>{discovery.designMd}</span>{/if}
          {#if discovery.tokensCss}<span>{discovery.tokensCss}</span>{/if}
        {:else}
          None found
        {/if}
      </dd>
      <dt>Notes vault</dt>
      <dd>
        {#if discovery.vaultPath}
          <a href="/notes?view=graph">Open vault graph</a>
          <span>{discovery.vaultPath}</span>
        {:else}
          None found
        {/if}
      </dd>
    </dl>

    <details>
      <summary>Project file tree</summary>
      {#if discovery.files.length}
        <div class="tree">{@render tree(discovery.files)}</div>
      {:else}
        <p>No files yet</p>
      {/if}
      {#if discovery.truncated}<p>Showing part of the project tree.</p>{/if}
    </details>
  {:else if app.projectDiscoveryLoading}
    <p role="status">Scanning project…</p>
  {/if}
</section>

<style>
  .project {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-2);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    font-size: var(--font-size-sm);
  }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: var(--space-1); flex-wrap: wrap; }
  h2, dl, dd, p { margin: 0; }
  h2 { font-size: var(--font-size-sm); }
  dt { font-weight: var(--font-weight-medium); }
  dd + dt { margin-top: var(--space-1); }
  dd, p { color: var(--color-text-secondary); overflow-wrap: anywhere; }
  dd span { display: block; }
  a { color: var(--color-accent-text); }
  summary { cursor: pointer; }
  .tree { max-height: calc(var(--space-12) * 4); overflow: auto; }
  ul { list-style: none; margin: var(--space-1) 0; padding-inline-start: var(--space-3); }
  li { overflow-wrap: anywhere; }
</style>
