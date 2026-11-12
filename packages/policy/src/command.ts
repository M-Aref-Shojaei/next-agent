// Mirror of crates/permission-engine/src/command.rs. Keep the two in lockstep;
// fixtures/command-corpus.json is the shared contract.

import { isSecretName, SENSITIVE_ANYWHERE } from "./path.ts";

export type CommandClass = "safe_dev" | "unknown" | "deploy" | "network_remote" | "sensitive" | "catastrophic";

export const RANK: Record<CommandClass, number> = {
  safe_dev: 0,
  unknown: 1,
  deploy: 2,
  network_remote: 3,
  sensitive: 4,
  catastrophic: 5,
};

export function autoForbidden(cls: CommandClass): boolean {
  return cls === "sensitive" || cls === "network_remote" || cls === "deploy" || cls === "catastrophic";
}

export interface PathArg {
  text: string;
  write: boolean;
}

export interface Analysis {
  class: CommandClass;
  commands: string[][];
  paths: PathArg[];
  opaque: boolean;
}

const worst = (a: CommandClass, b: CommandClass): CommandClass => (RANK[b] > RANK[a] ? b : a);

export function classify(input: string): CommandClass {
  return analyze(input).class;
}

export function analyze(input: string): Analysis {
  const out: Analysis = { class: "safe_dev", commands: [], paths: [], opaque: false };
  analyzeInto(input, 0, out);
  if (hasForkBomb(input)) out.class = "catastrophic";
  return out;
}

export function splitSegments(input: string): string[] {
  return parse(input, 0)
    .commands.map((cmd) => cmd.words.map((w) => w.text).join(" "))
    .filter((text) => text !== "");
}

const MAX_DEPTH = 12;

function analyzeInto(input: string, depth: number, out: Analysis): void {
  if (depth > MAX_DEPTH) {
    out.class = worst(out.class, "unknown");
    out.opaque = true;
    return;
  }
  const script = parse(input, depth);
  if (script.opaque) {
    out.opaque = true;
    out.class = worst(out.class, "unknown");
  }
  for (const cmd of script.commands) {
    // classifyCommand may raise out.class itself (inline scripts); read after.
    const cls = classifyCommand(cmd, script, depth, out);
    out.class = worst(out.class, cls);
  }
}

// ---------------------------------------------------------------------------
// Parser

interface Word {
  text: string;
  dynamic: boolean;
  quoted: boolean;
}

type RedirectKind = "input" | "output" | "heredoc" | "heredoc-strip" | "herestring" | "dup";

interface Redirect {
  kind: RedirectKind;
  target: Word;
}

interface SimpleCommand {
  words: Word[];
  redirects: Redirect[];
  heredocBody: string | null;
  pipeFrom: number | null;
}

interface Script {
  commands: SimpleCommand[];
  opaque: boolean;
}

type Stop = "eof" | "paren" | "backtick";

const newWord = (): Word => ({ text: "", dynamic: false, quoted: false });
const newCommand = (): SimpleCommand => ({ words: [], redirects: [], heredocBody: null, pipeFrom: null });

interface Level {
  cur: SimpleCommand;
  word: Word | null;
  pendingRedirect: RedirectKind | null;
  heredocOwners: [number, string, boolean][];
  pendingHeredocs: [string, boolean][];
  lastIndex: number | null;
  piped: boolean;
}

function parse(input: string, depth: number): Script {
  const parser = new Parser([...input], depth);
  parser.parseList("eof");
  return parser.script;
}

class Parser {
  chars: string[];
  pos = 0;
  depth: number;
  script: Script = { commands: [], opaque: false };

  constructor(chars: string[], depth: number) {
    this.chars = chars;
    this.depth = depth;
  }

  peek(offset = 0): string | undefined {
    return this.chars[this.pos + offset];
  }

