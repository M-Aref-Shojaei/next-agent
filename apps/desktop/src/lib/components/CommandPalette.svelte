<script lang="ts">
  import { tick } from "svelte";
  import { goto } from "$app/navigation";
  import { Icon, Kbd } from "$lib/ds/index.ts";

  type Action = { label: string; hint: string; href: string; icon: string };
  const actions: Action[] = [
    { label: "Chat", hint: "Open workspace", href: "/", icon: "message" },
    { label: "Code", hint: "Open boards", href: "/boards", icon: "code" },
    { label: "Knowledge", hint: "Open notes", href: "/notes", icon: "book" },
    { label: "Design", hint: "Open design workspace", href: "/design", icon: "sparkles" },
    { label: "Usage", hint: "View usage", href: "/usage", icon: "chart" },
    { label: "Settings", hint: "Open settings", href: "/settings", icon: "settings" },
  ];

  let { onclose }: { onclose: () => void } = $props();
  let query = $state("");
  let selected = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let filtered = $derived(actions.filter((action) => `${action.label} ${action.hint}`.toLowerCase().includes(query.trim().toLowerCase())));

  $effect(() => {
    input?.focus();
  });

  function choose(action: Action | undefined) {
    if (!action) return;
    onclose();
    void goto(action.href);
  }

  function oninput() {
    selected = 0;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onclose();
    } else if (event.key === "ArrowDown") {
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
    void selected;
    void tick().then(() => document.querySelector("[data-command-selected='true']")?.scrollIntoView({ block: "nearest" }));
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" onclick={(event) => event.target === event.currentTarget && onclose()}>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
    <div class="field">
      <Icon name="search" size={16} />
      <input bind:this={input} bind:value={query} oninput={oninput} onkeydown={onkeydown} placeholder="Search commands…" aria-label="Search commands" autocomplete="off" />
      <Kbd keys="Esc" size="sm" />
    </div>
    <ul role="listbox" aria-label="Commands">
      {#each filtered as action, i (action.href)}
        <li role="option" aria-selected={i === selected} data-command-selected={i === selected}>
          <button class:selected={i === selected} onclick={() => choose(action)} onmouseenter={() => (selected = i)}>
            <Icon name={action.icon} size={16} /><span>{action.label}<small>{action.hint}</small></span>
          </button>
        </li>
      {:else}
        <li class="empty">No matching commands</li>
      {/each}
    </ul>
    <footer><span><Kbd keys="Up" size="sm" /><Kbd keys="Down" size="sm" /> select</span><span><Kbd keys="Enter" size="sm" /> open</span></footer>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: var(--z-modal); background: var(--color-scrim); display: flex; justify-content: center; align-items: flex-start; padding: 12vh var(--space-4) 0; }
  .palette { width: min(560px, 100%); background: var(--color-surface-overlay); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-xl); box-shadow: var(--shadow-xl); overflow: hidden; }
  .field { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-3); border-bottom: var(--border-width) solid var(--color-border); }
  input { min-width: 0; flex: 1; border: 0; outline: 0; background: transparent; color: var(--color-text); font: inherit; }
  ul { list-style: none; margin: 0; padding: var(--space-1-5); max-height: 50vh; overflow: auto; }
  li button { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: var(--space-2); align-items: center; padding: var(--space-2); border-radius: var(--radius-md); cursor: pointer; }
  li button.selected { background: var(--color-accent-subtle); }
  li button span { display: grid; gap: var(--space-0-5); }
  small, footer, .empty { color: var(--color-text-secondary); font-size: var(--font-size-xs); }
  .empty { padding: var(--space-3); }
  footer { display: flex; gap: var(--space-4); padding: var(--space-1-5) var(--space-3); border-top: var(--border-width) solid var(--color-border); }
  footer span { display: inline-flex; align-items: center; gap: var(--space-1); }
</style>
