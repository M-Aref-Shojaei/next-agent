<script lang="ts">
  import { app } from "$lib/app.svelte.ts";
  // Reload profiles whenever the workspace changes.
  $effect(() => {
    void app.activeWorkspaceId;
    void app.loadProfiles();
  });
  import RightPanel from "$lib/components/RightPanel.svelte";
  import Timeline from "$lib/components/Timeline.svelte";

  let prompt = $state("");

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const text = prompt;
    prompt = "";
    await app.startTask(text);
  }
  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      (event.currentTarget as HTMLTextAreaElement).form?.requestSubmit();
    }
  }
</script>

<div class="page">
  <section class="chat" aria-label="Chat">
    <Timeline />
    <form class="composer" onsubmit={submit}>
      <label class="sr-only" for="chat">Describe a task</label>
      <textarea
        id="chat"
        data-chat-input
        rows="2"
        placeholder={app.activeWorkspace ? "Describe a task… (Enter to send, Shift+Tab cycles mode)" : "Open a workspace first"}
        disabled={!app.activeWorkspace}
        bind:value={prompt}
        {onkeydown}
      ></textarea>
      {#if app.profiles.length}
        <label class="sr-only" for="profile">Agent profile</label>
        <select id="profile" bind:value={app.profile} title="Agent profile">
          <option value="">Default agent</option>
          {#each app.profiles as p (p.name)}
            <option value={p.name} title={p.description}>{p.name}</option>
          {/each}
        </select>
      {/if}
      <button class="primary" type="submit" disabled={!app.activeWorkspace || !prompt.trim()}>Send</button>
      {#if app.activeTask?.status === "running"}
        <button type="button" onclick={() => app.cancel(app.activeTask!.id)}>Cancel</button>
      {:else if app.activeTask && (app.activeTask.snapshots ?? 0) > 0}
        <button type="button" onclick={() => app.undoStep(app.activeTask!.id)} title="Revert the last file change this task made">Undo last step</button>
      {/if}
    </form>
  </section>
  <RightPanel />
</div>

<style>
  .page { display: grid; grid-template-columns: minmax(0, 1fr) minmax(320px, 40%); flex: 1; min-height: 0; }
  .chat { display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .composer { display: flex; gap: 8px; padding: 10px; border-top: 1px solid var(--border); align-items: flex-end; }
  textarea { resize: none; }
</style>
