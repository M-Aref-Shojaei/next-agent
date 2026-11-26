<script lang="ts">
  import { app } from "$lib/app.svelte.ts";
  // Reload profiles whenever the workspace changes.
  $effect(() => {
    void app.activeWorkspaceId;
    void app.loadProfiles();
  });
  import { Button, Select, TextArea } from "$lib/ds/index.ts";
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
      <div class="prompt">
        <TextArea
          label="Describe a task"
          hideLabel
          id="chat"
          data-chat-input
          rows={2}
          resize="none"
          placeholder={app.activeWorkspace ? "Describe a task… (Enter to send, Shift+Tab cycles mode)" : "Open a workspace first"}
          disabled={!app.activeWorkspace}
          bind:value={prompt}
          {onkeydown}
        />
      </div>
      {#if app.profiles.length}
        <Select
          label="Agent profile"
          hideLabel
          id="profile"
          title="Agent profile"
          bind:value={app.profile}
          options={[
            { value: "", label: "Default agent" },
            ...app.profiles.map((p) => ({ value: p.name, label: p.name })),
          ]}
        />
      {/if}
      <Button variant="primary" type="submit" disabled={!app.activeWorkspace || !prompt.trim()}>Send</Button>
      {#if app.activeTask?.status === "running"}
        <Button onclick={() => app.cancel(app.activeTask!.id)}>Cancel</Button>
      {:else if app.activeTask && (app.activeTask.snapshots ?? 0) > 0}
        <Button onclick={() => app.undoStep(app.activeTask!.id)} title="Revert the last file change this task made">Undo last step</Button>
      {/if}
    </form>
  </section>
  <RightPanel />
</div>

<style>
  .page { display: grid; grid-template-columns: minmax(0, 1fr) minmax(320px, 40%); flex: 1; min-height: 0; }
  .chat { display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .composer {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-3);
    border-top: var(--border-width) solid var(--color-border);
    align-items: flex-end;
  }
  .prompt { flex: 1 1 var(--settings-nav-width); min-width: 0; }

  /* Narrow: details panel moves under the chat. */
  @media (max-width: 900px) {
    .page { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(min-content, 1fr) auto; overflow-y: auto; }
    /* Long histories belong in the timeline scrollport. */
    .chat > :global(.timeline) { contain: size; }
    .page > :global(.panel) { border-left: 0; border-top: var(--border-width) solid var(--color-border); max-height: 35vh; }
  }
</style>
