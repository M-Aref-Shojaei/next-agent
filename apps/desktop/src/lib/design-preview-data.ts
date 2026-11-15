// Colours and CSS that are DESIGN PREVIEW DATA, not UI chrome: the canvas a
// preview iframe is painted on, and the page CSS of the offline mock
// directions. A previewed page brings its own palette, so these cannot follow
// the app's light/dark tokens.

/** The surface behind a preview iframe: white, as a browser paints a page with no background. */
export const PREVIEW_CANVAS = "#ffffff";

/** The page CSS of a mock direction (browser dev build), after its design tokens. */
export function mockPageCss(radius: string, gap: string): string {
  return `body{margin:0;font-family:system-ui;background:var(--bg);color:var(--text)}
header{display:flex;justify-content:space-between;align-items:center;padding:${gap};border-bottom:1px solid var(--border);background:var(--panel)}
h1{margin:0;font-size:22px}main{padding:${gap};display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:${gap}}
.card{background:var(--panel);border:1px solid var(--border);border-radius:${radius};padding:${gap};display:grid;gap:8px}
.card h2{margin:0;font-size:15px}p{margin:0;color:var(--muted)}
button{font:inherit;border:1px solid var(--border);border-radius:${radius};background:var(--panel);color:var(--text);padding:6px 12px;justify-self:start}
.primary{background:var(--accent);border-color:var(--accent);color:#fff}`;
}
