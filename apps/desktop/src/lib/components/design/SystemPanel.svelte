<script lang="ts">
  // The Design Studio's left column: where the workspace's design system lives
  // and its colour / type / space tokens. Actions go back to the page.
  import { Badge, Button, Dialog } from "$lib/ds/index.ts";
  import type { DesignFiles } from "$lib/design.ts";

  let {
    files,
    busy,
    confirmReplace = $bindable(false),
    ongenerate,
    onedit,
  }: {
    files: DesignFiles | null;
    busy: boolean;
    /** Asked when a saved design system already exists. */
    confirmReplace?: boolean;
    ongenerate: (overwrite: boolean) => void | Promise<void>;
    onedit: () => void;
  } = $props();

  const colorTokens = $derived(files?.tokens.filter((t) => t.kind === "color") ?? []);
  const otherTokens = $derived(files?.tokens.filter((t) => t.kind !== "color" && t.kind !== "other") ?? []);

  /** A token value as a swatch background: only colour values (the swatch shows the project's own colour). */
  const swatch = (v: string) => (/^(#|rgb|hsl|oklch|oklab|lab|lch|hwb)/i.test(v.trim()) ? v : "transparent");
</script>

<aside class="system" aria-labelledby="ds-title">
  <h2 id="ds-title">Design system</h2>
  {#if files}
    <p class="muted small">
      <code>{files.designPath}</code> · <code>{files.tokensPath}</code><br />
      {files.saved ? "Saved in the workspace" : "Draft from the project's CSS (not saved)"}
    </p>
    <div class="row">
      <Button size="sm" onclick={() => ongenerate(false)} disabled={busy}>Generate from project</Button>
      <Button size="sm" onclick={onedit} disabled={busy}>Edit files</Button>
    </div>
    <h3>Colour</h3>
    <ul class="swatches">
      {#each colorTokens as t (t.name)}
        <li title="{t.name}: {t.value}{t.dark ? ` / dark ${t.dark}` : ''}">
          <span class="chip" aria-hidden="true"><i style:background={swatch(t.value)}></i>{#if t.dark}<i style:background={swatch(t.dark)}></i>{/if}</span>
          <code>{t.name}</code>
        </li>
      {:else}
        <li class="muted">No colour tokens</li>
      {/each}
    </ul>
    {#if otherTokens.length}
      <h3>Type, space, shape</h3>
      <ul class="tokens">
        {#each otherTokens as t (t.name)}
          <li>
            <code>{t.name}</code> <span class="muted">{t.value}</span>
            {#if t.derived}<span title="Derived from literal values in the CSS"><Badge size="sm">derived</Badge></span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    <p class="muted">Reading the project…</p>
  {/if}
</aside>

<Dialog
  bind:open={confirmReplace}
  title="Replace the saved design system?"
  description="Replace the saved DESIGN.md and tokens.css with a fresh read of the project's CSS?"
  confirmLabel="Replace"
  tone="danger"
  onconfirm={() => ongenerate(true)}
/>

<style>
  .system { overflow: auto; padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); background: var(--color-surface); border-right: var(--border-width) solid var(--color-border); }
  h2 { font-size: var(--font-size-md); }
  h3 { font-size: var(--font-size-xs); text-transform: uppercase; letter-spacing: var(--letter-spacing-caps); color: var(--color-text-secondary); margin-top: var(--space-1-5); }
  .small { font-size: var(--font-size-sm); margin: 0; }
  .row { display: flex; gap: var(--space-1-5); align-items: center; flex-wrap: wrap; }
  .swatches { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); }
  .swatches li { display: flex; align-items: center; gap: var(--space-2); font-size: var(--font-size-sm); }
  .chip { display: inline-flex; border: var(--border-width) solid var(--color-border); border-radius: var(--radius-sm); overflow: hidden; }
  .chip i { width: 14px; height: 18px; display: block; }
  .tokens { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-0-5); font-size: var(--font-size-sm); }
</style>