  parseList(stop: Stop): void {
    if (this.depth > MAX_DEPTH) {
      this.script.opaque = true;
      this.pos = this.chars.length;
      return;
    }
    const level: Level = {
      cur: newCommand(),
      word: null,
      pendingRedirect: null,
      heredocOwners: [],
      pendingHeredocs: [],
      lastIndex: null,
      piped: false,
    };
    for (;;) {
      const ch = this.peek();
      if (ch === undefined) {
        if (stop !== "eof") this.script.opaque = true;
        this.finishCommand(level);
        if (level.heredocOwners.length > 0) this.script.opaque = true;
        return;
      }
      if (ch === ")" && stop === "paren") {
        this.pos += 1;
        this.finishCommand(level);
        return;
      }
      if (ch === "`" && stop === "backtick") {
        this.pos += 1;
        this.finishCommand(level);
        return;
      }
      switch (ch) {
        case " ":
        case "\t":
          this.pos += 1;
          endWord(level);
          break;
        case "\n":
          this.pos += 1;
          this.finishCommand(level);
          level.piped = false;
          this.readHeredocBodies(level);
          break;
        case "#":
          if (level.word === null) {
            while (this.peek() !== undefined && this.peek() !== "\n") this.pos += 1;
          } else {
            this.pos += 1;
            level.word.text += ch;
          }
          break;
        case "\\": {
          this.pos += 1;
          const next = this.peek();
          if (next === "\n") this.pos += 1;
          else if (next !== undefined) {
            this.pos += 1;
            level.word ??= newWord();
            level.word.text += next;
            level.word.quoted = true;
          }
          break;
        }
        case "'": {
          this.pos += 1;
          level.word ??= newWord();
          level.word.quoted = true;
          let closed = false;
          while (this.peek() !== undefined) {
            const c = this.peek()!;
            this.pos += 1;
            if (c === "'") {
              closed = true;
              break;
            }
            level.word.text += c;
          }
          if (!closed) this.script.opaque = true;
          break;
        }
        case '"': {
          this.pos += 1;
          const word = level.word ?? newWord();
          level.word = null;
          word.quoted = true;
          this.readDoubleQuoted(word, true);
          level.word = word;
          break;
        }
        case "$": {
          const word = level.word ?? newWord();
          level.word = null;
          this.readDollar(word);
          level.word = word;
          break;
        }
        case "`": {
          this.pos += 1;
          this.nested("backtick");
          level.word ??= newWord();
          level.word.text += "`…`";
          level.word.dynamic = true;
          break;
        }
        case "(": {
          this.pos += 1;
          const inWord = level.word !== null && level.word.text !== "";
          if (inWord && this.peek() === ")") {
            this.pos += 1;
            level.word = null;
            this.finishCommand(level);
          } else if (inWord) {
            this.script.opaque = true;
            level.word!.text += "(";
          } else {
            this.finishCommand(level);
            this.nested("paren");
          }
          break;
        }
        case ")":
          this.pos += 1;
          this.script.opaque = true;
          this.finishCommand(level);
          break;
        case "<":
        case ">":
          this.readRedirect(level, ch);
          break;
        case "&":
          if (this.peek(1) === ">") {
            this.pos += 2;
            if (this.peek() === ">") this.pos += 1;
            endWord(level);
            level.pendingRedirect = "output";
          } else {
            this.pos += 1;
            if (this.peek() === "&") this.pos += 1;
            this.finishCommand(level);
            level.piped = false;
          }
          break;
        case "|": {
          this.pos += 1;
          let piped = true;
          if (this.peek() === "|") {
            this.pos += 1;
            piped = false;
          } else if (this.peek() === "&") {
            this.pos += 1;
          }
          this.finishCommand(level);
          level.piped = piped;
          break;
        }
        case ";":
          this.pos += 1;
          while (this.peek() === ";" || this.peek() === "&") this.pos += 1;
          this.finishCommand(level);
          level.piped = false;
          break;
        default:
          this.pos += 1;
          level.word ??= newWord();
          level.word.text += ch;
      }
    }
  }

  nested(stop: Stop): void {
    this.depth += 1;
    this.parseList(stop);
    this.depth -= 1;
  }

  finishCommand(level: Level): void {
    endWord(level);
    if (level.pendingRedirect !== null) {
      level.pendingRedirect = null;
      this.script.opaque = true;
    }
    const cmd = level.cur;
    level.cur = newCommand();
    const heredocs = level.pendingHeredocs;
    level.pendingHeredocs = [];
    if (cmd.words.length === 0 && cmd.redirects.length === 0) return;
    if (level.piped) cmd.pipeFrom = level.lastIndex;
    level.piped = false;
    this.script.commands.push(cmd);
    const index = this.script.commands.length - 1;
    level.lastIndex = index;
    for (const [delimiter, stripTabs] of heredocs) level.heredocOwners.push([index, delimiter, stripTabs]);
  }

  readHeredocBodies(level: Level): void {
    const owners = level.heredocOwners;
    level.heredocOwners = [];
    for (const [index, delimiter, stripTabs] of owners) {
      let body = "";
      let closed = false;
      while (this.pos < this.chars.length) {
        const start = this.pos;
        while (this.pos < this.chars.length && this.chars[this.pos] !== "\n") this.pos += 1;
        const line = this.chars.slice(start, this.pos).join("");
        if (this.pos < this.chars.length) this.pos += 1;
        const candidate = stripTabs ? line.replace(/^\t+/, "") : line;
        if (candidate === delimiter) {
          closed = true;
          break;
        }
        body += line + "\n";
      }
      if (!closed) this.script.opaque = true;
      const sub = new Parser([...body], this.depth + 1);
      sub.readDoubleQuoted(newWord(), false);
      const offset = this.script.commands.length;
      for (const cmd of sub.script.commands) {
        if (cmd.pipeFrom !== null) cmd.pipeFrom += offset;
        this.script.commands.push(cmd);
      }
      if (sub.script.opaque) this.script.opaque = true;
      const owner = this.script.commands[index];
      if (owner !== undefined) owner.heredocBody = body;
    }
  }

  readRedirect(level: Level, first: string): void {
    const w = level.word;
    if (w !== null && !w.quoted && w.text !== "" && /^[0-9]+$/.test(w.text)) level.word = null;
    else endWord(level);
    this.pos += 1;
    const next = this.peek();
    let kind: RedirectKind;
    if (next === "(") {
      this.pos += 1;
      this.nested("paren");
      level.word ??= newWord();
      level.word.text += "<(…)";
      level.word.dynamic = true;
      return;
    } else if ((first === ">" && (next === ">" || next === "|")) || (first === "<" && next === ">")) {
      this.pos += 1;
      kind = "output";
    } else if (next === "&") {
      this.pos += 1;
      const after = this.peek();
      const digitsOrDash = after !== undefined && /^[0-9-]$/.test(after);
      kind = digitsOrDash ? "dup" : first === ">" ? "output" : "input";
    } else if (first === "<" && next === "<") {
      this.pos += 1;
      if (this.peek() === "<") {
        this.pos += 1;
        kind = "herestring";
      } else if (this.peek() === "-") {
        this.pos += 1;
        kind = "heredoc-strip";
      } else {
        kind = "heredoc";
      }
    } else if (first === ">") {
      kind = "output";
    } else {
      kind = "input";
    }
    level.pendingRedirect = kind;
    while (this.peek() === " " || this.peek() === "\t") this.pos += 1;
  }

