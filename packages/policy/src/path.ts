// Mirror of crates/permission-engine/src/path.rs. Pure: filesystem facts come
// in through `FsFacts`. Shared fixture: fixtures/path-corpus.json.

export type Zone = "inside" | "outside" | "sensitive";
export type PathStyle = "posix" | "windows";

export interface FsFacts {
  /** Fully resolved path if it exists (follows every link), else null. */
  canonicalize(path: string): string | null;
  /** Target if `path` itself is a link (even dangling), else null. */
  readLink(path: string): string | null;
}

/** Knows nothing about the disk: resolution is purely lexical. */
export const lexical: FsFacts = {
  canonicalize: () => null,
  readLink: () => null,
};

export interface SandboxConfig {
  root: string;
  home: string | null;
  pathStyle: PathStyle;
  caseInsensitive: boolean;
}

export interface PathCheck {
  zone: Zone;
  resolved: string | null;
  note: string | null;
}

export const SENSITIVE_IN_HOME = [
  ".ssh", ".aws", ".azure", ".gnupg", ".kube", ".docker", ".config", ".netrc", ".npmrc", ".pypirc",
  ".git-credentials", ".gitconfig", ".password-store", ".vault-token", ".terraform.d", ".cargo/credentials",
  ".cargo/credentials.toml", ".local/share/keyrings", ".bashrc", ".bash_profile", ".bash_login", ".profile",
  ".zshrc", ".zshenv", ".zprofile", ".zlogin", "Library/Keychains", "Library/LaunchAgents", "Library/Cookies",
  "AppData/Roaming/Microsoft/Credentials", "AppData/Local/Microsoft/Credentials", "AppData/Roaming/Microsoft/Protect",
  "AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup",
];

export const SENSITIVE_ANYWHERE = [".ssh", ".aws", ".gnupg", ".kube", ".password-store", ".netrc", ".git-credentials", "Keychains"];

export const SENSITIVE_POSIX = [
  "/etc", "/private/etc", "/usr", "/bin", "/sbin", "/boot", "/dev", "/proc", "/sys", "/root", "/System",
  "/Library/Keychains", "/Library/LaunchAgents", "/Library/LaunchDaemons", "/var/root", "/private/var/root",
  "/var/db", "/private/var/db",
];

export const SENSITIVE_WINDOWS = ["Windows", "Program Files", "Program Files (x86)", "ProgramData", "System Volume Information"];

