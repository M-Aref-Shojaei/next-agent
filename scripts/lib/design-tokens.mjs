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

const lineOf = (text, index) => text.slice(0, index).split("\n").length;

function stripComments(css) {
  // Keep newlines so line numbers stay right.
  return css.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
}

function stripJsComments(code) {
  return code
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/(^|[^:"'`\\])\/\/[^\n]*/g, (m, pre) => pre + " ".repeat(m.length - pre.length));
}

/** Declarations in CSS text: `prop: value` ended by ; or }. */
export function scanCss(css, offset = 0, full = css) {
  const out = [];
  const text = stripComments(css);
  const re = /(--[\w-]+|[a-zA-Z-]+)\s*:\s*([^;{}]+?)\s*(?=[;}]|$)/g;
  for (const m of text.matchAll(re)) {
    const why = checkDeclaration(m[1], m[2]);
    if (why) out.push({ line: lineOf(full, offset + m.index), why });
  }
  return out;
}

/** CSS-like declarations in JS/TS source (style strings and style objects). */
export function scanScript(code, offset = 0, full = code) {
  const out = [];
  const text = stripJsComments(code);
  // prop: "value" | prop: 'value' | prop: `value` | "prop: value;" inside a string
  const re = /([a-zA-Z-]+|--[\w-]+)["']?\s*:\s*(["'`]?)([^"'`;,\n}]+)\2/g;
  for (const m of text.matchAll(re)) {
    const prop = kebab(m[1]).toLowerCase();
    if (prop !== "font-family" && !COLOUR_PROPS.test(prop) && !prop.startsWith("--")) continue;
    const why = checkDeclaration(m[1], m[3]);
    if (why) out.push({ line: lineOf(full, offset + m.index), why });
  }
  return out;
}

/** All violations in one file's text, by extension. */
export function scanFile(path, text) {
  if (path.endsWith(".css")) return scanCss(text);
  if (/\.[cm]?[jt]s$/.test(path)) return scanScript(text);
  if (!path.endsWith(".svelte")) return [];
  const out = [];
  const blocks = [];
  for (const m of text.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style>/g)) {
    const start = m.index + m[0].indexOf(">") + 1;
    blocks.push([m.index, m.index + m[0].length]);
    out.push(...scanCss(m[1], start, text));
  }
  for (const m of text.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)) {
    const start = m.index + m[0].indexOf(">") + 1;
    blocks.push([m.index, m.index + m[0].length]);
    out.push(...scanScript(m[1], start, text));
  }
  // Markup only from here: blank out style and script blocks.
  let markup = text;
  for (const [a, b] of blocks) markup = markup.slice(0, a) + markup.slice(a, b).replace(/[^\n]/g, " ") + markup.slice(b);
  for (const m of markup.matchAll(/\sstyle\s*=\s*(["'])([\s\S]*?)\1/g)) {
    for (const decl of m[2].split(";")) {
      const i = decl.indexOf(":");
      if (i < 0) continue;
      const why = checkDeclaration(decl.slice(0, i), decl.slice(i + 1));
      if (why) out.push({ line: lineOf(text, m.index), why });
    }
  }
  for (const m of markup.matchAll(/\sstyle:([\w-]+)\s*=\s*(["'])([^"']*)\2/g)) {
    const why = checkDeclaration(m[1], m[3]);
    if (why) out.push({ line: lineOf(text, m.index), why });
  }
  for (const m of markup.matchAll(COLOUR_ATTRS)) {
    const why = checkDeclaration(m[1] === "bgcolor" ? "background-color" : m[1], m[3]);
    if (why) out.push({ line: lineOf(text, m.index), why });
  }
  return out.sort((a, b) => a.line - b.line);
}

/**
 * Applies the allowlist. `results` maps a file to its violations; `allow` is the
 * allowlist's entries. Returns failures (strings) and notes (strings).
 */
export function applyAllowlist(results, allow) {
  const failures = [];
  const notes = [];
  const exact = new Map(allow.filter((e) => !e.path.includes("*")).map((e) => [e.path, e]));
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
