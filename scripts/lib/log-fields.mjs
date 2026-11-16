// Finds log calls whose fields break the diagnostics log's rules (ADR-0013,
// docs/architecture/Logging.md). Used by scripts/check-log-fields.mjs; tested
// in scripts/tests/log-fields.test.ts.
//
// What is checked:
//  - Rust: `tracing` event and span macros (`info!`, `tracing::warn!`,
//    `event!`, `info_span!` …). Every field name must be on the allowlist in
//    crates/diagnostics/src/fields.rs; `message` and `event` are the event's
//    own. A forbidden name is always an error, also as a dotted segment
//    (`self.prompt`), and can never be allowlisted.
//  - TypeScript: `log.debug|info|warn|error("event", { … })` calls. The event
//    must be a snake_case string literal and the fields an object literal
//    whose keys follow the same rules.

const FIELD_SHORTHAND = /^[%?]?\s*([A-Za-z_][\w.]*)$/;
const FIELD_ASSIGN = /^(?:([A-Za-z_][\w.]*)|"([^"]*)")\s*=(?!=)/;
const KEYWORD_ARG = /^(target|parent|name)\s*:/;
const LEVEL_ARG = /^(?:tracing::)?Level::[A-Z]+$/;
const EVENT_NAME = /^[a-z][a-z0-9_]{0,63}$/;
const RUST_MACRO =
  /(?:\b([A-Za-z_][\w]*)::)?\b(trace|debug|info|warn|error|event|span|trace_span|debug_span|info_span|warn_span|error_span)!\s*\(/g;
const TS_CALL = /\blog\.(debug|info|warn|error)\s*\(/g;

/** The `ALLOWED` and `FORBIDDEN` arrays of crates/diagnostics/src/fields.rs. */
export function readFieldLists(source) {
  const list = (name) => {
    const m = new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`).exec(source);
    if (!m) throw new Error(`fields.rs: no ${name} array`);
    return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
  };
  return { allowed: new Set(list("ALLOWED")), forbidden: new Set(list("FORBIDDEN")) };
}

/**
 * Blank out comments, keeping strings and newlines, so offsets and line
 * numbers stay right. With `blankStrings`, string contents are blanked too
 * (to find calls without matching text inside strings). Knows Rust raw
 * strings and char literals, and JS template strings well enough for log
 * calls.
 */
export function stripComments(code, blankStrings = false) {
  let out = "";
  let i = 0;
  const blank = (s) => s.replace(/[^\n]/g, " ");
  while (i < code.length) {
    const c = code[i];
    const next = code[i + 1];
    if (c === "/" && next === "/") {
      const end = code.indexOf("\n", i);
      const stop = end < 0 ? code.length : end;
      out += blank(code.slice(i, stop));
      i = stop;
    } else if (c === "/" && next === "*") {
      const end = code.indexOf("*/", i + 2);
      const stop = end < 0 ? code.length : end + 2;
      out += blank(code.slice(i, stop));
      i = stop;
    } else if (c === '"' || c === "`" || (c === "r" && /^r#*"/.test(code.slice(i, i + 8)) && !/\w/.test(code[i - 1] ?? ""))) {
      const stop = stringEnd(code, i);
      out += blankStrings ? blank(code.slice(i, stop)) : code.slice(i, stop);
      i = stop;
    } else if (c === "'") {
      // A char literal ('"', '\'', 'x') or a lifetime / JS string.
      const m = /^'(?:\\.|[^\\'\n])'/.exec(code.slice(i, i + 4)) ?? /^'\\u\{[0-9a-fA-F]+\}'/.exec(code.slice(i, i + 12));
      const stop = m ? i + m[0].length : quoteEnd(code, i);
      out += blankStrings ? blank(code.slice(i, stop)) : code.slice(i, stop);
      i = stop;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

/** End (exclusive) of the string literal starting at `i`. */
function stringEnd(code, i) {
  if (code[i] === "r") {
    const hashes = /^r(#*)"/.exec(code.slice(i))[1];
    const close = `"${hashes}`;
    const end = code.indexOf(close, i + 2 + hashes.length);
    return end < 0 ? code.length : end + close.length;
  }
  const quote = code[i];
  let j = i + 1;
  while (j < code.length && code[j] !== quote) j += code[j] === "\\" ? 2 : 1;
  return Math.min(j + 1, code.length);
}

/** A JS single-quoted string, or a lone `'` (a Rust lifetime). */
function quoteEnd(code, i) {
  let j = i + 1;
  while (j < code.length && code[j] !== "'" && code[j] !== "\n") j += code[j] === "\\" ? 2 : 1;
  return code[j] === "'" ? j + 1 : i + 1;
}

/** The text inside the parentheses opening at `open`, and where it ends. */
function balanced(code, open) {
  let depth = 0;
  let i = open;
  while (i < code.length) {
    const c = code[i];
    if (c === '"' || c === "`" || (c === "'" && /^'(?:\\.|[^\\'\n])*'/.test(code.slice(i)))) {
      i = c === "'" ? quoteEnd(code, i) : stringEnd(code, i);
      continue;
    }
    if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") {
      depth--;
      if (depth === 0) return { body: code.slice(open + 1, i), end: i };
    }
    i++;
  }
  return { body: code.slice(open + 1), end: code.length };
}