const RESERVED_DEVICE_NAMES = [
  "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
  "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

export function asciiLower(text: string): string {
  return text.replace(/[A-Z]/g, (c) => c.toLowerCase());
}

export function isSecretName(name: string): boolean {
  const lower = asciiLower(name);
  return lower === ".env" || lower.startsWith(".env.") || lower === ".envrc";
}

export function isSecretFile(path: string): boolean {
  const parts = path.split(/[\\/]/);
  return isSecretName(parts[parts.length - 1] ?? "");
}

// ---------------------------------------------------------------------------
// Parsing

type Prefix = { kind: "root" } | { kind: "drive"; letter: string } | { kind: "unc"; server: string; share: string };

interface AbsPath {
  prefix: Prefix;
  parts: string[];
}

type Parsed =
  | { kind: "abs"; path: AbsPath; verbatim: boolean }
  | { kind: "rel"; parts: string[] }
  | { kind: "rootrel"; parts: string[] };

class PathError extends Error {}

function fail(note: string): never {
  throw new PathError(note);
}

function render(path: AbsPath): string {
  const p = path.prefix;
  if (p.kind === "root") return "/" + path.parts.join("/");
  if (p.kind === "drive") return `${p.letter}:\\` + path.parts.join("\\");
  return `\\\\${p.server}\\${p.share}` + path.parts.map((part) => "\\" + part).join("");
}

function samePrefix(a: Prefix, b: Prefix): boolean {
  if (a.kind === "root" && b.kind === "root") return true;
  if (a.kind === "drive" && b.kind === "drive") return a.letter === b.letter;
  if (a.kind === "unc" && b.kind === "unc") return a.server === b.server && a.share === b.share;
  return false;
}

function sameAbs(a: AbsPath, b: AbsPath): boolean {
  return samePrefix(a.prefix, b.prefix) && a.parts.length === b.parts.length && a.parts.every((p, i) => p === b.parts[i]);
}

function parentOf(path: AbsPath): AbsPath | null {
  if (path.parts.length === 0) return null;
  return { prefix: path.prefix, parts: path.parts.slice(0, -1) };
}

function lexicalOf(path: AbsPath): AbsPath {
  const parts: string[] = [];
  for (const part of path.parts) {
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return { prefix: path.prefix, parts };
}

function parse(text: string, style: PathStyle): Parsed {
  if (text.includes("\0")) fail("path contains a NUL byte");
  if (style === "posix") {
    if (text.startsWith("/")) return { kind: "abs", path: { prefix: { kind: "root" }, parts: posixParts(text.slice(1)) }, verbatim: false };
    return { kind: "rel", parts: posixParts(text) };
  }
  return parseWindows(text);
}

function posixParts(text: string): string[] {
  return text.split("/").filter((part) => part !== "" && part !== ".");
}

const isAlpha = (c: string | undefined): boolean => c !== undefined && /^[A-Za-z]$/.test(c);

function parseWindows(text: string): Parsed {
  const chars = [...text];
  const sep = (i: number): boolean => chars[i] === "\\" || chars[i] === "/";

  if (text.startsWith("\\\\?\\") || text.startsWith("\\??\\")) {
    const rest = text.slice(4);
    if (rest.includes("/")) fail("forward slash inside a verbatim path");
    if (asciiLower(rest).startsWith("unc\\")) return uncPath(rest.slice(4), true);
    const restChars = [...rest];
    if (restChars.length >= 2 && isAlpha(restChars[0]) && restChars[1] === ":") {
      if (restChars.length > 2 && restChars[2] !== "\\") fail("malformed verbatim drive path");
      return {
        kind: "abs",
        path: { prefix: { kind: "drive", letter: restChars[0].toUpperCase() }, parts: verbatimParts(rest.slice(3)) },
        verbatim: true,
      };
    }
    fail("device or volume path");
  }
  if (sep(0) && sep(1) && chars[2] === "." && (chars.length === 3 || sep(3))) fail("device path");
  if (sep(0) && sep(1)) return uncPath(text.slice(2), false);
  if (chars.length >= 2 && isAlpha(chars[0]) && chars[1] === ":") {
    if (!sep(2)) fail("drive-relative path");
    return { kind: "abs", path: { prefix: { kind: "drive", letter: chars[0].toUpperCase() }, parts: windowsParts(text.slice(3)) }, verbatim: false };
  }
  if (sep(0)) return { kind: "rootrel", parts: windowsParts(text.slice(1)) };
  return { kind: "rel", parts: windowsParts(text) };
}

function uncPath(rest: string, verbatim: boolean): Parsed {
  const pieces = rest.split(/[\\/]/);
  const server = asciiLower(pieces[0] ?? "");
  const share = asciiLower(pieces[1] ?? "");
  if (server === "" || share === "") fail("incomplete UNC path");
  const remainder = pieces.slice(2).join("\\");
  const parts = verbatim ? verbatimParts(remainder) : windowsParts(remainder);
  const local = ["localhost", "127.0.0.1", "[::1]", "::1", "0:0:0:0:0:0:0:1"].includes(server) || server.startsWith("127.");
  const shareChars = [...share];
  if (local && shareChars.length === 2 && isAlpha(shareChars[0]) && shareChars[1] === "$") {
    return { kind: "abs", path: { prefix: { kind: "drive", letter: shareChars[0].toUpperCase() }, parts }, verbatim };
  }
  return { kind: "abs", path: { prefix: { kind: "unc", server, share }, parts }, verbatim };
}

function windowsParts(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/[\\/]/)) {
    if (raw === "" || raw === ".") continue;
    if (raw === "..") {
      out.push(raw);
      continue;
    }
    const withoutStream = raw.split(":")[0] ?? "";
    const cleaned = withoutStream.replace(/[. ]+$/, "");
    if (cleaned === "") fail("ambiguous path component");
    const stem = asciiLower((cleaned.split(".")[0] ?? "").trimEnd());
    if (RESERVED_DEVICE_NAMES.includes(stem)) fail("reserved device name");
    out.push(cleaned);
  }
  return out;
}

function verbatimParts(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split("\\")) {
    if (raw === "") continue;
    if (raw === "." || raw === "..") fail("dot component inside a verbatim path");
    if (raw.includes(":")) fail("stream or device syntax inside a verbatim path");
    out.push(raw);
  }
  return out;
}

function tryParse(text: string, style: PathStyle): Parsed | null {
  try {
    return parse(text, style);
  } catch (error) {
    if (error instanceof PathError) return null;
    throw error;
  }
}

function joinRaw(base: AbsPath, input: string, style: PathStyle): AbsPath | null {
  const parsed = tryParse(input, style);
  if (parsed === null) return null;
  if (parsed.kind === "abs") return parsed.path;
  if (parsed.kind === "rel") return { prefix: base.prefix, parts: [...base.parts, ...parsed.parts] };
  return { prefix: base.prefix, parts: parsed.parts };
}

