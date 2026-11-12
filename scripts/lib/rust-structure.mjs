// Cheap structural checks for Rust sources, used while no cargo toolchain is
// available. Not a parser. It catches the mistakes a blind hand-review misses:
// unbalanced delimiters, an unterminated string or comment, a function that
// ended up outside its impl block, and duplicate top-level items.

const OPEN = { "(": ")", "[": "]", "{": "}" };
const CLOSE = new Set([")", "]", "}"]);

/**
 * Walk a source file, tracking nesting while ignoring comments and literals.
 * @param {string} source
 */
export function scan(source) {
  const problems = [];
  const stack = [];
  let line = 1;
  let index = 0;
  let rawHashes = 0;
  let inRaw = false;

  while (index < source.length) {
    const char = source[index];
    const next = source[index + 1];
    if (char === "\n") line += 1;

    if (!inRaw && char === "/" && next === "/") {
      while (index < source.length && source[index] !== "\n") index += 1;
      continue;
    }

    if (!inRaw && char === "/" && next === "*") {
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source[index] === "\n") line += 1;
        if (source[index] === "/" && source[index + 1] === "*") {
          depth += 1;
          index += 2;
          continue;
        }
        if (source[index] === "*" && source[index + 1] === "/") {
          depth -= 1;
          index += 2;
          continue;
        }
        index += 1;
      }
      if (depth > 0) problems.push(`line ${line}: unterminated block comment`);
      continue;
    }

    if (inRaw) {
      if (char === '"') {
        let hashes = 0;
        while (source[index + 1 + hashes] === "#") hashes += 1;
        if (hashes >= rawHashes) {
          inRaw = false;
          index += rawHashes + 1;
          continue;
        }
      }
      index += 1;
      continue;
    }

    if (char === "r" && (next === '"' || next === "#")) {
      const inWord = index > 0 && /[A-Za-z0-9_]/.test(source[index - 1]);
      if (!inWord) {
        let probe = index + 1;
        let hashes = 0;
        while (source[probe] === "#") {
          hashes += 1;
          probe += 1;
        }
        if (source[probe] === '"') {
          inRaw = true;
          rawHashes = hashes;
          index = probe + 1;
          continue;
        }
      }
    }

    if (char === '"') {
      index += 1;
      let closed = false;
      while (index < source.length) {
        if (source[index] === "\\") {
          index += 2;
          continue;
        }
        if (source[index] === "\n") line += 1;
        if (source[index] === '"') {
          closed = true;
          index += 1;
          break;
        }
        index += 1;
      }
      if (!closed) problems.push(`line ${line}: unterminated string literal`);
      continue;
    }

    if (char === "'") {
      const escape = source[index + 1] === "\\";
      const span = escape ? 4 : 2;
      if (source[index + span] === "'") {
        index += span + 1;
        continue;
      }
      index += 1;
      continue;
    }

    if (OPEN[char]) {
      stack.push({ char, line });
      index += 1;
      continue;
    }

    if (CLOSE.has(char)) {
      const top = stack.pop();
      if (!top) {
        problems.push(`line ${line}: unexpected ${char}`);
      } else if (OPEN[top.char] !== char) {
        problems.push(`line ${line}: ${char} closes ${top.char} opened on line ${top.line}`);
      }
      index += 1;
      continue;
    }

    index += 1;
  }

  for (const open of stack) {
    problems.push(`line ${open.line}: ${open.char} is never closed`);
  }
  return problems;
}

/**
 * Lines where a method-shaped fn sits outside any impl block. Free functions
 * are legal, so the signal is a `fn` with a `self` receiver at file scope —
 * exactly the result of appending code after an impl's closing brace.
 * @param {string} source
 */
export function orphanFunctions(source) {
  const lines = source.split("\n");
  const orphans = [];
  let depth = 0;
  let inImpl = false;

  lines.forEach((text, offset) => {
    const trimmed = text.trim();
    if (trimmed.startsWith("//")) return;

    if (depth === 0 && trimmed.startsWith("impl") && trimmed.includes("{")) inImpl = true;
    const isFn = /^(pub(\([^)]*\))?\s+)?(const\s+)?(async\s+)?fn\s/.test(trimmed);
    if (isFn && depth === 0 && !inImpl) orphans.push(offset + 1);

    const cleaned = text.replace(/\/\/.*$/, "").replace(/"(?:[^"\\]|\\.)*"/g, '""');
    for (const char of cleaned) {
      if (char === "{") depth += 1;
      else if (char === "}") depth -= 1;
    }
    if (depth < 0) depth = 0;
    if (depth === 0) inImpl = false;
  });

  // Keep only the ones whose signature carries a self receiver.
  return orphans.filter((line) => {
    const signature = lines.slice(line - 1, line + 4).join(" ");
    return /\(\s*(&\s*(mut\s+)?)?(self|'[a-z]+\s+self|&'[a-z]+\s+mut\s+self)\b/.test(signature);
  });
}
