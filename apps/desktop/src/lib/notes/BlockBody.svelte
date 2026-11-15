<script lang="ts">
  import { Badge } from "$lib/ds/index.ts";
  import Inline from "./Inline.svelte";
  import Mermaid from "./Mermaid.svelte";
  import QueryBlock from "./QueryBlock.svelte";
  import { parseBody } from "./render.ts";

  let { content, onTask }: { content: string; onTask?: () => void } = $props();
  const body = $derived(parseBody(content));
  const done = $derived(body.marker === "DONE" || body.marker === "CANCELED" || body.marker === "CANCELLED");

  const PRIORITY_TONE = { A: "danger", B: "warn", C: "info" } as const;
  const LABEL: Record<string, string> = {
    TODO: "To do", DOING: "Doing", DONE: "Done", NOW: "Now", LATER: "Later", WAITING: "Waiting",
    CANCELED: "Canceled", CANCELLED: "Canceled", "IN-PROGRESS": "In progress",
  };
</script>

<div class="body" class:done class:h1={body.heading === 1} class:h2={body.heading === 2} class:h3={body.heading !== null && body.heading >= 3}>
  {#each body.parts as part, i (i)}
    {#if part.kind === "line"}
      <div class="line">
        {#if i === 0 && body.marker}
          {#if onTask}
            <button class="task {body.marker.toLowerCase()}" onclick={(e) => { e.stopPropagation(); onTask(); }} aria-label={`${LABEL[body.marker]}: click to change`} title="Click to change state">
              {#if body.marker === "DONE"}✓{:else if body.marker === "DOING" || body.marker === "NOW" || body.marker === "IN-PROGRESS"}▸{:else if body.marker === "CANCELED" || body.marker === "CANCELLED"}×{/if}
            </button>
          {:else}
            <span class="task {body.marker.toLowerCase()}" aria-label={LABEL[body.marker]}></span>
          {/if}
          {#if ["WAITING", "NOW", "LATER", "IN-PROGRESS"].includes(body.marker)}<span class="chip"><Badge size="sm">{body.marker}</Badge></span>{/if}
        {/if}
        {#if i === 0 && body.priority}<span class="chip"><Badge size="sm" variant="outline" tone={PRIORITY_TONE[body.priority as keyof typeof PRIORITY_TONE] ?? "neutral"}>{body.priority}</Badge></span>{/if}
        <span class="txt"><Inline tokens={part.inline} /></span>
      </div>
    {:else if part.kind === "property"}
      <div class="prop"><span class="k"><Badge size="sm">{part.key}</Badge></span><span class="v"><Inline tokens={part.value} /></span></div>
    {:else if part.kind === "meta"}
      <div class="meta"><span class="k"><Badge size="sm">{part.label.toLowerCase()}</Badge></span> {part.value}</div>
    {:else if part.kind === "code"}
      {#if part.lang === "mermaid"}
        <Mermaid code={part.code} />
      {:else}
        <pre><code>{part.code}</code></pre>
      {/if}
    {:else if part.kind === "query"}
      <QueryBlock text={part.text} />
    {/if}
  {/each}
</div>

<style>
  .body { line-height: var(--line-height-normal); overflow-wrap: anywhere; }
  .body.done .line { color: var(--color-text-secondary); text-decoration: line-through; }
  .h1 .line { font-size: 1.6em; font-weight: var(--font-weight-bold); }
  .h2 .line { font-size: 1.35em; font-weight: var(--font-weight-semibold); }
  .h3 .line { font-size: 1.15em; font-weight: var(--font-weight-semibold); }
  .line { min-height: 1.5em; }
  .txt { white-space: pre-wrap; }
  .task { all: unset; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; width: var(--space-4); height: var(--space-4); margin-right: var(--space-1-5); vertical-align: -2px; border: var(--border-width-strong) solid var(--color-text-secondary); border-radius: var(--radius-sm); font-size: var(--font-size-xs); line-height: 1; cursor: default; color: var(--color-text-inverse); }
  button.task { cursor: pointer; }
  button.task:hover { border-color: var(--color-accent); }
  button.task:focus-visible { outline: var(--border-width-strong) solid var(--color-focus); outline-offset: 1px; }
  .task.done { background: var(--color-ok); border-color: var(--color-ok); color: var(--color-on-ok); }
  .task.doing, .task.now, .task.in-progress { background: var(--color-info); border-color: var(--color-info); color: var(--color-on-info); }
  .task.canceled, .task.cancelled { background: var(--color-text-secondary); border-color: var(--color-text-secondary); }
  .chip { margin-right: var(--space-1-5); }
  .prop, .meta { font-size: var(--font-size-sm); color: var(--color-text-secondary); }
  .k { margin-right: var(--space-1-5); }
  pre { margin: var(--space-1) 0; padding: var(--space-2) var(--space-3); background: var(--color-surface-sunken); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-md); overflow: auto; font-size: var(--font-size-sm); }
</style>
