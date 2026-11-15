<script lang="ts" generics="V extends string">
  import type { Snippet } from "svelte";
  import { rovingIndex } from "./keyboard.ts";

  interface Tab {
    id: V;
    label: string;
    disabled?: boolean;
    /** Small count or marker after the label. */
    badge?: string | number;
  }

  let {
    tabs,
    value = $bindable(),
    label,
    /** Renders the active panel; receives the active tab id. */
    children,
    onchange,
  }: {
    tabs: readonly Tab[];
    value?: V;
    /** Accessible name of the tab list. */
    label: string;
    children?: Snippet<[V]>;
    onchange?: (id: V) => void;
  } = $props();

  const uid = $props.id();
  let buttons: HTMLButtonElement[] = $state([]);
  const active = $derived(tabs.find((t) => t.id === value && !t.disabled) ?? tabs.find((t) => !t.disabled));

  function select(tab: Tab) {
    if (tab.disabled) return;
    value = tab.id;
    onchange?.(tab.id);
  }

  function onkeydown(event: KeyboardEvent, i: number) {
    const next = rovingIndex(event.key, i, tabs.map((t) => !!t.disabled));
    if (next === null) return;
    event.preventDefault();
    select(tabs[next]);
    buttons[next]?.focus();
  }
</script>

<div class="ds-tabs">
  <div class="list" role="tablist" aria-label={label}>
    {#each tabs as tab, i (tab.id)}
      {@const selected = tab.id === active?.id}
      <button
        bind:this={buttons[i]}
        type="button"
        role="tab"
        id="{uid}-tab-{tab.id}"
        aria-selected={selected}
        aria-controls="{uid}-panel"
        tabindex={selected ? 0 : -1}
        disabled={tab.disabled}
        onclick={() => select(tab)}
        onkeydown={(e) => onkeydown(e, i)}
      >
        {tab.label}
        {#if tab.badge !== undefined}<span class="badge">{tab.badge}</span>{/if}
      </button>
    {/each}
  </div>
  {#if children && active}
    <div class="panel" role="tabpanel" id="{uid}-panel" aria-labelledby="{uid}-tab-{active.id}" tabindex="0">
      {@render children(active.id)}
    </div>
  {/if}
</div>

<style>
  .ds-tabs { display: grid; gap: var(--space-3); min-width: 0; }
  .list {
    display: flex;
    gap: var(--space-1);
    border-bottom: var(--border-width) solid var(--color-border);
    overflow-x: auto;
    scrollbar-width: none;
  }
  button {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1-5);
    height: var(--control-height-lg);
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-md) var(--radius-md) 0 0;
    background: transparent;
    color: var(--color-text-secondary);
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
    cursor: pointer;
  }
  button:hover:not(:disabled) { color: var(--color-text); background: var(--color-surface-hover); }
  button[aria-selected="true"] { color: var(--color-text); }
  button[aria-selected="true"]::after {
    content: "";
    position: absolute;
    left: var(--space-2);
    right: var(--space-2);
    bottom: -1px;
    height: 2px;
    border-radius: var(--radius-full);
    background: var(--color-accent);
  }
  button:focus-visible { outline: none; box-shadow: var(--focus-ring-inset); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .badge {
    min-width: 18px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-full);
    background: var(--color-neutral-subtle);
    color: var(--color-neutral-text);
    font-size: var(--font-size-xs);
    line-height: 16px;
    text-align: center;
  }
  .panel:focus-visible { outline: none; box-shadow: var(--focus-ring); border-radius: var(--radius-md); }
</style>
