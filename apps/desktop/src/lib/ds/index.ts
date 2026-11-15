// Next Agent design system (docs/design/Design System.md, ADR-0012).
// Every screen, ours and vendored upstreams', builds on these components and
// the tokens in ./tokens.css (loaded by app.css).

export { default as Badge } from "./Badge.svelte";
export { default as Banner } from "./Banner.svelte";
export { default as BrandMark } from "./BrandMark.svelte";
export { default as Button } from "./Button.svelte";
export { default as Card } from "./Card.svelte";
export { default as Checkbox } from "./Checkbox.svelte";
export { default as ColorSwatchGroup } from "./ColorSwatchGroup.svelte";
export { default as Dialog } from "./Dialog.svelte";
export { default as EmptyState } from "./EmptyState.svelte";
export { default as Field } from "./Field.svelte";
export { default as Icon, ICON_NAMES, type IconName } from "./Icon.svelte";
export { default as IconButton } from "./IconButton.svelte";
export { default as Kbd } from "./Kbd.svelte";
export { default as List } from "./List.svelte";
export { default as ListRow } from "./ListRow.svelte";
export { default as ProgressBar } from "./ProgressBar.svelte";
export { default as RadioGroup } from "./RadioGroup.svelte";
export { default as SegmentedControl } from "./SegmentedControl.svelte";
export { default as Select } from "./Select.svelte";
export { default as SettingsLayout } from "./SettingsLayout.svelte";
export { default as SettingsRow } from "./SettingsRow.svelte";
export { default as SettingsSection } from "./SettingsSection.svelte";
export { default as ShortcutField } from "./ShortcutField.svelte";
export { default as Slider } from "./Slider.svelte";
export { default as Spinner } from "./Spinner.svelte";
export { default as StatusDot } from "./StatusDot.svelte";
export { default as StatTile } from "./StatTile.svelte";
export { default as Switch } from "./Switch.svelte";
export { default as Table } from "./Table.svelte";
export { default as Tabs } from "./Tabs.svelte";
export { default as TextArea } from "./TextArea.svelte";
export { default as TextField } from "./TextField.svelte";
export { default as Tooltip } from "./Tooltip.svelte";

export { compareValues, nextSort, sortRows, type SortDir, type TableColumn } from "./table.ts";
export { rovingIndex, type Orientation } from "./keyboard.ts";
export {
  currentPlatform,
  formatKeys,
  pressedKey,
  recordPress,
  shortcutLabel,
  shortcutParts,
  type KeyPress,
  type Platform,
  type Recorded,
} from "./shortcut.ts";
export type { FieldControl, Option, SettingsNavItem, Size, StatusTone, Tone } from "./types.ts";
