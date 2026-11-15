<script lang="ts">
  import type { Snippet } from "svelte";
  import Button from "./Button.svelte";

  let {
    open = $bindable(false),
    title,
    description,
    confirmLabel = "Confirm",
    cancelLabel = "Cancel",
    /** "danger" paints the confirm button red and focuses Cancel first. */
    tone = "default",
    /** Shows a spinner on the confirm button while the action runs. */
    busy = false,
    /** Hide the confirm button (an informational dialog with one "Close"). */
    hideConfirm = false,
    onconfirm,
    oncancel,
    /** Extra body content (a form, a list of what will change). */
    children,
  }: {
    open?: boolean;
    title: string;
    description?: string;
    confirmLabel?: string;
    cancelLabel?: string;
    tone?: "default" | "danger";
    busy?: boolean;
    hideConfirm?: boolean;
    /** Return a promise to keep the dialog open (busy) until it settles; throw to keep it open. */
    onconfirm?: () => void | Promise<void>;
    oncancel?: () => void;
    children?: Snippet;
  } = $props();

  const uid = $props.id();
  let dialog: HTMLDialogElement | undefined = $state();
  let running = $state(false);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  function cancel() {
    if (running) return;
    open = false;
    oncancel?.();
  }

  async function confirm() {
    running = true;
    try {
      await onconfirm?.();
      open = false;
    } catch {
      // The caller shows its own error; the dialog stays open.
    } finally {
      running = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="ds-dialog"
  aria-labelledby="{uid}-title"
  aria-describedby={description ? `${uid}-desc` : undefined}
  oncancel={(e) => { e.preventDefault(); cancel(); }}
  onclick={(e) => { if (e.target === dialog) cancel(); }}
>
  {#if open}
    <div class="body">
      <h2 id="{uid}-title">{title}</h2>
      {#if description}<p id="{uid}-desc" class="desc">{description}</p>{/if}
      {#if children}<div class="extra">{@render children()}</div>{/if}
    </div>
    <div class="actions">
      <!-- svelte-ignore a11y_autofocus -->
      <Button variant={hideConfirm ? "primary" : "secondary"} onclick={cancel} disabled={running} autofocus={tone === "danger" || hideConfirm}>{hideConfirm ? "Close" : cancelLabel}</Button>
      {#if !hideConfirm}
        <!-- svelte-ignore a11y_autofocus -->
        <Button
          variant={tone === "danger" ? "danger" : "primary"}
          loading={busy || running}
          onclick={confirm}
          autofocus={tone !== "danger"}
        >{confirmLabel}</Button>
      {/if}
    </div>
  {/if}
</dialog>

<style>
  .ds-dialog {
    width: min(440px, calc(100vw - 2 * var(--space-4)));
    max-height: calc(100vh - 2 * var(--space-8));
    padding: 0;
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-xl);
    background: var(--color-surface-overlay);
    color: var(--color-text);
    box-shadow: var(--shadow-xl);
    overflow: auto;
  }
  .ds-dialog[open] { animation: pop var(--duration-normal) var(--ease-out); }
  .ds-dialog::backdrop { background: var(--color-scrim); }
  .body { display: grid; gap: var(--space-2); padding: var(--space-5) var(--space-5) var(--space-4); }
  h2 { font-size: var(--font-size-lg); }
  .desc { margin: 0; color: var(--color-text-secondary); line-height: var(--line-height-relaxed); }
  .extra { margin-top: var(--space-2); }
  .actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-5);
    border-top: var(--border-width) solid var(--color-border-subtle);
    background: var(--color-surface-sunken);
  }
  @keyframes pop { from { opacity: 0; transform: translateY(6px) scale(0.98); } }
  @media (prefers-reduced-motion: reduce) { .ds-dialog[open] { animation: none; } }
</style>
