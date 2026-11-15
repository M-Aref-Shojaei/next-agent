<script lang="ts">
  import Inline from "./Inline.svelte";
  import Mermaid from "./Mermaid.svelte";
  import QueryBlock from "./QueryBlock.svelte";
  import { parseBody } from "./render.ts";

  let { content, onTask }: { content: string; onTask?: () => void } = $props();
  const body = $derived(parseBody(content));
  const done = $derived(body.marker === "DONE" || body.marker === "CANCELED" || body.marker === "CANCELLED");

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
          {#if ["WAITING", "NOW", "LATER", "IN-PROGRESS"].includes(body.marker)}<span class="marker">{body.marker}</span>{/if}
        {/if}
        {#if i === 0 && body.priority}<span class="prio p{body.priority}">{body.priority}</span>{/if}
        <span class="txt"><Inline tokens={part.inline} /></span>
      </div>
    {:else if part.kind === "property"}
      <div class="prop"><span class="k">{part.key}</span><span class="v"><Inline tokens={part.value} /></span></div>
    {:else if part.kind === "meta"}
      <div class="meta"><span class="k">{part.label.toLowerCase()}</span> {part.value}</div>
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
  .body { line-height: 1.5; overflow-wrap: anywhere; }
  .body.done .line { color: var(--muted); text-decoration: line-through; }
  .h1 .line { font-size: 1.6em; font-weight: 700; }
  .h2 .line { font-size: 1.35em; font-weight: 650; }
  .h3 .line { font-size: 1.15em; font-weight: 600; }
  .line { min-height: 1.5em; }
  .txt { white-space: pre-wrap; }
  .task { all: unset; box-sizing: border-box; display: inline-flex; align-items: center; justify-content: center; width: 15px; height: 15px; margin-right: 6px; vertical-align: -2px; border: 1.5px solid var(--muted); border-radius: 4px; font-size: 11px; line-height: 1; cursor: default; color: #fff; }
  button.task { cursor: pointer; }
  button.task:hover { border-color: var(--accent); }
  button.task:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .task.done { background: var(--ok); border-color: var(--ok); }
  .task.doing, .task.now, .task.in-progress { background: var(--info); border-color: var(--info); }
  .task.canceled, .task.cancelled { background: var(--muted); border-color: var(--muted); }
  .marker { font-size: 10px; letter-spacing: 0.05em; color: var(--muted); border: 1px solid var(--border); border-radius: 4px; padding: 0 4px; margin-right: 6px; }
  .prio { font-size: 11px; font-weight: 700; border-radius: 4px; padding: 0 5px; margin-right: 6px; border: 1px solid currentColor; }
  .pA { color: var(--danger); } .pB { color: var(--warn); } .pC { color: var(--info); }
  .prop, .meta { font-size: 12px; color: var(--muted); }
  .k { background: var(--sidebar); border: 1px solid var(--border); border-radius: 4px; padding: 0 5px; margin-right: 6px; }
  pre { margin: 4px 0; padding: 8px 10px; background: var(--sidebar); border: 1px solid var(--border); border-radius: 6px; overflow: auto; font-size: 12px; }
</style>
