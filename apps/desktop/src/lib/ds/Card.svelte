<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    description,
    /** "hero" is the about/intro card (brand mark + headline); "subtle" is a sunken panel. */
    variant = "default",
    padding = "md",
    /** Heading level for the title, to fit the page outline. */
    level = 3,
    media,
    actions,
    children,
  }: {
    title?: string;
    description?: string;
    variant?: "default" | "hero" | "subtle";
    padding?: "none" | "sm" | "md" | "lg";
    level?: 2 | 3 | 4;
    /** Left visual (a <BrandMark>, icon or image). */
    media?: Snippet;
    /** Buttons at the header's end. */
    actions?: Snippet;
    children?: Snippet;
  } = $props();
</script>

<section class="ds-card {variant} pad-{padding}">
  {#if title || media || actions}
    <header>
      {#if media}<div class="media">{@render media()}</div>{/if}
      {#if title || description}
        <div class="heading">
          {#if title}<svelte:element this={`h${level}`} class="title">{title}</svelte:element>{/if}
          {#if description}<p class="desc">{description}</p>{/if}
        </div>
      {/if}
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </header>
  {/if}
  {#if children}<div class="content">{@render children()}</div>{/if}
</section>

<style>
  .ds-card {
    display: grid;
    gap: var(--space-3);
    min-width: 0;
    background: var(--color-surface);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
  }
  .pad-none { padding: 0; }
  .pad-sm { padding: var(--space-3); }
  .pad-md { padding: var(--space-4); }
  .pad-lg { padding: var(--space-6); }
  .subtle { background: var(--color-surface-sunken); box-shadow: none; }
  .hero {
    background:
      radial-gradient(120% 140% at 0% 0%, var(--color-accent-subtle), transparent 60%),
      var(--color-surface);
  }
  header { display: flex; align-items: center; gap: var(--space-3); min-width: 0; }
  .heading { flex: 1; min-width: 0; display: grid; gap: var(--space-0-5); }
  .title { font-size: var(--font-size-md); font-weight: var(--font-weight-semibold); }
  .hero .title { font-size: var(--font-size-xl); }
  .desc { margin: 0; color: var(--color-text-secondary); font-size: var(--font-size-sm); line-height: var(--line-height-normal); }
  .hero .desc { font-size: var(--font-size-md); }
  .media { flex: none; display: flex; }
  .actions { flex: none; display: flex; gap: var(--space-2); flex-wrap: wrap; }
  .content { min-width: 0; }
  @media (max-width: 520px) {
    header { flex-wrap: wrap; }
  }
</style>
