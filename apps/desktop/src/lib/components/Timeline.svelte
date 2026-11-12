<script lang="ts">
  import { app } from "$lib/app.svelte.ts";
  import { pendingApprovals } from "$lib/timeline.ts";
  import ApprovalCard from "./ApprovalCard.svelte";
  import ToolCard from "./ToolCard.svelte";

  const task = $derived(app.activeTask);
  const pending = $derived(task ? pendingApprovals(task) : []);
  let scroller: HTMLElement | undefined = $state();

  $effect(() => {
    void task?.items.length;
    void pending.length;
    scroller?.scrollTo({ top: scroller.scrollHeight });
  });
</script>

<div class="timeline" bind:this={scroller} role="log" aria-live="polite" aria-label="Task timeline" tabindex="-1">
  {#if !task}
    <p class="muted empty">Open a workspace, then describe a task below. The mock executor will run it.</p>
  {:else}
    <div class="user"><span class="sr-only">You: </span>{task.title}</div>
    {#each task.items as item (item.id)}
      {#if item.kind === "message"}
        <div class="msg {item.role}">{item.text}</div>
      {:else if item.kind === "tool"}
        <ToolCard {item} />
      {:else}
        <div class="notice {item.tone}" role="status">{item.text}</div>
      {/if}
    {/each}
    {#each pending as approval (approval.approvalId)}
      <ApprovalCard item={approval} onrespond={(c) => app.respond(task.id, approval.approvalId, c)} />
    {/each}
  {/if}
</div>

<style>
  .timeline { overflow: auto; padding: 16px; display: flex; flex-direction: column; gap: 10px; flex: 1; }
  .empty { margin: auto; text-align: center; max-width: 360px; }
  .user { align-self: flex-end; background: var(--accent); color: #fff; padding: 6px 12px; border-radius: 12px; max-width: 80%; }
  .msg { max-width: 85%; white-space: pre-wrap; }
  .msg.plan { border-left: 3px solid var(--info); padding-left: 10px; }
  .notice { font-size: 12px; color: var(--muted); }
  .notice.ok { color: var(--ok); } .notice.error { color: var(--danger); }
</style>
