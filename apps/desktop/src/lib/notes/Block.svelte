<script lang="ts">
  import { tick } from "svelte";
  import { Icon, IconButton } from "$lib/ds/index.ts";
  import Self from "./Block.svelte";
  import BlockBody from "./BlockBody.svelte";
  import { completeLink, editableText, isCollapsed, linkContext, markerOf } from "./content.ts";
  import { useNotes } from "./ctx.ts";
  import { caretLines, editorAction } from "./keymap.ts";
  import type { Outliner } from "./outliner.svelte.ts";
  import type { OBlock } from "./outline.ts";

  let { block, outliner }: { block: OBlock; outliner: Outliner } = $props();
  const ctx = useNotes();

  const collapsed = $derived(isCollapsed(block.content));
  const editing = $derived(outliner.editing?.id === block.id ? outliner.editing : null);
  const hasChildren = $derived(block.children.length > 0);

  let area = $state<HTMLTextAreaElement | null>(null);
  let popup = $state<{ start: number; query: string } | null>(null);
  let selected = $state(0);

  const suggestions = $derived.by(() => {
    if (!popup) return [] as { name: string; create: boolean }[];
    const q = popup.query.trim().toLowerCase();
    const names = ctx.pageNames();
    const hits = names
      .filter((n) => n.toLowerCase().includes(q))
      .sort((a, b) => Number(b.toLowerCase().startsWith(q)) - Number(a.toLowerCase().startsWith(q)) || a.length - b.length)
      .slice(0, 8)
      .map((name) => ({ name, create: false }));
    if (q && !names.some((n) => n.toLowerCase() === q)) hits.push({ name: popup.query.trim(), create: true });
    return hits;
  });

  // Focus (and place the caret) whenever the outliner asks for this block.
  $effect(() => {
    const request = editing;
    if (!request) return;
    void tick().then(() => {
      if (!area) return;
      area.value = editableText(block.content);
      resize();
      area.focus();
      const at = request.caret === "end" ? area.value.length : Math.min(request.caret, area.value.length);
      area.setSelectionRange(at, at);
      updatePopup();
    });
  });

  // The text changed from outside the editor (undo, merge): show it.
  $effect(() => {
    const text = editableText(block.content);
    if (area && area.value !== text) {
      area.value = text;
      resize();
    }
  });

  function resize() {
    if (!area) return;
    area.style.height = "auto";
    area.style.height = `${area.scrollHeight}px`;
  }

  function updatePopup() {
    if (!area) return;
    const c = linkContext(area.value, area.selectionStart);
    if (c?.query !== popup?.query) selected = 0;
    popup = c;
  }

  function accept(index: number) {
    const choice = suggestions[index];
    if (!area || !popup || !choice) return;
    const out = completeLink(area.value, area.selectionStart, popup, choice.name);
    popup = null;
    outliner.replaceText(block.id, out.text, out.caret);
  }

  function onkeydown(event: KeyboardEvent) {
    if (!area) return;
    const action = editorAction(event, {
      selectionStart: area.selectionStart,
      selectionEnd: area.selectionEnd,
      ...caretLines(area.value, area.selectionStart),
      popupOpen: popup !== null && suggestions.length > 0,
    });
    if (!action) return;
    event.preventDefault();
    if (action === "popupAccept") accept(selected);
    else if (action === "popupNext") selected = (selected + 1) % suggestions.length;
    else if (action === "popupPrevious") selected = (selected + suggestions.length - 1) % suggestions.length;
    else if (action === "popupClose") popup = null;
    else outliner.act(action, block.id, area.selectionStart);
  }

  function oninput() {
    if (!area) return;
    resize();
    outliner.input(block.id, area.value);
    updatePopup();
  }

  // Moving a block in the DOM (Alt+Up/Down) blurs its editor for a moment; the focus effect
  // takes it back right after, so only a blur that stays counts as leaving the block.
  function onblur() {
    const request = editing;
    setTimeout(() => {
      const now = document.activeElement;
      if (now instanceof HTMLTextAreaElement && now.dataset.block === block.id) return;
      popup = null;
      if (request) outliner.blur(block.id, request.nonce);
    }, 0);
  }

  function startEditing(event: Event) {
    if ((event.target as HTMLElement).closest("a, button, .query")) return;
    outliner.focus(block.id, "end");
  }
