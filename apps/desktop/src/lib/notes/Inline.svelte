<script lang="ts">
  import type { Inline } from "./render.ts";
  import BlockRef from "./BlockRef.svelte";
  import { useNotes } from "./ctx.ts";

  let { tokens }: { tokens: Inline[] } = $props();
  const ctx = useNotes();

  function go(event: Event, name: string) {
    event.preventDefault();
    event.stopPropagation();
    ctx.open(name);
  }
</script>

{#each tokens as t, i (i)}
  {#if t.kind === "text"}{t.text}
  {:else if t.kind === "link"}<a class="ref" href={`/notes?page=${encodeURIComponent(t.name)}`} onclick={(e) => go(e, t.name)}><span class="br">[[</span>{t.label}<span class="br">]]</span></a>
  {:else if t.kind === "tag"}<a class="ref tag" href={`/notes?page=${encodeURIComponent(t.name)}`} onclick={(e) => go(e, t.name)}>#{t.name}</a>
  {:else if t.kind === "blockref"}<BlockRef uuid={t.uuid} />
  {:else if t.kind === "code"}<code>{t.text}</code>
  {:else if t.kind === "bold"}<strong>{t.text}</strong>
  {:else if t.kind === "italic"}<em>{t.text}</em>
  {:else if t.kind === "url"}<span class="url" title={t.href}>{t.label}</span>
  {/if}
{/each}

<style>
  .ref { color: var(--color-accent-text); text-decoration: none; }
  .ref:hover { text-decoration: underline; }
  .br { opacity: 0.45; }
  .tag { background: var(--color-accent-subtle); border-radius: var(--radius-sm); padding: 0 var(--space-1); }
  code { background: var(--color-surface-sunken); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-sm); padding: 0 var(--space-1); }
  .url { color: var(--color-accent-text); text-decoration: underline dotted; cursor: default; }
</style>
