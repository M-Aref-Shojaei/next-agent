<script lang="ts" module>
  // A small built-in icon set (24×24 strokes, currentColor). Enough for the
  // design system's own controls and common actions; add paths here rather
  // than pasting SVGs into pages.
  const PATHS = {
    check: "M5 12.5l4.5 4.5L19 7.5",
    x: "M6 6l12 12M18 6L6 18",
    plus: "M12 5v14M5 12h14",
    minus: "M5 12h14",
    "chevron-down": "M6 9l6 6 6-6",
    "chevron-right": "M9 6l6 6-6 6",
    "chevron-left": "M15 6l-6 6 6 6",
    info: "M12 11v5M12 8h.01M12 3a9 9 0 100 18 9 9 0 000-18z",
    ok: "M8 12.5l3 3 5-6M12 3a9 9 0 100 18 9 9 0 000-18z",
    warn: "M12 9v4M12 16.5h.01M10.3 4.3L2.7 17.5A2 2 0 004.4 20.5h15.2a2 2 0 001.7-3L13.7 4.3a2 2 0 00-3.4 0z",
    danger: "M12 8v5M12 16h.01M12 3a9 9 0 100 18 9 9 0 000-18z",
    search: "M11 4a7 7 0 100 14 7 7 0 000-14zM20 20l-4-4",
    trash: "M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 002 2h6a2 2 0 002-2l1-12M9 7V4h6v3",
    edit: "M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4",
    more: "M5 12h.01M12 12h.01M19 12h.01",
    settings: "M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z",
    copy: "M9 9h10v10H9zM5 15V5h10",
    refresh: "M20 11a8 8 0 10-2.3 5.7M20 5v6h-6",
    external: "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 01-1 1H5a1 1 0 01-1-1V7a1 1 0 011-1h5",
  } as const;
  export type IconName = keyof typeof PATHS;
  export const ICON_NAMES = Object.keys(PATHS) as IconName[];
</script>

<script lang="ts">
  let {
    name,
    size = 16,
    /** Visible to screen readers when set; decorative (aria-hidden) otherwise. */
    label,
  }: { name: IconName; size?: number; label?: string } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
  focusable="false"
><path d={PATHS[name]} /></svg>

<style>
  .icon { flex: none; display: inline-block; vertical-align: middle; }
</style>
