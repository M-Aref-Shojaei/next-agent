// Finds raw colour values and font stacks in style code (ADR-0012, docs/design/Design System.md).
// Used by scripts/check-design-tokens.mjs; tested in scripts/tests/design-tokens.test.ts.
//
// What counts as a violation:
//  - a colour literal (#hex, rgb()/rgba()/hsl()/hsla()/hwb()/lab()/lch()/oklab()/oklch()/color())
//    in any CSS declaration value;
//  - a named colour ("red", "white" …) in a colour property;
//  - a font-family declaration whose value is not only var(--font-…) or a CSS-wide keyword.
// Where style code is looked for:
//  - .css: the whole file;
//  - .svelte: <style> blocks, style="…" attributes, style:prop="…" directives, and colour
//    presentation attributes (fill, stroke, color, stop-color …) in the markup; <script> as .ts;
//  - .ts/.js: CSS-like declarations in the code ("color: '#fff'", "background: 'red'",
//    `fontFamily: "Inter"`), which is where style strings and style objects put them.

const COLOUR_FN = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i;
const HEX = /(^|[^\w&-])#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})(?![\w-])/i;

const NAMED_COLOURS = new Set(
  ("aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown " +
    "burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan " +
    "darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid " +
    "darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet " +
    "deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro " +
    "ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki " +
    "lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow " +
    "lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray " +
    "lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine " +
    "mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise " +
    "mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab " +
    "orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru " +
    "pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown " +
    "seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan " +
    "teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen").split(" "),
);

/** Properties whose value is (or contains) a colour, kebab-case. */
const COLOUR_PROPS =
  /^(?:color|background(?:-color)?|border(?:-(?:top|right|bottom|left|block|inline)(?:-(?:start|end))?)?(?:-color)?|outline(?:-color)?|fill|stroke|stop-color|flood-color|lighting-color|caret-color|accent-color|column-rule(?:-color)?|text-decoration(?:-color)?|text-emphasis-color|box-shadow|text-shadow|scrollbar-color)$/i;

