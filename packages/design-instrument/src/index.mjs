// Next Agent Design Studio source instrumentation (Vite plugin).
//
// Adapted from Onlook (Apache-2.0, © 2024 On Off, Inc.):
// packages/parser/src/ids.ts (`addOidsToAst`: tag every JSX opening element)
// @ 423e2e9 — modified: no Babel and no ids; a small scanner adds
// `data-na-src="<file>:<line>:<col>"` to host elements in JSX/TSX, Svelte,
// Vue and HTML at dev time only, so the studio's edit mode can map a
// clicked element straight to its source position. Nothing is written to
// disk.
//
// Usage (vite.config.ts):
//   import { nextAgentSource } from "@next-agent/design-instrument";
//   export default defineConfig({ plugins: [nextAgentSource(), react()] });
//
// Svelte 5 and React <= 18 dev builds work without this plugin (the studio
// reads their own dev metadata); it is needed for React 19 and gives Vue
// exact lines.

const ATTR = "data-na-src";
const JSX_EXT = /\.(jsx|tsx)$/;
const MARKUP_EXT = /\.(svelte|vue|html)$/;
// Tokens after which `<` starts a JSX element rather than a comparison.
const JSX_BEFORE = new Set(["(", ",", "=", ":", "?", "{", "[", ">", "&", "|", "!", ";", "}", ""]);
const JSX_KEYWORDS = ["return", "yield", "await", "default", "case"];

function lineCol(code, index, lines) {
  // lines: sorted start offsets of each line
  let lo = 0;
  let hi = lines.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (lines[mid] <= index) lo = mid;
    else hi = mid - 1;
  }
  return [lo + 1, index - lines[lo] + 1];
}

function lineStarts(code) {
  const starts = [0];
  for (let i = 0; i < code.length; i++) if (code[i] === "\n") starts.push(i + 1);
  return starts;
}

const isTagStart = (c) => c >= "a" && c <= "z";
const isTagChar = (c) => /[A-Za-z0-9\-:._]/.test(c);
const escapeAttr = (s) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

function previousToken(code, i) {
  let j = i - 1;
  while (j >= 0 && /\s/.test(code[j])) j--;
  if (j < 0) return "";
  if (/[A-Za-z0-9_$]/.test(code[j])) {
    let k = j;
    while (k >= 0 && /[A-Za-z0-9_$]/.test(code[k])) k--;
    const word = code.slice(k + 1, j + 1);
    return JSX_KEYWORDS.includes(word) ? "(" : "ident";
  }
  if (code[j] === ">" && code[j - 1] === "=") return "(";
  return code[j];
}

/** Insert data-na-src into JSX host elements. */
export function instrumentJsx(code, file) {
  const starts = lineStarts(code);
  const inserts = [];
  const stack = []; // "`" template, "{" expression inside template
  let i = 0;
  while (i < code.length) {
    const c = code[i];
    const top = stack[stack.length - 1];
    if (top === "`") {
      if (c === "\\") { i += 2; continue; }
      if (c === "`") { stack.pop(); i++; continue; }
      if (c === "$" && code[i + 1] === "{") { stack.push("${"); i += 2; continue; }
      i++;
      continue;
    }
    if (c === "/" && code[i + 1] === "/") { const e = code.indexOf("\n", i); i = e < 0 ? code.length : e; continue; }
    if (c === "/" && code[i + 1] === "*") { const e = code.indexOf("*/", i + 2); i = e < 0 ? code.length : e + 2; continue; }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < code.length && code[j] !== c && code[j] !== "\n") j += code[j] === "\\" ? 2 : 1;
      i = j + 1;
      continue;
    }
    if (c === "`") { stack.push("`"); i++; continue; }
    if (c === "{" && top === "${") { stack.push("{"); i++; continue; }
    if (c === "}" && (top === "${" || top === "{")) { stack.pop(); i++; continue; }
    if (c === "<" && isTagStart(code[i + 1] ?? "") && JSX_BEFORE.has(previousToken(code, i))) {
      let j = i + 1;
      while (j < code.length && isTagChar(code[j])) j++;
      const name = code.slice(i + 1, j);
      if (!name.includes(".") && /[\s/>]/.test(code[j] ?? "")) {
        const [line, col] = lineCol(code, i, starts);
        inserts.push([j, ` ${ATTR}="${escapeAttr(`${file}:${line}:${col}`)}"`]);
      }
      i = j;
      continue;
    }
    i++;
  }
  return apply(code, inserts);
}

