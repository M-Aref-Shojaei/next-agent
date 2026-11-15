<script lang="ts">
  import type { Snippet } from "svelte";
  import type { SettingsNavItem } from "./types.ts";

  let {
    title = "Settings",
    description,
    nav,
    /** Id of the current nav item (aria-current="page"). */
    current,
    /** Above the title: a back link, breadcrumbs. */
    top,
    /** Buttons at the header's end. */
    actions,
    children,
  }: {
    title?: string;
    description?: string;
    nav: readonly SettingsNavItem[];
    current?: string;
    top?: Snippet;
    actions?: Snippet;
    children: Snippet;
  } = $props();
</script>

<div class="ds-settings-frame">
<div class="ds-settings-layout">
  <nav aria-label="{title} sections">
    <ul>
      {#each nav as item (item.id)}
        <li>
          <a href={item.href} aria-current={item.id === current ? "page" : undefined}>
            <span>{item.label}</span>
            {#if item.badge !== undefined}<span class="badge">{item.badge}</span>{/if}
          </a>
        </li>
      {/each}
    </ul>
  </nav>
  <div class="content">
    {#if top}<div class="top">{@render top()}</div>{/if}
    <header>
      <div class="heading">
        <h1>{title}</h1>
        {#if description}<p>{description}</p>{/if}
      </div>
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </header>
    <div class="body">{@render children()}</div>
  </div>
</div>
</div>

<style>
  /* Container queries: the app sidebar takes width, so the viewport is the wrong measure. */
  .ds-settings-frame { container: settings / inline-size; flex: 1; height: 100%; min-height: 0; }
  .ds-settings-layout {
    display: grid;
    grid-template-columns: var(--settings-nav-width) minmax(0, 1fr);
    min-height: 0;
    height: 100%;
    overflow: hidden;
  }
  nav {
    padding: var(--space-5) var(--space-2);
    border-right: var(--border-width) solid var(--color-border);
    background: var(--color-surface-sunken);
    overflow: auto;
  }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-0-5); }
  a {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    height: var(--control-height-md);
    padding: 0 var(--space-2);
    border-radius: var(--radius-md);
    color: var(--color-text-secondary);
    text-decoration: none;
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
  }
  a:hover { background: var(--color-surface-hover); color: var(--color-text); }
  a[aria-current="page"] { background: var(--color-surface-active); color: var(--color-text); }
  a:focus-visible { outline: none; box-shadow: var(--focus-ring-inset); }
  .badge {
    padding: 0 var(--space-1-5);
    border-radius: var(--radius-full);
    background: var(--color-accent);
    color: var(--color-on-accent);
    font-size: var(--font-size-xs);
    line-height: 16px;
  }
  .content { min-width: 0; overflow: auto; padding: var(--space-6) var(--space-8) var(--space-12); }
  .top, header, .body { max-width: var(--content-max-width); }
  .top { margin-bottom: var(--space-3); font-size: var(--font-size-sm); }
  header { display: flex; flex-wrap: wrap; align-items: flex-end; gap: var(--space-3); margin-bottom: var(--space-6); }
  /* The basis makes the actions wrap under the title instead of squeezing it. */
  .heading { flex: 1 1 20rem; min-width: 0; display: grid; gap: var(--space-1); }
  h1 { font-size: var(--font-size-xl); }
  header p { margin: 0; color: var(--color-text-secondary); }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); max-width: 100%; }
  .body { display: grid; gap: var(--space-8); }

  @container settings (max-width: 640px) {
    .ds-settings-layout { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
    nav { padding: var(--space-2); border-right: 0; border-bottom: var(--border-width) solid var(--color-border); }
    ul { display: flex; overflow-x: auto; }
    .content { padding: var(--space-4) var(--space-4) var(--space-10); }
    header { margin-bottom: var(--space-4); }
    .heading { flex-basis: 100%; }
  }
</style>