  readDoubleQuoted(word: Word, terminated: boolean): void {
    for (;;) {
      const c = this.peek();
      if (c === undefined) {
        if (terminated) this.script.opaque = true;
        return;
      }
      if (c === '"' && terminated) {
        this.pos += 1;
        return;
      }
      if (c === "\\") {
        this.pos += 1;
        const next = this.peek();
        if (next === "$" || next === "`" || next === '"' || next === "\\") {
          this.pos += 1;
          word.text += next;
        } else if (next === "\n") {
          this.pos += 1;
        } else if (next !== undefined) {
          this.pos += 1;
          word.text += "\\" + next;
        } else {
          word.text += "\\";
        }
      } else if (c === "$") {
        this.readDollar(word);
      } else if (c === "`") {
        this.pos += 1;
        this.nested("backtick");
        word.text += "`…`";
        word.dynamic = true;
      } else {
        this.pos += 1;
        word.text += c;
      }
    }
  }

  readDollar(word: Word): void {
    this.pos += 1;
    const c = this.peek();
    if (c === "(") {
      this.pos += 1;
      this.nested("paren");
      word.text += "$(…)";
      word.dynamic = true;
    } else if (c === "{") {
      this.pos += 1;
      word.text += "${";
      word.dynamic = true;
      let depth = 1;
      for (;;) {
        const d = this.peek();
        if (d === undefined) {
          this.script.opaque = true;
          return;
        }
        if (d === "}") {
          this.pos += 1;
          depth -= 1;
          word.text += "}";
          if (depth === 0) return;
        } else if (d === "{") {
          this.pos += 1;
          depth += 1;
          word.text += "{";
        } else if (d === "$") {
          this.readDollar(word);
        } else if (d === "`") {
          this.pos += 1;
          this.nested("backtick");
        } else if (d === '"') {
          this.pos += 1;
          this.readDoubleQuoted(word, true);
        } else if (d === "'") {
          this.pos += 1;
          while (this.peek() !== undefined) {
            const q = this.peek()!;
            this.pos += 1;
            if (q === "'") break;
            word.text += q;
          }
        } else if (d === "\\") {
          this.pos += 1;
          const n = this.peek();
          if (n !== undefined) {
            this.pos += 1;
            word.text += n;
          }
        } else {
          this.pos += 1;
          word.text += d;
        }
      }
    } else if (c === "'") {
      this.pos += 1;
      word.dynamic = true;
      word.quoted = true;
      word.text += "$'";
      let closed = false;
      while (this.peek() !== undefined) {
        const d = this.peek()!;
        this.pos += 1;
        if (d === "\\") {
          const n = this.peek();
          if (n !== undefined) {
            this.pos += 1;
            word.text += "\\" + n;
          }
          continue;
        }
        if (d === "'") {
          closed = true;
          break;
        }
        word.text += d;
      }
      if (!closed) this.script.opaque = true;
    } else if (c === '"') {
      this.pos += 1;
      word.quoted = true;
      this.readDoubleQuoted(word, true);
    } else if (c !== undefined && /^[A-Za-z0-9_]$/.test(c)) {
      word.dynamic = true;
      word.text += "$";
      while (this.peek() !== undefined && /^[A-Za-z0-9_]$/.test(this.peek()!)) {
        word.text += this.peek();
        this.pos += 1;
      }
    } else if (c !== undefined && "?#@*!$-".includes(c)) {
      this.pos += 1;
      word.dynamic = true;
      word.text += "$" + c;
    } else {
      word.text += "$";
    }
  }
}

function endWord(level: Level): void {
  const word = level.word;
  if (word === null) return;
  level.word = null;
  const kind = level.pendingRedirect;
  if (kind !== null) {
    level.pendingRedirect = null;
    if (kind === "heredoc" || kind === "heredoc-strip") level.pendingHeredocs.push([word.text, kind === "heredoc-strip"]);
    level.cur.redirects.push({ kind, target: word });
    return;
  }
  level.cur.words.push(word);
}

// ---------------------------------------------------------------------------
// Fork bombs