/** Insert data-na-src into element tags of Svelte / Vue / HTML markup. */
export function instrumentMarkup(code, file) {
  const starts = lineStarts(code);
  const inserts = [];
  const lower = code.toLowerCase();
  const vue = file.endsWith(".vue");
  let i = 0;
  let depth = 0; // { } expression depth (Svelte)
  let templateDepth = 0; // Vue: inside the top-level <template>
  while (i < code.length) {
    if (code.startsWith("<!--", i)) { const e = code.indexOf("-->", i); i = e < 0 ? code.length : e + 3; continue; }
    for (const raw of ["script", "style"]) {
      if (lower.startsWith(`<${raw}`, i) && /[\s>]/.test(code[i + raw.length + 1] ?? "")) {
        const e = lower.indexOf(`</${raw}`, i);
        i = e < 0 ? code.length : e;
        break;
      }
    }
    const c = code[i];
    if (c === "{" ) { depth++; i++; continue; }
    if (c === "}" && depth > 0) { depth--; i++; continue; }
    if (depth > 0) {
      if (c === '"' || c === "'" || c === "`") {
        let j = i + 1;
        while (j < code.length && code[j] !== c) j += code[j] === "\\" ? 2 : 1;
        i = j + 1;
        continue;
      }
      i++;
      continue;
    }
    if (c === "<" && isTagStart(code[i + 1] ?? "")) {
      let j = i + 1;
      while (j < code.length && isTagChar(code[j])) j++;
      const name = code.slice(i + 1, j).toLowerCase();
      if (vue && name === "template") {
        templateDepth++;
      } else if (!name.includes(":") && !name.includes(".") && name !== "slot" && name !== "template" && (!vue || templateDepth > 0) && /[\s/>]/.test(code[j] ?? "")) {
        const [line, col] = lineCol(code, i, starts);
        inserts.push([j, ` ${ATTR}="${escapeAttr(`${file}:${line}:${col}`)}"`]);
      }
      // Skip the rest of the tag head (quoted values may contain '>').
      let q = null;
      let braces = 0;
      for (; j < code.length; j++) {
        const d = code[j];
        if (q) { if (d === q) q = null; continue; }
        if (d === '"' || d === "'") q = d;
        else if (d === "{") braces++;
        else if (d === "}") braces--;
        else if (d === ">" && braces <= 0) break;
      }
      i = j + 1;
      continue;
    }
    if (vue && lower.startsWith("</template", i)) templateDepth = Math.max(0, templateDepth - 1);
    i++;
  }
  return apply(code, inserts);
}

function apply(code, inserts) {
  if (!inserts.length) return code;
  let out = "";
  let last = 0;
  for (const [at, text] of inserts) {
    out += code.slice(last, at) + text;
    last = at;
  }
  return out + code.slice(last);
}

/** Instrument one module by extension; returns the code unchanged otherwise. */
export function instrument(code, file) {
  if (JSX_EXT.test(file)) return instrumentJsx(code, file);
  if (MARKUP_EXT.test(file)) return instrumentMarkup(code, file);
  return code;
}

/** Vite plugin: dev server only, project files only. */
export function nextAgentSource() {
  let root = "";
  return {
    name: "next-agent-source",
    apply: "serve",
    enforce: "pre",
    configResolved(config) {
      root = config.root.replace(/\\/g, "/").replace(/\/$/, "");
    },
    transform(code, id) {
      const path = id.split("?")[0].replace(/\\/g, "/");
      if (path.includes("/node_modules/") || !path.startsWith(`${root}/`)) return null;
      const rel = path.slice(root.length + 1);
      const out = instrument(code, rel);
      return out === code ? null : { code: out, map: null };
    },
  };
}

export default nextAgentSource;
