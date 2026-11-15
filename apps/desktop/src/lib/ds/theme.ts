// Which theme is showing: a forced `<html data-theme>` wins over the OS setting.
// For code that cannot read CSS variables (Mermaid, canvas). Tested in tests/ds.test.ts.

export type ThemeName = "light" | "dark";

/** `forced` is the value of `data-theme` (anything but "light" or "dark" means not forced). */
export function resolveTheme(forced: string | undefined | null, prefersDark: boolean): ThemeName {
  if (forced === "light" || forced === "dark") return forced;
  return prefersDark ? "dark" : "light";
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** The theme showing now. */
export function currentTheme(): ThemeName {
  const prefersDark = typeof matchMedia === "function" && matchMedia(DARK_QUERY).matches;
  const forced = typeof document === "undefined" ? undefined : document.documentElement.dataset.theme;
  return resolveTheme(forced, prefersDark);
}

/** Calls `onchange` when the forced theme or the OS setting changes. Returns the unsubscribe function. */
export function watchTheme(onchange: (theme: ThemeName) => void): () => void {
  if (typeof document === "undefined") return () => {};
  const notify = () => onchange(currentTheme());
  const observer = new MutationObserver(notify);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  const media = typeof matchMedia === "function" ? matchMedia(DARK_QUERY) : undefined;
  media?.addEventListener("change", notify);
  return () => {
    observer.disconnect();
    media?.removeEventListener("change", notify);
  };
}