function hasForkBomb(input: string): boolean {
  const chars = [...input];
  const boundary = (c: string): boolean => /\s/.test(c) || ";&|{}()".includes(c);
  const skipBlank = (index: number): number => {
    while (chars[index] === " " || chars[index] === "\t") index += 1;
    return index;
  };
  const names: string[] = [];
  chars.forEach((ch, index) => {
    if (ch !== "(") return;
    const close = skipBlank(index + 1);
    if (chars[close] === ")" && chars[skipBlank(close + 1)] === "{") {
      let start = index;
      while (start > 0 && chars[start - 1] === " ") start -= 1;
      const end = start;
      while (start > 0 && !boundary(chars[start - 1])) start -= 1;
      if (start < end) names.push(chars.slice(start, end).join(""));
    }
  });
  const words = input.split(/[\s;]/).filter((w) => w !== "");
  for (let i = 0; i + 1 < words.length; i += 1) {
    if (words[i] === "function") {
      const name = words[i + 1].replace(/[{()]+$/, "");
      if (name !== "") names.push(name);
    }
  }
  const compact = input.replace(/\s/g, "");
  return names.some((name) => compact.includes(`${name}|${name}&`) || compact.includes(`${name}|${name};`));
}

// ---------------------------------------------------------------------------
// Tables (mirror command.rs)

const READ_ONLY = [
  "ls", "cat", "head", "tail", "wc", "rg", "grep", "fd", "find", "tree", "file", "stat", "which", "echo", "printf", "pwd",
  "true", "false", "sort", "uniq", "diff", "basename", "dirname", "realpath", "test", "[",
];

const PATH_OPERAND_PROGRAMS = [
  "ls", "cat", "head", "tail", "wc", "rg", "grep", "fd", "find", "tree", "file", "stat", "sort", "uniq", "diff", "realpath",
  "cd", "pushd",
];

const SAFE_TOOLS = ["tsc", "vitest", "jest", "pytest", "eslint", "prettier", "ruff", "mypy"];

const SAFE_SUBCOMMANDS: Record<string, string[]> = {
  pnpm: ["test", "t", "lint", "build", "run", "install", "i", "fmt", "check", "typecheck"],
  npm: ["test", "t", "run", "run-script", "install", "i", "ci", "ls"],
  yarn: ["test", "run", "install", "build", "lint"],
  bun: ["test", "run", "install", "i", "build"],
  cargo: ["build", "b", "test", "t", "check", "c", "clippy", "fmt", "run", "r", "doc", "bench", "metadata", "tree", "nextest"],
  git: ["status", "diff", "log", "show", "add", "commit", "rev-parse", "blame", "ls-files", "describe"],
  uv: ["sync", "lock"],
  go: ["build", "test", "vet"],
};

const INSTALL_VERBS = ["install", "i", "ci", "add"];
const SAFE_PYTHON_MODULES = ["pytest", "unittest", "mypy", "ruff", "black"];
const SAFE_ENV = ["CI", "NODE_ENV", "RUST_BACKTRACE", "RUST_LOG", "DEBUG", "FORCE_COLOR", "NO_COLOR", "TZ", "LANG", "LC_ALL", "TERM", "COLUMNS"];

const SENSITIVE_PROGRAMS = [
  "sudo", "su", "doas", "pkexec", "run0", "apt", "apt-get", "dpkg", "dnf", "yum", "rpm", "pacman", "zypper", "apk", "emerge",
  "snap", "flatpak", "brew", "port", "mas", "softwareupdate", "choco", "winget", "scoop", "systemctl", "service", "launchctl",
  "crontab", "at", "defaults", "reg", "regedit", "sc", "schtasks", "bcdedit", "icacls", "takeown", "setx", "wmic", "vssadmin",
  "net", "netsh", "iptables", "ip6tables", "ufw", "firewall-cmd", "pfctl", "nft", "diskutil", "security", "csrutil", "spctl",
  "tccutil", "nvram", "bless", "kextload", "kextunload", "kmutil", "dscl", "sysadminctl", "passwd", "chpasswd", "visudo",
  "useradd", "usermod", "userdel", "groupadd", "mount", "umount", "modprobe", "insmod", "rmmod", "sysctl", "setenforce",
  "chattr", "xattr", "osascript", "chmod", "chown", "chgrp", "killall", "pkill",
];

const NETWORK_PROGRAMS = [
  "ssh", "scp", "sftp", "ssh-copy-id", "sshpass", "autossh", "mosh", "telnet", "nc", "netcat", "ncat", "socat", "ngrok",
  "cloudflared", "tailscale", "frpc", "bore", "zrok", "lt", "rclone", "ftp", "tftp", "curl", "wget", "aria2c", "http", "https",
  "xh", "invoke-webrequest", "iwr", "invoke-restmethod", "irm", "bitsadmin", "certutil",
];

const DEPLOY_PAIRS: Record<string, string[]> = {
  git: ["push"],
  vercel: ["deploy", "--prod", "promote"],
  fly: ["deploy", "launch"],
  flyctl: ["deploy", "launch"],
  kubectl: ["apply", "create", "delete", "patch", "replace", "rollout", "scale", "drain", "set", "edit"],
  helm: ["upgrade", "install", "uninstall", "rollback"],
  terraform: ["apply", "destroy", "import"],
  tofu: ["apply", "destroy", "import"],
  pulumi: ["up", "destroy"],
  netlify: ["deploy"],
  heroku: ["deploy", "pg:push", "releases:rollback"],
  aws: ["s3", "cloudformation", "ecs", "lambda", "deploy"],
  gcloud: ["deploy", "app", "run"],
  firebase: ["deploy"],
  wrangler: ["deploy", "publish"],
  serverless: ["deploy"],
  sls: ["deploy"],
  railway: ["up"],
  docker: ["push"],
  podman: ["push"],
  npm: ["publish", "unpublish"],
  pnpm: ["publish"],
  yarn: ["publish"],
  bun: ["publish"],
  cargo: ["publish", "yank"],
  twine: ["upload"],
  gh: ["release", "merge"],
};

const DEPLOY_PROGRAMS = ["ansible-playbook"];
const SHELLS = ["sh", "bash", "zsh", "dash", "ksh", "mksh", "fish", "ash", "yash", "tcsh", "csh"];
const BLOCK_DEVICE_PREFIXES = ["/dev/sd", "/dev/hd", "/dev/vd", "/dev/xvd", "/dev/nvme", "/dev/disk", "/dev/rdisk", "/dev/mmcblk", "/dev/mapper", "/dev/md", "/dev/dm-"];
const HARMLESS_DEVICES = ["/dev/null", "/dev/stdin", "/dev/stdout", "/dev/stderr", "/dev/tty", "/dev/zero", "/dev/random", "/dev/urandom"];

const own = <T>(record: Record<string, T>, key: string): T | undefined =>
  Object.prototype.hasOwnProperty.call(record, key) ? record[key] : undefined;

// ---------------------------------------------------------------------------
// Simple-command classification

function programName(word: string): string {
  const parts = word.split(/[\\/]/);
  const base = (parts[parts.length - 1] ?? word).toLowerCase();
  for (const ext of [".exe", ".cmd", ".bat", ".com"]) {
    if (base.endsWith(ext) && base.length > ext.length) return base.slice(0, -ext.length);
  }
  return base;
}

function trustedProgramPath(word: string): boolean {
  if (!/[\\/]/.test(word)) return true;
  return ["/bin/", "/usr/bin/", "/usr/local/bin/", "/sbin/", "/usr/sbin/", "/opt/homebrew/bin/"].some((dir) => {
    if (!word.startsWith(dir)) return false;
    const rest = word.slice(dir.length);
    return rest !== "" && !/[\\/]/.test(rest);
  });
}

function assignmentName(word: Word): string | null {
  const at = word.text.indexOf("=");
  if (at === -1) return null;
  const name = word.text.slice(0, at);
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) ? name : null;
}

