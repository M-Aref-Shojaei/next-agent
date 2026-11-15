<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import EmptyState from "./EmptyState.svelte";
  import Icon from "./Icon.svelte";
  import { nextSort, sortRows, type SortDir, type TableColumn } from "./table.ts";

  let {
    columns,
    rows,
    /** Stable key of a row (an id); the index is used when omitted. */
    rowKey,
    /** Accessible name of the table and of its scrollable region. */
    label,
    /** Key of the column the rows are sorted by (bindable). Leave unset to keep the given order. */
    sortKey = $bindable(),
    sortDir = $bindable("asc"),
    /** Title of the empty state, shown instead of the table when there are no rows. */
    empty = "Nothing here yet.",
    emptyDescription,
    /** Custom cell content. Without it, a column's `value` is shown. */
    cell,
    onsort,
  }: {
    columns: readonly TableColumn<T>[];
    rows: readonly T[];
    rowKey?: (row: T, index: number) => string | number;
    label: string;
    sortKey?: string;
    sortDir?: SortDir;
    empty?: string;
    emptyDescription?: string;
    cell?: Snippet<[row: T, column: TableColumn<T>]>;
    onsort?: (key: string, dir: SortDir) => void;
  } = $props();

  const sorted = $derived(sortRows(rows, columns.find((c) => c.key === sortKey), sortDir));

  function sortBy(key: string) {
    const next = nextSort(sortKey, sortDir, key);
    sortKey = next.key;
    sortDir = next.dir;
    onsort?.(next.key, next.dir);
  }

  const ariaSort = (c: TableColumn<T>) =>
    c.sortable ? (c.key === sortKey ? (sortDir === "asc" ? "ascending" : "descending") : "none") : undefined;
</script>

{#if rows.length === 0}
  <div class="ds-table-empty"><EmptyState compact title={empty} description={emptyDescription} /></div>
{:else}
  <!-- A scrollable region must be focusable so keyboard users can scroll it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="ds-table-scroll" role="region" aria-label={label} tabindex="0">
    <table class="ds-table">
      <thead>
        <tr>
          {#each columns as col (col.key)}
            <th scope="col" class:end={col.align === "end"} aria-sort={ariaSort(col)}>
              {#if col.sortable}
                <button type="button" class="sort" class:active={col.key === sortKey} onclick={() => sortBy(col.key)}>
                  <span>{col.label}</span>
                  <span class="arrow" class:up={col.key === sortKey && sortDir === "asc"} class:show={col.key === sortKey}>
                    <Icon name="chevron-down" size={12} />
                  </span>
                </button>
              {:else}
                {col.label}
              {/if}
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each sorted as row, i (rowKey ? rowKey(row, i) : i)}
          <tr>
            {#each columns as col (col.key)}
              <td class:end={col.align === "end"} class:nowrap={col.nowrap}>
                {#if cell}{@render cell(row, col)}{:else}{col.value?.(row) ?? ""}{/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .ds-table-scroll { overflow-x: auto; max-width: 100%; }
  .ds-table-scroll:focus-visible { outline: none; box-shadow: var(--focus-ring-inset); }
  .ds-table { width: 100%; min-width: max-content; border-collapse: collapse; font-size: var(--font-size-sm); }
  th, td { text-align: left; padding: var(--space-2) var(--space-3); border-bottom: var(--border-width) solid var(--color-border-subtle); vertical-align: top; }
  th { color: var(--color-text-secondary); font-weight: var(--font-weight-medium); white-space: nowrap; }
  tbody tr:last-child td { border-bottom: 0; }
  tbody tr:hover { background: var(--color-surface-hover); }
  th.end, td.end { text-align: right; }
  td.end { font-variant-numeric: tabular-nums; }
  .nowrap { white-space: nowrap; }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .sort:hover, .sort.active { color: var(--color-text); border-color: transparent; }
  .sort:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .arrow { display: inline-flex; opacity: 0; transition: transform var(--duration-fast) var(--ease-standard); }
  .arrow.show { opacity: 1; }
  .arrow.up { transform: rotate(180deg); }
  .sort:hover .arrow { opacity: 0.6; }
  .sort:hover .arrow.show { opacity: 1; }
</style>