</script>

<li class="block">
  <div class="row">
    <span class="twisty" class:invisible={!hasChildren}>
      <IconButton label={collapsed ? "Expand" : "Collapse"} size="sm" aria-expanded={!collapsed} tabindex={-1} onclick={() => outliner.toggleCollapsed(block.id)}>
        <span class="chev" class:open={!collapsed}><Icon name="chevron-right" size={14} /></span>
      </IconButton>
    </span>
    <span class="bullet" class:folded={collapsed && hasChildren} aria-hidden="true"></span>
    <div class="content">
      {#if editing}
        <textarea
          bind:this={area}
          class="editor"
          data-block={block.id}
          rows="1"
          spellcheck="false"
          aria-label="Block text"
          {onkeydown}
          {oninput}
          onclick={updatePopup}
          onkeyup={(e) => (e.key.startsWith("Arrow") ? updatePopup() : undefined)}
          onblur={onblur}
        ></textarea>
        {#if popup && suggestions.length}
          <ul class="popup" role="listbox" aria-label="Pages">
            {#each suggestions as s, i (s.name + s.create)}
              <li role="option" aria-selected={i === selected}>
                <button class:on={i === selected} onmousedown={(e) => { e.preventDefault(); accept(i); }}>
                  {#if s.create}Create page “{s.name}”{:else}{s.name}{/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div class="view" role="group" tabindex="0" aria-label="Block" onclick={startEditing} onkeydown={(e) => { if (e.key === "Enter" && e.target === e.currentTarget) { e.preventDefault(); outliner.focus(block.id, "end"); } }}>
          <BlockBody content={block.content} onTask={markerOf(block.content) ? () => outliner.advanceTask(block.id) : undefined} />
        </div>
      {/if}
    </div>
  </div>
  {#if hasChildren && !collapsed}
    <ul class="children">
      {#each block.children as child (child.id)}
        <Self block={child} {outliner} />
      {/each}
    </ul>
  {/if}
</li>

<style>
  .block { list-style: none; }
  .row { display: flex; align-items: flex-start; gap: var(--space-0-5); }
  .twisty { flex: none; display: flex; opacity: 0; }
  .twisty :global(button) { width: var(--space-4); height: var(--space-5); padding: 0; }
  .row:hover .twisty, .twisty:focus-within { opacity: 1; }
  .twisty.invisible { visibility: hidden; }
  .chev { display: inline-flex; transition: transform var(--duration-fast) var(--ease-standard); }
  .chev.open { transform: rotate(90deg); }
  .bullet { flex: none; width: var(--space-1-5); height: var(--space-1-5); border-radius: var(--radius-full); background: var(--color-text-secondary); margin: var(--space-2) var(--space-2) 0 var(--space-0-5); }
  .bullet.folded { box-shadow: 0 0 0 var(--space-0-5) var(--color-border-strong); }
  .content { flex: 1; min-width: 0; position: relative; }
  .view { min-height: var(--space-5); border-radius: var(--radius-sm); cursor: text; }
  .view:focus-visible { outline: var(--border-width-strong) solid var(--color-focus); outline-offset: 1px; }
  .editor { display: block; width: 100%; resize: none; overflow: hidden; border: var(--border-width) solid var(--color-accent); border-radius: var(--radius-sm); padding: 0 var(--space-1); margin: 0; line-height: var(--line-height-normal); background: var(--color-surface); font: inherit; }
  .editor:focus-visible { outline: none; }
  .children { margin: 0 0 0 var(--space-2); padding: 0 0 0 var(--space-5); border-left: var(--border-width) solid var(--color-border); }
  .popup { position: absolute; z-index: var(--z-dropdown); left: 0; top: 100%; margin: var(--space-0-5) 0 0; padding: var(--space-1); list-style: none; min-width: 220px; max-width: 420px; background: var(--color-surface-raised); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); }
  .popup button { all: unset; display: block; width: 100%; box-sizing: border-box; padding: var(--space-1) var(--space-2); border-radius: var(--radius-md); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .popup button.on, .popup button:hover { background: var(--color-accent-subtle); }
</style>
