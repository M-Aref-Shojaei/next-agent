<script lang="ts" module>
  /**
   * One reviewed change to an agent's config (~/.claude/settings.json, an
   * agent's hooks file): the exact diff and where the backup goes, written
   * only after the user confirms, and only for the fingerprint of that diff.
   */
  export interface Review {
    title: string;
    /** What will change, in words. */
    text: string;
    diff: string;
    /** "Backup → path", or why there is nothing to back up. */
    backup: string;
    confirmLabel: string;
    /** Removing: a red confirm button, Cancel focused first. */
    danger: boolean;
    /** Writes the change; resolves to the note shown once written. */
    apply: () => Promise<string>;
    /** The note when writing failed. */
    failed: (err: unknown) => string;
  }
</script>

<script lang="ts">
  import { Banner, Dialog } from "$lib/ds/index.ts";
  import { diffLines } from "$lib/overlay-settings.ts";

  let {
    review = $bindable(),
    ondone,
    oncancel,
  }: {
    review: Review | null;
    /** Written: the note to show. */
    ondone: (note: string) => void;
    /** Closed without writing. */
    oncancel?: () => void;
  } = $props();

  let open = $state(false);
  let error = $state("");
  $effect(() => {
    if (review) {
      error = "";
      open = true;
    }
  });

  async function confirm() {
    const current = review;
    if (!current) return;
    error = "";
    try {
      const note = await current.apply();
      review = null;
      ondone(note);
    } catch (err) {
      error = current.failed(err);
      throw err; // keeps the dialog open, its confirm button enabled again
    }
  }

  function cancel() {
    review = null;
    oncancel?.();
  }
</script>

<Dialog
  bind:open
  size="lg"
  title={review?.title ?? ""}
  description={review?.text}
  confirmLabel={review?.confirmLabel}
  tone={review?.danger ? "danger" : "default"}
  onconfirm={confirm}
  oncancel={cancel}
>
  {#if review}
    <div class="diff" role="region" aria-label="Change to be written">
      {#each diffLines(review.diff) as line, i (i)}
        <div class={line.kind}>{line.text || " "}</div>
      {/each}
    </div>
    <p class="backup">{review.backup}</p>
    {#if error}<Banner tone="danger">{error}</Banner>{/if}
  {/if}
</Dialog>

<style>
  .diff {
    max-height: 320px;
    overflow: auto;
    padding: var(--space-2) 0;
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-sunken);
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
    line-height: var(--line-height-normal);
    white-space: pre;
  }
  .diff > div { padding: 0 var(--space-3); }
  .add { background: var(--color-ok-subtle); color: var(--color-ok-text); }
  .del { background: var(--color-danger-subtle); color: var(--color-danger-text); }
  .ctx { color: var(--color-text-secondary); }
  .backup {
    margin: var(--space-2) 0;
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    overflow-wrap: anywhere;
  }
</style>