const isPython = (program: string): boolean => program.startsWith("python") && /^[0-9.]*$/.test(program.slice(6));

function at(words: Word[], i: number, offset: number): string | undefined {
  return words[i + offset]?.text;
}

function classifyCommand(cmd: SimpleCommand, script: Script, depth: number, out: Analysis): CommandClass {
  let cls: CommandClass = "safe_dev";
  const raise = (other: CommandClass): void => {
    cls = worst(cls, other);
  };

  for (const redirect of cmd.redirects) {
    if (redirect.kind !== "input" && redirect.kind !== "output") continue;
    const target = redirect.target;
    const write = redirect.kind === "output";
    if (write && isBlockDevice(target.text)) raise("catastrophic");
    if (target.dynamic) {
      raise("unknown");
      continue;
    }
    if (HARMLESS_DEVICES.includes(target.text) || target.text.startsWith("/dev/fd/")) continue;
    out.paths.push({ text: target.text, write });
  }

  const words = cmd.words;
  let i = 0;
  while (i < words.length) {
    const name = assignmentName(words[i]);
    if (name === null) break;
    if (!SAFE_ENV.includes(name)) raise("unknown");
    collectWordPath(words[i], out);
    i += 1;
  }
  reserved: while (i < words.length && !words[i].quoted) {
    switch (words[i].text) {
      case "!": case "{": case "}": case "if": case "then": case "else": case "elif": case "fi": case "do": case "done":
      case "while": case "until": case "esac":
        i += 1;
        break;
      case "for": case "select": case "case": case "function": case "in": case "[[":
        for (const word of words.slice(i)) {
          collectWordPath(word, out);
          if (word.dynamic) raise("unknown");
        }
        return cls;
      default:
        break reserved;
    }
  }
  if (i >= words.length) return cls;
  for (const word of words.slice(i)) collectWordPath(word, out);

  for (;;) {
    if (i >= words.length) return cls;
    if (words[i].dynamic) {
      raise("unknown");
      out.commands.push(words.slice(i).map((w) => w.text));
      return cls;
    }
    const program = programName(words[i].text);
    if (["sudo", "doas", "pkexec", "run0"].includes(program)) {
      raise("sensitive");
      i += 1;
      for (;;) {
        const flag = words[i]?.text;
        if (flag === undefined) break;
        if (flag === "--") {
          i += 1;
          break;
        }
        if (!flag.startsWith("-") || flag === "-") break;
        const takesValue = ["-u", "-g", "-p", "-U", "-C", "-h", "-r", "-t", "-D", "-R", "-T", "--user", "--group", "--prompt", "--chdir", "--host"].includes(flag);
        i += takesValue ? 2 : 1;
      }
    } else if (program === "su") {
      raise("sensitive");
      const rest = words.slice(i + 1);
      const pos = rest.findIndex((w) => w.text === "-c" || w.text === "--command");
      if (pos !== -1 && rest[pos + 1] !== undefined) analyzeInto(rest[pos + 1].text, depth + 1, out);
      return cls;
    } else if (program === "env") {
      i += 1;
      for (;;) {
        const word = words[i];
        if (word === undefined) {
          raise("unknown");
          return cls;
        }
        const text = word.text;
        if (text === "--") {
          i += 1;
          break;
        } else if (["-i", "-0", "-", "-v", "--ignore-environment", "--null", "--debug"].includes(text)) {
          i += 1;
        } else if (text === "-u" || text === "--unset") {
          i += 2;
        } else if (text === "-C" || text === "--chdir") {
          raise("unknown");
          i += 2;
        } else if (text === "-S" || text === "--split-string") {
          raise("unknown");
          analyzeInto(words.slice(i + 1).map((w) => w.text).join(" "), depth + 1, out);
          return cls;
        } else if (text.startsWith("-S")) {
          raise("unknown");
          let joined = text.slice(2);
          for (const w of words.slice(i + 1)) joined += " " + w.text;
          analyzeInto(joined, depth + 1, out);
          return cls;
        } else if (text.startsWith("-")) {
          raise("unknown");
          i += 1;
        } else {
          const name = assignmentName(word);
          if (name === null) break;
          if (!SAFE_ENV.includes(name)) raise("unknown");
          i += 1;
        }
      }
    } else if (program === "command") {
      const next = at(words, i, 1);
      if (next === "-v" || next === "-V") return cls;
      i += 1;
      while (at(words, i, 0) === "-p") i += 1;
    } else if (["builtin", "nohup", "unbuffer", "chronic", "busybox", "toybox"].includes(program)) {
      i += 1;
    } else if (program === "exec") {
      i += 1;
      for (;;) {
        const flag = at(words, i, 0);
        if (flag === "-a") i += 2;
        else if (flag === "-c" || flag === "-l") i += 1;
        else break;
      }
    } else if (program === "time") {
      i += 1;
      for (;;) {
        const flag = at(words, i, 0);
        if (flag === undefined) break;
        if (flag === "-o" || flag === "--output") {
          raise("unknown");
          i += 2;
        } else if (flag.startsWith("-")) i += 1;
        else break;
      }
    } else if (["nice", "ionice", "caffeinate", "stdbuf"].includes(program)) {
      const valueFlags = program === "nice" ? ["-n"] : program === "ionice" ? ["-c", "-n", "-p"] : program === "caffeinate" ? ["-t", "-w"] : ["-i", "-o", "-e"];
      i += 1;
      for (;;) {
        const flag = at(words, i, 0);
        if (flag === undefined || !flag.startsWith("-")) break;
        i += valueFlags.includes(flag) ? 2 : 1;
      }
    } else if (program === "timeout" || program === "gtimeout") {
      i += 1;
      for (;;) {
        const flag = at(words, i, 0);
        if (flag === undefined || !flag.startsWith("-")) break;
        i += ["-s", "-k", "--signal", "--kill-after"].includes(flag) ? 2 : 1;
      }
      i += 1;
    } else if (program === "xargs") {
      raise("unknown");
      i += 1;
      for (;;) {
        const flag = at(words, i, 0);
        if (flag === undefined || !flag.startsWith("-")) break;
        i += ["-n", "-L", "-P", "-I", "-d", "-E", "-e", "-s", "-a", "-R", "-S"].includes(flag) ? 2 : 1;
      }
      if (i >= words.length) return cls;
    } else if (program === "watch") {
      raise("unknown");
      i += 1;
      for (;;) {
        const flag = at(words, i, 0);
        if (flag === undefined || !flag.startsWith("-")) break;
        i += flag === "-n" || flag === "--interval" ? 2 : 1;
      }
      analyzeInto(words.slice(Math.min(i, words.length)).map((w) => w.text).join(" "), depth + 1, out);
      return cls;
    } else if (program === "uv" && at(words, i, 1) === "run") {
      i += 2;
      if (at(words, i, 0)?.startsWith("-")) {
        raise("unknown");
        return cls;
      }
    } else if ((program === "pnpm" || program === "yarn" || program === "npm") && at(words, i, 1) === "exec") {
      i += 2;
      if (at(words, i, 0) === "--") i += 1;
      else if (at(words, i, 0)?.startsWith("-")) {
        raise("unknown");
        return cls;
      }
    } else if (["npx", "pnpx", "bunx", "uvx"].includes(program)) {
      raise("unknown");
      i += 1;
      while (at(words, i, 0)?.startsWith("-")) i += 1;
    } else if ((program === "pnpm" || program === "bun") && (at(words, i, 1) === "dlx" || at(words, i, 1) === "x")) {
      raise("unknown");
      i += 2;
      while (at(words, i, 0)?.startsWith("-")) i += 1;
    } else {
      break;
    }
  }
  if (i >= words.length) return cls;

  const head = words[i];
  const program = programName(head.text);
  const rest = words.slice(i + 1);
  const args = rest.map((w) => w.text);
  out.commands.push([program, ...args]);

  if (PATH_OPERAND_PROGRAMS.includes(program)) {
    for (const word of rest) {
      if (!word.dynamic && !word.text.startsWith("-") && word.text !== "") out.paths.push({ text: word.text, write: false });
    }
  }
  if (!trustedProgramPath(head.text)) raise("unknown");
  if (rest.some((w) => w.dynamic)) raise("unknown");
  raise(classifyProgram(program, args, cmd, script, depth, out));
  return cls;
}

