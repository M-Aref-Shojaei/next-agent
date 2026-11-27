<script lang="ts">
  import { page } from "$app/state";
  import { Icon } from "$lib/ds/index.ts";

  const ITEMS = [
    { href: "/", label: "Chat", icon: "more" },
    { href: "/settings/overlay/agents", label: "Agents", icon: "settings" },
    { href: "/notes", label: "Knowledge", icon: "search" },
    { href: "/design", label: "Design", icon: "edit" },
    { href: "/usage", label: "Monitor", icon: "info" },
  ] as const;

  function active(href: string) {
    return page.url.pathname === href || page.url.pathname.startsWith(`${href}/`);
  }
</script>

<nav class="rail" aria-label="Activity modes">
  {#each ITEMS as item (item.label)}
    {@const current = active(item.href)}
    <a class:active={current} href={item.href} aria-current={current ? "page" : undefined}>
      <Icon name={item.icon} size={18} />
      <span>{item.label}</span>
    </a>
  {/each}
</nav>

<style>
  .rail {
    width: var(--shell-activity-rail-width);
    min-width: var(--shell-activity-rail-width);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-1);
    background: var(--color-surface);
    border-right: var(--border-width) solid var(--color-border);
  }
  a {
    position: relative;
    display: flex;
    width: var(--space-12);
    min-height: var(--shell-titlebar-height);
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-0-5);
    color: var(--color-text-secondary);
    border-radius: var(--radius-sm);
    text-decoration: none;
    font-size: calc(var(--font-size-xs) - 1px);
    font-weight: var(--font-weight-medium);
  }
  a:hover { color: var(--color-text); background: var(--color-surface-sunken); }
  a:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  a.active { color: var(--color-accent); background: var(--color-surface-sunken); }
  a.active::before {
    content: "";
    position: absolute;
    left: calc(var(--space-1) * -1);
    top: var(--space-2);
    bottom: var(--space-2);
    width: calc(var(--space-0-5) + var(--border-width));
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    background: var(--color-accent);
  }
  @media (max-width: 640px) {
    .rail {
      width: 100%;
      min-width: 0;
      flex-direction: row;
      justify-content: space-around;
      border-right: 0;
      border-bottom: var(--border-width) solid var(--color-border);
      overflow-x: auto;
    }
    a { flex: 0 0 var(--space-12); }
    a.active::before { left: var(--space-2); right: var(--space-2); top: auto; bottom: 0; width: auto; height: calc(var(--space-0-5) + var(--border-width)); }
  }
</style>
