<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    description,
    /** Anchor id (SettingsLayout nav links point at it). Generated when omitted. */
    id,
    actions,
    /** Content outside the rows box, under it (notes, a table). */
    footer,
    children,
  }: {
    title: string;
    description?: string;
    id?: string;
    /** Buttons at the header's end ("Add", "Reset"). */
    actions?: Snippet;
    footer?: Snippet;
    /** <SettingsRow>s (or any content) inside the rows box. */
    children?: Snippet;
  } = $props();

  const uid = $props.id();
  const sectionId = $derived(id ?? uid);
</script>

<section class="ds-settings-section" id={sectionId} aria-labelledby="{sectionId}-title">
  <header>
    <div class="heading">
      <h2 id="{sectionId}-title">{title}</h2>
      {#if description}<p>{description}</p>{/if}
    </div>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  {#if children}<div class="rows">{@render children()}</div>{/if}
  {#if footer}<div class="footer">{@render footer()}</div>{/if}
</section>

<style>
  .ds-settings-section { display: grid; gap: var(--space-2); min-width: 0; scroll-margin-top: var(--space-4); }
  header { display: flex; align-items: flex-end; gap: var(--space-3); padding: 0 var(--space-1); }
  .heading { flex: 1; min-width: 0; display: grid; gap: var(--space-0-5); }
  h2 { font-size: var(--font-size-md); font-weight: var(--font-weight-semibold); }
  p { margin: 0; color: var(--color-text-secondary); font-size: var(--font-size-sm); line-height: var(--line-height-normal); }
  .actions { flex: none; display: flex; gap: var(--space-2); }
  .rows {
    container-type: inline-size;
    display: grid;
    min-width: 0;
    background: var(--color-surface);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
  }
  .rows > :global(* + *) { border-top: var(--border-width) solid var(--color-border-subtle); }
  .footer { padding: 0 var(--space-1); font-size: var(--font-size-sm); color: var(--color-text-secondary); }
</style>
