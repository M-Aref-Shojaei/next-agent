<script lang="ts">
  import { app } from "$lib/app.svelte.ts";
  import { pendingApprovals } from "$lib/timeline.ts";
  import { Banner, BrandMark, EmptyState } from "$lib/ds/index.ts";
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
    <div class="empty">
      <EmptyState title="No task yet" description="Open a workspace, then describe a task below. Without a router and model, a scripted demo agent runs it.">
        {#snippet icon()}<BrandMark size={32} />{/snippet}
      </EmptyState>
    </div>
  {:else}
    {#if task.origin}
      <div class="origin" role="note">From {task.origin} · approvals happen here, never in the chat</div>
      <div class="user channel"><span class="sr-only">{task.origin}: </span>{task.title}</div>
    {:else}
      <div class="user"><span class="sr-only">You: </span>{task.title}</div>
    {/if}
    {#each task.items as item (item.id)}
      {#if item.kind === "message"}
        <div class="msg {item.role}">{item.text}</div>
      {:else if item.kind === "tool"}
        <ToolCard {item} />
      {:else}
        <Banner tone={item.tone === "error" ? "danger" : item.tone}>{item.text}</Banner>
      {/if}
    {/each}
    {#each pending as approval (approval.approvalId)}
      <ApprovalCard item={approval} onrespond={(c) => app.respond(task.id, approval.approvalId, c)} />
    {/each}
  {/if}
</div>

<style>
  .timeline { overflow: auto; padding: var(--space-4); display: flex; flex-direction: column; gap: var(--space-3); flex: 1; }
  .empty { margin: auto; max-width: 360px; }
  .user {
    align-self: flex-end;
    background: var(--color-accent);
    color: var(--color-on-accent);
    padding: var(--space-1-5) var(--space-3);
    border-radius: var(--radius-xl);
    max-width: 80%;
    overflow-wrap: anywhere;
  }
  .msg { max-width: 85%; white-space: pre-wrap; overflow-wrap: anywhere; }
  .origin { align-self: flex-end; font-size: var(--font-size-xs); color: var(--color-text-secondary); }
  .user.channel { background: var(--color-surface); color: inherit; border: var(--border-width) solid var(--color-accent); }
  .msg.plan { border-left: 3px solid var(--color-info); padding-left: var(--space-3); }
</style>