/** Split on top-level commas. */
function splitArgs(body) {
  const args = [];
  let depth = 0;
  let start = 0;
  let i = 0;
  while (i < body.length) {
    const c = body[i];
    if (c === '"' || c === "`" || (c === "'" && /^'(?:\\.|[^\\'\n])*'/.test(body.slice(i)))) {
      i = c === "'" ? quoteEnd(body, i) : stringEnd(body, i);
      continue;
    }
    if ("([{".includes(c)) depth++;
    else if (")]}".includes(c)) depth--;
    else if (c === "," && depth === 0) {
      args.push(body.slice(start, i));
      start = i + 1;
    }
    i++;
  }
  args.push(body.slice(start));
  return args.map((a) => a.trim()).filter(Boolean);
}

const lineAt = (code, index) => code.slice(0, index).split("\n").length;

/** Field names of every `tracing` macro call in Rust source: [{ line, fields }]. */
export function scanRust(source) {
  const code = stripComments(source);
  const calls = [];
  for (const m of stripComments(source, true).matchAll(RUST_MACRO)) {
    // `log::info!` and other crates' macros are not tracing's.
    if (m[1] && m[1] !== "tracing") continue;
    const isSpan = m[2].includes("span");
    const open = m.index + m[0].length - 1;
    const fields = [];
    let named = !isSpan;
    for (const arg of splitArgs(balanced(code, open).body)) {
      if (KEYWORD_ARG.test(arg) || LEVEL_ARG.test(arg)) continue;
      const assign = FIELD_ASSIGN.exec(arg);
      if (assign) {
        fields.push(assign[1] ?? assign[2]);
        continue;
      }
      if (/^r?#*"/.test(arg)) {
        if (named) break; // the message; the rest are format arguments
        named = true; // a span's name
        continue;
      }
      const short = FIELD_SHORTHAND.exec(arg);
      if (short) fields.push(short[1]);
    }
    calls.push({ line: lineAt(code, m.index), fields });
  }
  return calls;
}

/** Event and field names of every `log.*` call in TypeScript: [{ line, event, fields, problems }]. */
export function scanTs(source) {
  const code = stripComments(source);
  const calls = [];
  for (const m of stripComments(source, true).matchAll(TS_CALL)) {
    const open = m.index + m[0].length - 1;
    const args = splitArgs(balanced(code, open).body);
    const problems = [];
    const event = /^["'`]([^"'`$]*)["'`]$/.exec(args[0] ?? "")?.[1];
    if (event === undefined) problems.push("the event name must be a string literal");
    else if (!EVENT_NAME.test(event)) problems.push(`event "${event}" is not a snake_case name`);
    const fields = [];
    if (args[1] !== undefined) {
      if (!/^\{[\s\S]*\}$/.test(args[1])) problems.push("fields must be an object literal");
      else {
        for (const prop of splitArgs(args[1].slice(1, -1))) {
          if (prop.startsWith("...")) {
            problems.push("fields must not be spread");
            continue;
          }
          const key = /^(?:([A-Za-z_$][\w$]*)|"([^"]*)"|'([^']*)')\s*(?::|\(|$)/.exec(prop);
          if (key) fields.push(key[1] ?? key[2] ?? key[3]);
          else problems.push(`computed field key ${prop.split(":")[0].trim()}`);
        }
      }
    }
    calls.push({ line: lineAt(code, m.index), event, fields, problems });
  }
  return calls;
}

/**
 * Whether a field name is a forbidden name, alone or as a dotted segment
 * (`self.prompt`). Snake_case parts are not split: `prompt_tokens` is a
 * count, and an unlisted name fails as "not on the allowlist" anyway.
 */
export function isForbidden(name, lists) {
  return name.split(".").some((part) => lists.forbidden.has(part));
}

/**
 * Problems in one file's calls: [{ line, why, forbidden }]. `message` and
 * `event` are the event's own names and always allowed.
 */
export function checkCalls(calls, lists) {
  const problems = [];
  for (const call of calls) {
    for (const why of call.problems ?? []) problems.push({ line: call.line, why, forbidden: false });
    for (const name of call.fields) {
      if (name === "message" || name === "event") continue;
      if (isForbidden(name, lists)) {
        problems.push({ line: call.line, why: `forbidden field "${name}" (user content or a credential)`, forbidden: true });
      } else if (!lists.allowed.has(name)) {
        problems.push({ line: call.line, why: `field "${name}" is not on the allowlist (crates/diagnostics/src/fields.rs)`, forbidden: false });
      }
    }
  }
  return problems;
}

/**
 * Compare findings with the pending allowlist: one entry per file, `max` =
 * the count of non-allowlisted fields it may still have. Forbidden names are
 * never covered.
 */
export function applyAllowlist(results, allow) {
  const failures = [];
  const notes = [];
  const byPath = new Map(allow.map((e) => [e.path, e]));
  for (const [file, problems] of results) {
    for (const p of problems.filter((p) => p.forbidden)) failures.push(`${file}:${p.line}: ${p.why}`);
    const pending = problems.filter((p) => !p.forbidden);
    if (pending.length === 0) continue;
    const entry = byPath.get(file);
    if (!entry) {
      for (const p of pending) failures.push(`${file}:${p.line}: ${p.why}`);
    } else if (pending.length > entry.max) {
      failures.push(`${file}: ${pending.length} non-allowlisted log fields, pending allowlist max is ${entry.max} (no new ones)`);
    } else if (pending.length < entry.max) {
      notes.push(`${file}: down to ${pending.length} (allowlist says ${entry.max}); lower "max" in scripts/log-fields-allowlist.json`);
    }
  }
  for (const [path] of byPath) {
    if (!results.has(path)) failures.push(`${path}: in the log-fields allowlist but not found; remove the entry`);
    else if (results.get(path).every((p) => p.forbidden)) failures.push(`${path}: in the log-fields allowlist but now clean; remove the entry`);
  }
  return { failures, notes };
}