function classifyProgram(program: string, args: string[], cmd: SimpleCommand, script: Script, depth: number, out: Analysis): CommandClass {
  if (isCatastrophic(program, args)) return "catastrophic";
  const inline = inlineScript(program, args, cmd, script, depth, out);
  if (inline !== null) return inline;
  if (program === "systemctl" && args.some((a) => ["poweroff", "reboot", "halt", "kexec"].includes(a))) return "catastrophic";
  if (program === "cd" || program === "pushd") {
    const targets = operands(args);
    return targets.length === 1 && targets[0] !== "-" ? "safe_dev" : "unknown";
  }
  if (SENSITIVE_PROGRAMS.includes(program)) return "sensitive";
  if (NETWORK_PROGRAMS.includes(program)) return "network_remote";
  if (program === "rsync" && args.some((a) => !a.startsWith("-") && (a.includes(":") || a.startsWith("rsync://")))) return "network_remote";
  if (
    ["npm", "pnpm", "yarn", "bun"].includes(program) &&
    (args.some((a) => a === "-g" || a === "--global" || a === "--location=global") || args[0] === "global")
  ) {
    return "sensitive";
  }
  if (isDeploy(program, args)) return "deploy";
  return isSafe(program, args) ? "safe_dev" : "unknown";
}

function inlineScript(program: string, args: string[], cmd: SimpleCommand, script: Script, depth: number, out: Analysis): CommandClass | null {
  const inner = (text: string): CommandClass => {
    const sub: Analysis = { class: "safe_dev", commands: [], paths: [], opaque: false };
    analyzeInto(text, depth + 1, sub);
    if (hasForkBomb(text)) sub.class = "catastrophic";
    out.commands.push(...sub.commands);
    out.paths.push(...sub.paths);
    if (sub.opaque) out.opaque = true;
    return worst(sub.class, "unknown");
  };

  if (SHELLS.includes(program)) {
    let hasC = false;
    let idx = 0;
    while (idx < args.length) {
      const a = args[idx];
      if (a === "--" || a === "-") {
        idx += 1;
        break;
      }
      if (["-o", "+o", "-O", "+O", "--rcfile", "--init-file"].includes(a)) {
        idx += 2;
        continue;
      }
      if (a.startsWith("--")) {
        idx += 1;
        continue;
      }
      if ((a.startsWith("-") || a.startsWith("+")) && a.length > 1) {
        if (a.startsWith("-") && a.slice(1).includes("c")) hasC = true;
        idx += 1;
        continue;
      }
      break;
    }
    if (hasC) return args[idx] !== undefined ? inner(args[idx]) : "unknown";
    if (idx < args.length) return "unknown";
    const text = stdinScript(cmd, script);
    return text !== null ? inner(text) : "unknown";
  }

  if (program === "cmd") {
    const pos = args.findIndex((a) => ["/c", "/k", "/r"].includes(a.toLowerCase()));
    return pos !== -1 ? inner(args.slice(pos + 1).join(" ")) : "unknown";
  }

  if (program === "powershell" || program === "pwsh") {
    for (let pos = 0; pos < args.length; pos += 1) {
      const lower = args[pos].toLowerCase();
      if (lower.length >= 2 && "-command".startsWith(lower)) return inner(args.slice(pos + 1).join(" "));
      if (lower.length >= 2 && ("-encodedcommand".startsWith(lower) || lower === "-ec" || lower === "-enc")) return "unknown";
    }
    return "unknown";
  }

  if (program === "eval") return inner(args.join(" "));
  if (program === "source" || program === ".") return "unknown";

  let inlineFlags: string[];
  if (isPython(program)) inlineFlags = ["c"];
  else if (program === "node" || program === "nodejs") inlineFlags = ["e", "p", "-eval", "-print"];
  else if (program === "perl" || program === "ruby") inlineFlags = ["e", "E"];
  else if (program === "php") inlineFlags = ["r"];
  else if (program === "lua" || program === "luajit" || program === "rscript") inlineFlags = ["e"];
  else if (program === "bun") inlineFlags = ["e", "-eval", "p", "-print"];
  else if (program === "deno") return args[0] === "eval" ? inner(args.slice(1).join(" ")) : null;
  else return null;

  for (let pos = 0; pos < args.length; pos += 1) {
    const a = args[pos];
    if (!a.startsWith("-")) break;
    const body = a.slice(1);
    const long = body.startsWith("-");
    const matched = long
      ? inlineFlags.includes(body.split("=")[0])
      : [...body].some((c) => inlineFlags.some((f) => f.length === 1 && f === c));
    if (matched) {
      let text: string | undefined;
      if (long) {
        const eq = body.indexOf("=");
        text = eq !== -1 ? body.slice(eq + 1) : args[pos + 1];
      } else {
        text = args[pos + 1];
      }
      return text !== undefined ? inner(text) : "unknown";
    }
  }
  return null;
}

