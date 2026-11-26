<script lang="ts">
  import { tick } from "svelte";
  import { goto } from "$app/navigation";
  import { Icon, Kbd, type IconName } from "$lib/ds/index.ts";

  type Action = { label: string; hint: string; href: string; icon: IconName };
  const actions: Action[] = [
    { label: "Chat", hint: "Open workspace", href: "/", icon: "more" },
    { label: "Boards", hint: "Open design boards", href: "/boards", icon: "edit" },
    { label: "Knowledge", hint: "Open notes", href: "/notes", icon: "search" },
    { label: "Design", hint: "Open design workspace", href: "/design", icon: "edit" },
    { label: "Usage", hint: "View usage", href: "/usage", icon: "info" },
    { label: "Settings", hint: "Open settings", href: "/settings", icon: "settings" },
  ];

  let { onclose }: { onclose: () => void } = $props();
  const listId = $props.id();
  let query = $state("");
  let selected = $state(0);
  let dialog = $state<HTMLDialogElement>();
  let input = $state<HTMLInputElement | null>(null);
  let filtered = $derived(actions.filter((action) => `${action.label} ${action.hint}`.toLowerCase().includes(query.trim().toLowerCase())));

  $effect(() => {
    if (!dialog || dialog.open) return;
    dialog.showModal();
    input?.focus();
  });

  function dismiss() {
    dialog?.close();
    onclose();
  }

  function choose(action: Action | undefined) {
    if (!action) return;
    dismiss();
    void goto(action.href);
  }

  function oninput() {
    selected = 0;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      selected = filtered.length ? (selected + 1) % filtered.length : 0;
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      selected = filtered.length ? (selected + filtered.length - 1) % filtered.length : 0;
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(filtered[selected]);
    }
  }

  $effect(() => {
    const activeId = `${listId}-${selected}`;
    void tick().then(() => document.getElementById(activeId)?.scrollIntoView({ block: "nearest" }));
  });
</script>

<dialog
  bind:this={dialog}
  class="palette"
  aria-label="Command palette"
  onclick={(event) => { if (event.target === event.currentTarget) dismiss(); }}
  oncancel={(event) => { event.preventDefault(); dismiss(); }}
>
    <div class="field">
      <Icon name="search" size={16} />
      <input bind:this={input} bind:value={query} oninput={oninput} onkeydown={onkeydown} placeholder="Search commands…" aria-label="Search commands" autocomplete="off" role="combobox" aria-autocomplete="list" aria-expanded="true" aria-controls={listId} aria-activedescendant={filtered[selected] ? `${listId}-${selected}` : undefined} />
      <Kbd keys="Esc" size="sm" />
    </div>
    <div id={listId} class="commands" role="listbox" aria-label="Commands">
      {#each filtered as action, i (action.href)}
        <button id={`${listId}-${i}`} type="button" role="option" aria-selected={i === selected} tabindex="-1" class:selected={i === selected} onclick={() => choose(action)} onmouseenter={() => (selected = i)}>
          <Icon name={action.icon} size={16} /><span>{action.label}<small>{action.hint}</small></span>
        </button>
      {/each}
    </div>
    {#if filtered.length === 0}<p class="empty" role="status">No matching commands</p>{/if}
    <footer><span><Kbd keys="Up" size="sm" /><Kbd keys="Down" size="sm" /> select</span><span><Kbd keys="Enter" size="sm" /> open</span></footer>
</dialog>

<style>
  .palette { width: min(560px, calc(100% - 2 * var(--space-4))); margin: 12vh auto 0; padding: 0; background: var(--color-surface-overlay); color: var(--color-text); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-xl); box-shadow: var(--shadow-xl); overflow: hidden; }
  .palette::backdrop { background: var(--color-scrim); }
  .field { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-3); border-bottom: var(--border-width) solid var(--color-border); }
  input { min-width: 0; flex: 1; border: 0; outline: 0; background: transparent; color: var(--color-text); font: inherit; }
  .commands { padding: var(--space-1-5); max-height: 50vh; overflow: auto; }
  .commands button { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: var(--space-2); align-items: center; padding: var(--space-2); border-radius: var(--radius-md); cursor: pointer; }
  .commands button.selected { background: var(--color-accent-subtle); }
  .commands button span { display: grid; gap: var(--space-0-5); }
  small, footer, .empty { color: var(--color-text-secondary); font-size: var(--font-size-xs); }
  .empty { margin: 0; padding: var(--space-3); }
  footer { display: flex; gap: var(--space-4); padding: var(--space-1-5) var(--space-3); border-top: var(--border-width) solid var(--color-border); }
  footer span { display: inline-flex; align-items: center; gap: var(--space-1); }
</style>
