// Pure helpers for <Table>: sorting and the next sort state. Tested in tests/ds.test.ts.

export type SortDir = "asc" | "desc";

export interface TableColumn<T> {
  /** Stable id of the column (the sort key). */
  key: string;
  /** Header text. */
  label: string;
  /** Numbers line up on the right, in tabular figures. */
  align?: "start" | "end";
  /** Shows a sort button in the header. */
  sortable?: boolean;
  /** Plain value of the cell: shown when the table has no `cell` snippet, and the default sort value. */
  value?: (row: T) => string | number | null | undefined;
  /** Value to sort by, when it differs from `value` (a timestamp behind a formatted date). */
  sortValue?: (row: T) => string | number | null | undefined;
  /** Keep the cell on one line. */
  nowrap?: boolean;
}

type Sortable = string | number | null | undefined;

const isEmpty = (v: Sortable) => v === null || v === undefined || v === "";

/** Numbers numerically; strings in natural, locale-aware order. Empty values sort last. */
export function compareValues(a: Sortable, b: Sortable): number {
  if (isEmpty(a) || isEmpty(b)) return isEmpty(a) === isEmpty(b) ? 0 : isEmpty(a) ? 1 : -1;
  if (typeof a === "number" && typeof b === "number") return a - b;
  return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: "base" });
}

/** A sorted copy; the input is untouched. Stable: ties keep their order. Empty values stay last in both directions. */
export function sortRows<T>(rows: readonly T[], column: TableColumn<T> | undefined, dir: "asc" | "desc"): T[] {
  const get = column?.sortValue ?? column?.value;
  if (!get) return [...rows];
  const sign = dir === "asc" ? 1 : -1;
  return rows
    .map((row, i) => ({ row, i, v: get(row) }))
    .sort((x, y) => {
      if (isEmpty(x.v) || isEmpty(y.v)) return compareValues(x.v, y.v) || x.i - y.i;
      return sign * compareValues(x.v, y.v) || x.i - y.i;
    })
    .map((x) => x.row);
}

/** What a click on a header does: a new column starts ascending; the same column flips. */
export function nextSort(key: string | undefined, dir: SortDir, clicked: string): { key: string; dir: SortDir } {
  return key === clicked ? { key, dir: dir === "asc" ? "desc" : "asc" } : { key: clicked, dir: "asc" };
}