function stdinScript(cmd: SimpleCommand, script: Script): string | null {
  for (const redirect of cmd.redirects) if (redirect.kind === "herestring") return redirect.target.text;
  if (cmd.heredocBody !== null) return cmd.heredocBody;
  if (cmd.pipeFrom === null) return null;
  const source = script.commands[cmd.pipeFrom];
  if (source === undefined) return null;
  if (source.heredocBody !== null) return source.heredocBody;
  const first = source.words[0];
  if (first === undefined) return null;
  const program = programName(first.text);
  if (program === "echo" || program === "printf") {
    return source.words
      .slice(1)
      .filter((w) => !w.text.startsWith("-"))
      .map((w) => w.text)
      .join(" ")
      .replaceAll("\\n", "\n");
  }
  return null;
}

function isDeploy(program: string, args: string[]): boolean {
  if (DEPLOY_PROGRAMS.includes(program)) return true;
  if (args.some((a) => a === "--prod" || a === "--production")) return true;
  const verbs = own(DEPLOY_PAIRS, program);
  return verbs !== undefined && args.some((a) => verbs.includes(a));
}

function isSafe(program: string, args: string[]): boolean {
  if (READ_ONLY.includes(program)) {
    const blocked: string[] =
      program === "find" ? ["-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls"]
      : program === "fd" ? ["-x", "--exec", "-X", "--exec-batch"]
      : program === "rg" ? ["--pre"]
      : program === "tree" ? ["-o"]
      : program === "file" ? ["-C", "--compile"]
      : program === "sort" ? ["-o", "--output"]
      : [];
    return !args.some(
      (a) =>
        blocked.includes(a) ||
        blocked.some((b) => b.startsWith("--") && a.startsWith(`${b}=`)) ||
        (program === "sort" && a.startsWith("-o")),
    );
  }
  if (SAFE_TOOLS.includes(program)) return true;
  if (isPython(program)) return args[0] === "-m" && args[1] !== undefined && SAFE_PYTHON_MODULES.includes(args[1]);
  if (program === "node") return args[0] === "--test";
  const verbs = own(SAFE_SUBCOMMANDS, program);
  if (verbs === undefined) return false;
  const first = args[0];
  if (first === undefined) return true;
  if (!verbs.includes(first)) return false;
  if (program === "git" && args.some((a) => a.startsWith("--output") || a === "--ext-diff")) return false;
  if (INSTALL_VERBS.includes(first)) return args.slice(1).every((a) => a.startsWith("-"));
  return true;
}

function isRootTarget(arg: string): boolean {
  let text = arg.replaceAll("\\", "/");
  for (const home of ["$HOME", "${HOME}", "$USERPROFILE", "${USERPROFILE}", "%USERPROFILE%", "%HOMEPATH%"]) {
    if (text.startsWith(home)) text = "~" + text.slice(home.length);
  }
  while (text.includes("//")) text = text.replaceAll("//", "/");
  while (text.includes("/./")) text = text.replaceAll("/./", "/");
  for (;;) {
    if (text.length > 2 && (text.endsWith("/*") || text.endsWith("/."))) text = text.slice(0, -2);
    else if (text.length > 1 && text.endsWith("/")) text = text.slice(0, -1);
    else break;
  }
  if (["/", "/*", "/.", "~", ".", ".."].includes(text)) return true;
  if (text.startsWith("$") && !text.includes("/")) return true;
  const lower = text.replace(/[A-Z]/g, (c) => c.toLowerCase());
  if (lower.length >= 2 && /^[a-z]$/.test(lower[0]) && lower[1] === ":") {
    const rest = lower.slice(2).replace(/^\/+/, "");
    return ["", "*", "windows", "users", "program files", "program files (x86)"].includes(rest);
  }
  if (text.startsWith("/")) {
    const parts = text.slice(1).split("/").filter((p) => p !== "");
    return parts.length <= 1 || (parts.length === 2 && ["home", "Users", "users"].includes(parts[0]));
  }
  return false;
}

