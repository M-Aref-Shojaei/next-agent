// Shared prop types for the design-system components.

export type Size = "sm" | "md" | "lg";
/** Status tones. "neutral" is grey; "accent" is the interactive blue. */
export type Tone = "neutral" | "accent" | "info" | "ok" | "warn" | "danger";
export type StatusTone = "info" | "ok" | "warn" | "danger";

export interface Option<V extends string = string> {
  value: V;
  label: string;
  description?: string;
  disabled?: boolean;
}

/** One entry of <SettingsLayout>'s section nav. */
export interface SettingsNavItem {
  id: string;
  label: string;
  /** Route ("/settings/providers") or in-page anchor ("#general"). */
  href: string;
  /** Small marker after the label ("new", a count). */
  badge?: string | number;
}

/** What <Field> and <SettingsRow> hand to the control they wrap. */
export interface FieldControl {
  /** Put on the control: the label points at it. */
  id: string;
  /** Put on the control as aria-describedby (hint and error), or undefined. */
  describedby: string | undefined;
  invalid: boolean;
  /** Id of the visible label, for controls a <label for> cannot name (groups): use as aria-labelledby. */
  labelId: string;
}

/** Joins the ids that exist into an aria-describedby value. */
export function describedBy(...ids: (string | false | null | undefined)[]): string | undefined {
  const list = ids.filter(Boolean);
  return list.length ? list.join(" ") : undefined;
}
