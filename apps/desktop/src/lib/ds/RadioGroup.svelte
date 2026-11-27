<script lang="ts" generics="V extends string">
  import type { Option } from "./types.ts";

  let {
    value = $bindable(),
    options,
    /** The group's name, shown as the legend. */
    label,
    hideLabel = false,
    hint,
    orientation = "vertical",
    disabled = false,
    name,
    onchange,
  }: {
    value?: V;
    options: readonly Option<V>[];
    label: string;
    hideLabel?: boolean;
    hint?: string;
    orientation?: "vertical" | "horizontal";
    disabled?: boolean;
    /** Native radio name; generated when omitted. */
    name?: string;
    onchange?: (value: V) => void;
  } = $props();

  const uid = $props.id();
  const groupName = $derived(name ?? `${uid}-radio`);
</script>

<fieldset class="ds-radio {orientation}" {disabled} aria-describedby={hint ? `${uid}-hint` : undefined}>
  <legend class:sr-only={hideLabel}>{label}</legend>
  {#if hint}<p class="hint" id="{uid}-hint">{hint}</p>{/if}
  <div class="options">
    {#each options as opt (opt.value)}
      <label class="opt" class:disabled={disabled || opt.disabled}>
        <input
          type="radio"
          name={groupName}
          value={opt.value}
          disabled={opt.disabled}
          bind:group={value}
          onchange={() => onchange?.(opt.value)}
        />
        <span class="dot" aria-hidden="true"></span>
        <span class="text">
          <span>{opt.label}</span>
          {#if opt.description}<span class="desc">{opt.description}</span>{/if}
        </span>
      </label>
    {/each}
  </div>
</fieldset>

<style>
  fieldset { margin: 0; padding: 0; border: 0; min-width: 0; display: grid; gap: var(--space-1-5); }
  legend { padding: 0; font-size: var(--font-size-sm); font-weight: var(--font-weight-medium); }
  .hint { margin: 0; font-size: var(--font-size-sm); color: var(--color-text-secondary); }
  .options { display: grid; gap: var(--space-2); }
  .horizontal .options { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-4); }
  .opt { position: relative; display: inline-flex; align-items: flex-start; gap: var(--space-2); cursor: pointer; }
  .opt.disabled { cursor: not-allowed; opacity: 0.55; }
  input { position: absolute; opacity: 0; width: var(--space-4); height: var(--space-4); margin: 0; }
  .dot {
    flex: none;
    width: var(--space-4);
    height: var(--space-4);
    margin-top: var(--space-0-5);
    border-radius: var(--radius-full);
    border: var(--border-width) solid var(--color-border-strong);
    background: var(--color-surface);
    transition: border-color var(--duration-fast) var(--ease-standard), box-shadow var(--duration-fast) var(--ease-standard);
  }
  .opt:hover .dot { border-color: var(--color-accent); }
  input:checked + .dot { border: calc(var(--space-1) + var(--border-width)) solid var(--color-accent); background: var(--color-control-thumb); }
  input:focus-visible + .dot { box-shadow: var(--focus-ring); }
  .text { display: grid; gap: var(--space-0-5); }
  .desc { color: var(--color-text-secondary); font-size: var(--font-size-sm); }
</style>