function operands(args: string[]): string[] {
  const out: string[] = [];
  let optionsDone = false;
  for (const a of args) {
    if (!optionsDone && a === "--") {
      optionsDone = true;
      continue;
    }
    if (!optionsDone && a.startsWith("-") && a.length > 1) continue;
    out.push(a);
  }
  return out;
}

function hasRecursiveFlag(args: string[]): boolean {
  const end = args.indexOf("--");
  return (end === -1 ? args : args.slice(0, end)).some(
    (a) => a === "--recursive" || (a.startsWith("-") && !a.startsWith("--") && /[rR]/.test(a.slice(1))),
  );
}

const lowerAscii = (s: string): string => s.replace(/[A-Z]/g, (c) => c.toLowerCase());

function isCatastrophic(program: string, args: string[]): boolean {
  if (
    program.startsWith("mkfs") ||
    program.startsWith("newfs") ||
    [
      "mke2fs", "mkswap", "wipefs", "fdisk", "sfdisk", "gdisk", "sgdisk", "parted", "diskpart", "format", "format-volume",
      "clear-disk", "shutdown", "reboot", "halt", "poweroff", "stop-computer", "restart-computer",
    ].includes(program)
  ) {
    return true;
  }
  const noPreserve = args.some((a) => a.startsWith("--no-preserve-root"));
  switch (program) {
    case "rm": {
      const targets = operands(args);
      return (hasRecursiveFlag(args) && targets.some(isRootTarget)) || (noPreserve && targets.length > 0);
    }
    case "rmdir":
    case "rd":
    case "del":
    case "erase":
      return args.some((a) => lowerAscii(a) === "/s") && args.filter((a) => !a.startsWith("/") || a.length > 2).some(isRootTarget);
    case "remove-item":
    case "ri":
      return (
        args.some((a) => a.length > 2 && "-recurse".startsWith(lowerAscii(a))) &&
        args.filter((a) => !a.startsWith("-")).some(isRootTarget)
      );
    case "chmod":
    case "chown":
    case "chgrp":
      return hasRecursiveFlag(args) && operands(args).slice(1).some(isRootTarget);
    case "mv": {
      const targets = operands(args);
      return targets.length >= 2 && targets.slice(0, -1).some((t) => isRootTarget(t) && t !== "." && t !== "*");
    }
    case "find": {
      const destructive = args.some((a) => a === "-delete" || a === "-exec" || a === "-execdir");
      const starts: string[] = [];
      for (const a of args) {
        if (a.startsWith("-")) break;
        starts.push(a);
      }
      return destructive && starts.some((t) => isRootTarget(t) && ![".", "..", "./", "../"].includes(t));
    }
    case "dd":
      return args.some((a) => a.startsWith("of=") && (isBlockDevice(a.slice(3)) || a.slice(3).startsWith("\\\\.\\")));
    case "shred":
      return args.some(isBlockDevice);
    case "diskutil":
      return args.some((a) => {
        const lower = lowerAscii(a);
        return (
          lower.startsWith("erase") || lower.startsWith("zero") || lower.startsWith("random") ||
          ["secureerase", "partitiondisk", "reformat", "deletecontainer", "deletevolume"].includes(lower)
        );
      });
    case "kill":
      return args.length >= 2 && args[args.length - 1] === "-1";
    case "init":
    case "telinit":
      return args[0] === "0" || args[0] === "6";
    case "vssadmin":
      return args.some((a) => lowerAscii(a) === "delete");
    default:
      return false;
  }
}

function isBlockDevice(target: string): boolean {
  return BLOCK_DEVICE_PREFIXES.some((prefix) => target.startsWith(prefix));
}

// ---------------------------------------------------------------------------
// Path arguments

function looksLikePath(text: string): boolean {
  if (text === "" || HARMLESS_DEVICES.includes(text) || text.startsWith("/dev/fd/")) return false;
  const chars = [...text];
  const windowsAbs = chars.length >= 2 && /^[A-Za-z]$/.test(chars[0]) && chars[1] === ":";
  const parts = text.split(/[\\/]/);
  const name = parts[parts.length - 1] ?? text;
  return (
    text.startsWith("/") ||
    text.startsWith("~") ||
    text.startsWith("\\\\") ||
    windowsAbs ||
    text === ".." ||
    text.startsWith("../") ||
    text.startsWith("..\\") ||
    text.includes("/../") ||
    text.includes("\\..\\") ||
    text.endsWith("/..") ||
    text.endsWith("\\..") ||
    isSecretName(name) ||
    parts.some((part) => SENSITIVE_ANYWHERE.some((s) => lowerAscii(s) === lowerAscii(part)))
  );
}

function collectWordPath(word: Word, out: Analysis): void {
  if (word.dynamic) return;
  const text = word.text;
  let candidate: string;
  if (text.startsWith("-")) {
    const eq = text.indexOf("=");
    if (eq === -1) return;
    candidate = text.slice(eq + 1);
  } else {
    const eq = text.indexOf("=");
    if (eq !== -1 && /^[A-Za-z0-9_]*$/.test(text.slice(0, eq))) candidate = text.slice(eq + 1);
    else candidate = text;
  }
  if (looksLikePath(candidate)) out.paths.push({ text: candidate, write: false });
}