function parseAbsolute(path: string, style: PathStyle): AbsPath | null {
  const parsed = tryParse(path, style);
  return parsed !== null && parsed.kind === "abs" ? lexicalOf(parsed.path) : null;
}

/** In-memory filesystem facts for tests and UI previews. */
export class MapFs implements FsFacts {
  private readonly entries = new Set<string>();
  private readonly links = new Map<string, string>();
  private readonly style: PathStyle;

  constructor(style: PathStyle) {
    this.style = style;
  }

  withEntry(path: string): this {
    const parsed = tryParse(path, this.style);
    if (parsed !== null && parsed.kind === "abs") {
      const acc: AbsPath = { prefix: parsed.path.prefix, parts: [] };
      for (const part of parsed.path.parts) {
        acc.parts.push(part);
        this.entries.add(render(acc));
      }
    }
    return this;
  }

  withLink(path: string, target: string): this {
    const parsed = tryParse(path, this.style);
    if (parsed !== null && parsed.kind === "abs") {
      const key = render(parsed.path);
      const parent = parentOf(parsed.path);
      if (parent !== null) this.withEntry(render(parent));
      this.links.set(key, target);
    }
    return this;
  }

  private resolveInner(path: AbsPath, budget: { n: number }): AbsPath | null {
    let acc: AbsPath = { prefix: path.prefix, parts: [] };
    for (const part of path.parts) {
      if (part === "..") {
        acc.parts.pop();
        continue;
      }
      acc.parts.push(part);
      const key = render(acc);
      const target = this.links.get(key);
      if (target !== undefined) {
        if (budget.n === 0) return null;
        budget.n -= 1;
        const base = parentOf(acc) ?? acc;
        const joined = joinRaw(base, target, this.style);
        if (joined === null) return null;
        const next = this.resolveInner(joined, budget);
        if (next === null) return null;
        acc = { prefix: next.prefix, parts: [...next.parts] };
      } else if (!this.entries.has(key)) {
        return null;
      }
    }
    return acc;
  }

  canonicalize(path: string): string | null {
    const parsed = tryParse(path, this.style);
    if (parsed === null || parsed.kind !== "abs") return null;
    const resolved = this.resolveInner(parsed.path, { n: 40 });
    return resolved === null ? null : render(resolved);
  }

  readLink(path: string): string | null {
    const parsed = tryParse(path, this.style);
    if (parsed === null || parsed.kind !== "abs") return null;
    return this.links.get(render(parsed.path)) ?? null;
  }
}

// ---------------------------------------------------------------------------
// Checking

const IGNORABLE = /[­͏​-‏‪-‮⁠-⁤⁪-⁯﻿]/g;

function fold(text: string): string {
  return text.replace(IGNORABLE, "").toLowerCase();
}

function isShortName(part: string): boolean {
  return /~[0-9]/.test(part);
}

export class Checker {
  private readonly style: PathStyle;
  private readonly fs: FsFacts;
  private readonly caseInsensitive: boolean;
  private readonly root: AbsPath | null;
  private readonly homes: AbsPath[];

  constructor(config: SandboxConfig, fs: FsFacts) {
    this.style = config.pathStyle;
    this.fs = fs;
    this.caseInsensitive = config.caseInsensitive || config.pathStyle === "windows";
    const canonical = (path: string): AbsPath | null => {
      const raw = parseAbsolute(path, this.style);
      if (raw === null) return null;
      const real = fs.canonicalize(render(raw));
      return (real === null ? null : parseAbsolute(real, this.style)) ?? raw;
    };
    const root = canonical(config.root);
    this.root = root !== null && root.parts.length > 0 ? root : null;
    this.homes = [];
    if (config.home !== null) {
      const raw = parseAbsolute(config.home, this.style);
      if (raw !== null) this.homes.push(raw);
      const real = canonical(config.home);
      if (real !== null && !this.homes.some((h) => sameAbs(h, real))) this.homes.push(real);
    }
  }

  rootPath(): string | null {
    return this.root === null ? null : render(this.root);
  }

  check(cwd: string | null, input: string): PathCheck {
    try {
      const [lex, canonical] = this.resolve(cwd, input);
      const [zone, note] = this.zoneOf(lex, canonical);
      return { zone, resolved: render(canonical), note };
    } catch (error) {
      if (error instanceof PathError) return { zone: "sensitive", resolved: null, note: error.message };
      throw error;
    }
  }