/** SVG/HTML presentation attributes that take a colour. */
const COLOUR_ATTRS = /\s(fill|stroke|color|stop-color|flood-color|lighting-color|bgcolor)\s*=\s*(["'])([^"']*)\2/gi;

const kebab = (s) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);

/** Why a declaration breaks the rule, or null. `prop` may be camelCase. */
export function checkDeclaration(prop, value) {
  const p = kebab(prop.trim()).toLowerCase();
  const v = value.trim().replace(/^["'`]|["'`]$/g, "");
  if (p === "font-family") {
    const ok = /^(?:var\(--font-[\w-]+\)|inherit|initial|unset|revert)$/i.test(v);
    return ok ? null : `font-family "${v}" (use var(--font-sans) or var(--font-mono))`;
  }
  if (HEX.test(v)) return `raw colour ${v.match(HEX)[0].trim()} in ${p}`;
  if (COLOUR_FN.test(v)) return `raw colour function in ${p}: ${v.replace(/\s+/g, " ")}`;
  // Named colours: any word of a colour property's value (outside var()/url()), or a
  // custom property whose whole value is one.
  const words = v.replace(/\b(?:var|url)\([^)]*\)/gi, " ").toLowerCase().split(/[^a-z]+/);
  const named = COLOUR_PROPS.test(p)
    ? words.find((w) => NAMED_COLOURS.has(w))
    : p.startsWith("--") && NAMED_COLOURS.has(v.toLowerCase()) ? v : undefined;
  return named ? `named colour "${named}" in ${p}` : null;
}

function stripComments(css) {
  // Keep newlines so line numbers stay right.
  return css.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
}

const stripJsComments = (code) =>
  stripComments(code).replace(/(^|[^:"'`\\])\/\/[^\n]*/g, (m, pre) => pre + " ".repeat(m.length - pre.length));

/** Turns {index, why} findings into {line, why}, counting newlines in one pass. */
function withLines(text, found) {
  found.sort((a, b) => a.index - b.index);
  let line = 1;
  let pos = 0;
  return found.map(({ index, why }) => {
    for (; pos < index; pos++) if (text.charCodeAt(pos) === 10) line++;
    return { line, why };
  });
}

function cssFindings(css, offset = 0) {
  const out = [];
  const re = /(--[\w-]+|[a-zA-Z-]+)\s*:\s*([^;{}]+?)\s*(?=[;}]|$)/g;
  for (const m of stripComments(css).matchAll(re)) {
    const why = checkDeclaration(m[1], m[2]);
    if (why) out.push({ index: offset + m.index, why });
  }
  return out;
}

function scriptFindings(code, offset = 0) {
  const out = [];
  // prop: "value" | prop: 'value' | prop: `value` | "prop: value;" inside a string
  const re = /([a-zA-Z-]+|--[\w-]+)["']?\s*:\s*(["'`]?)([^"'`;,\n}]+)\2/g;
  for (const m of stripJsComments(code).matchAll(re)) {
    const prop = kebab(m[1]).toLowerCase();
    if (prop !== "font-family" && !COLOUR_PROPS.test(prop) && !prop.startsWith("--")) continue;
    const why = checkDeclaration(prop, m[3]);
    if (why) out.push({ index: offset + m.index, why });
  }
  return out;
}

/** Declarations in CSS text: `prop: value` ended by ; or }. */
export const scanCss = (css) => withLines(css, cssFindings(css));

/** CSS-like declarations in JS/TS source (style strings and style objects). */
export const scanScript = (code) => withLines(code, scriptFindings(code));

/** All violations in one file's text, by extension. */
export function scanFile(path, text) {
  if (path.endsWith(".css")) return scanCss(text);
  if (/\.[cm]?[jt]s$/.test(path)) return scanScript(text);
  if (!path.endsWith(".svelte")) return [];
  const out = [];
  let markup = text;
  // <style> as CSS, <script> as TS; then blank them out so only markup is left.
  for (const [re, find] of [[/<style\b[^>]*>([\s\S]*?)<\/style>/g, cssFindings], [/<script\b[^>]*>([\s\S]*?)<\/script>/g, scriptFindings]]) {
    for (const m of text.matchAll(re)) {
      out.push(...find(m[1], m.index + m[0].indexOf(">") + 1));
      markup = markup.slice(0, m.index) + m[0].replace(/[^\n]/g, " ") + markup.slice(m.index + m[0].length);
    }
  }
  const add = (index, prop, value) => {
    const why = checkDeclaration(prop, value);
    if (why) out.push({ index, why });
  };
  for (const m of markup.matchAll(/\sstyle\s*=\s*(["'])([\s\S]*?)\1/g)) {
    for (const decl of m[2].split(";")) {
      const i = decl.indexOf(":");
      if (i >= 0) add(m.index, decl.slice(0, i), decl.slice(i + 1));
    }
  }
  for (const m of markup.matchAll(/\sstyle:([\w-]+)\s*=\s*(["'])([^"']*)\2/g)) add(m.index, m[1], m[3]);
  for (const m of markup.matchAll(COLOUR_ATTRS)) add(m.index, m[1] === "bgcolor" ? "background-color" : m[1], m[3]);
  return withLines(text, out);
}

/**
 * Applies the allowlist. `results` maps a file to its violations; `allow` is the
 * allowlist's entries. Returns failures (strings) and notes (strings).
 */
export function applyAllowlist(results, allow) {
  const failures = [];
  const notes = [];
  // "pending" entries name trees outside the scan (vendored upstreams): tracked, not checked.
  const exact = new Map(allow.filter((e) => !e.pending).map((e) => [e.path, e]));
  for (const [file, problems] of results) {
    if (problems.length === 0) continue;
    const entry = exact.get(file);
    if (!entry) {
      for (const p of problems) failures.push(`${file}:${p.line}: ${p.why}`);
    } else if (problems.length > entry.max) {
      failures.push(`${file}: ${problems.length} raw values, allowlist max is ${entry.max} (no new ones; move them to tokens)`);
    } else if (problems.length < entry.max) {
      notes.push(`${file}: down to ${problems.length} (allowlist says ${entry.max}); lower "max" in scripts/design-tokens-allowlist.json`);
    }
  }
  for (const [path] of exact) {
    if (!results.has(path)) failures.push(`${path}: allowlisted but not found; remove the entry`);
    else if (results.get(path).length === 0) failures.push(`${path}: allowlisted but now clean; remove the entry`);
  }
  return { failures, notes };
}

/** The [data-theme="dark"] block must repeat the prefers-color-scheme dark block exactly. */
export function darkThemeParity(tokensCss) {
  const text = stripComments(tokensCss);
  const media = /@media\s*\(prefers-color-scheme:\s*dark\)\s*\{\s*:root:not\(\[data-theme="light"\]\)\s*\{([^}]*)\}/.exec(text);
  const forced = /\n\[data-theme="dark"\]\s*\{([^}]*)\}/.exec(text);
  if (!media || !forced) return ["tokens.css: dark theme blocks not found"];
  const decls = (s) => s.split(";").map((d) => d.replace(/\s+/g, " ").trim()).filter(Boolean);
  const a = decls(media[1]);
  const b = decls(forced[1]);
  const missing = a.filter((d) => !b.includes(d)).map((d) => `only in the media query: ${d}`);
  const extra = b.filter((d) => !a.includes(d)).map((d) => `only in [data-theme="dark"]: ${d}`);
  return [...missing, ...extra].map((m) => `tokens.css dark blocks differ, ${m}`);
}