  checkShellWord(cwd: string | null, input: string): PathCheck {
    const check = this.check(cwd, input);
    if (!/[*?[]/.test(input)) return check;
    const parts = input.split(/[\\/]/);
    const firstGlob = parts.findIndex((part) => /[*?[]/.test(part));
    const dotdotAfterGlob = firstGlob !== -1 && parts.slice(firstGlob).some((part) => part === "..");
    if (check.zone !== "inside" || dotdotAfterGlob) return { ...check, zone: "sensitive", note: "glob pattern outside the workspace" };
    return check;
  }

  isProtected(path: string): boolean {
    const abs = parseAbsolute(path, this.style);
    if (abs === null) return true;
    const parts = this.root !== null && this.within(abs, this.root) ? abs.parts.slice(this.root.parts.length) : abs.parts;
    const last = parts[parts.length - 1];
    return parts.some((part) => {
      const lower = asciiLower(part);
      return lower === ".git" || lower === ".next-agent";
    }) || (last !== undefined && isSecretName(last));
  }

  private resolve(cwd: string | null, input: string): [AbsPath, AbsPath] {
    const expanded = this.expandHome(input);
    if (expanded.startsWith("$") || (this.style === "windows" && expanded.startsWith("%"))) fail("unexpanded variable in path");
    const base = cwd !== null ? this.resolveFrom(this.root, cwd)[1] : this.root;
    return this.resolveFrom(base, expanded);
  }

  private expandHome(input: string): string {
    if (!input.startsWith("~")) return input;
    const rest = input.slice(1);
    const isSep = (c: string): boolean => c === "/" || (this.style === "windows" && c === "\\");
    if (!(rest === "" || isSep(rest[0]))) fail("~user expansion is not supported");
    const home = this.homes[0];
    if (home === undefined) fail("no home directory configured");
    return `${render(home)}${rest}`;
  }

  private resolveFrom(base: AbsPath | null, input: string): [AbsPath, AbsPath] {
    const parsed = parse(input, this.style);
    let raw: AbsPath;
    if (parsed.kind === "abs") raw = parsed.path;
    else if (parsed.kind === "rel") {
      if (base === null) fail("relative path without a usable workspace root");
      raw = { prefix: base.prefix, parts: [...base.parts, ...parsed.parts] };
    } else {
      if (base === null) fail("root-relative path without a usable workspace root");
      raw = { prefix: base.prefix, parts: parsed.parts };
    }
    const lex = lexicalOf(raw);
    let acc: AbsPath = { prefix: raw.prefix, parts: [] };
    let exists = true;
    for (const part of raw.parts) {
      if (part === "..") {
        acc.parts.pop();
        continue;
      }
      acc.parts.push(part);
      if (!exists) continue;
      const rendered = render(acc);
      const real = this.fs.canonicalize(rendered);
      if (real !== null) {
        const next = parseAbsolute(real, this.style);
        if (next === null) fail("unparseable canonical path");
        acc = { prefix: next.prefix, parts: [...next.parts] };
      } else if (this.fs.readLink(rendered) !== null) {
        fail("dangling symlink");
      } else {
        exists = false;
      }
    }
    return [lex, acc];
  }

  private zoneOf(lex: AbsPath, canonical: AbsPath): [Zone, string | null] {
    const note = this.sensitive(lex) ?? this.sensitive(canonical);
    if (note !== null) return ["sensitive", note];
    if (this.style === "windows" && canonical.parts.some(isShortName)) return ["sensitive", "unresolved 8.3 short name"];
    if (this.root !== null && this.within(canonical, this.root)) return ["inside", null];
    return ["outside", null];
  }

  private sensitive(path: AbsPath): string | null {
    if (path.parts.some((part) => SENSITIVE_ANYWHERE.some((name) => this.eqPart(part, name)))) return "credential directory";
    for (const home of this.homes) {
      for (const entry of SENSITIVE_IN_HOME) {
        const prefix: AbsPath = { prefix: home.prefix, parts: [...home.parts, ...entry.split("/")] };
        if (this.within(path, prefix)) return "sensitive location in home";
      }
    }
    const p = path.prefix;
    if (this.style === "posix" && p.kind === "root") {
      const hit = SENSITIVE_POSIX.some((entry) => {
        const prefix = parseAbsolute(entry, this.style);
        return prefix !== null && this.within(path, prefix);
      });
      return hit ? "system location" : null;
    }
    if (this.style === "windows" && p.kind === "drive") {
      const hit = SENSITIVE_WINDOWS.some((entry) => this.within(path, { prefix: p, parts: [entry] }));
      return hit ? "system location" : null;
    }
    return null;
  }

  private within(child: AbsPath, parent: AbsPath): boolean {
    return (
      samePrefix(child.prefix, parent.prefix) &&
      parent.parts.length <= child.parts.length &&
      parent.parts.every((part, i) => this.eqPart(child.parts[i], part))
    );
  }

  private eqPart(a: string, b: string): boolean {
    return this.caseInsensitive ? fold(a) === fold(b) : a === b;
  }
}
