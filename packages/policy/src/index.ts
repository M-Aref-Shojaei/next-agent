// Reference implementation of the M1 decision table, used to (a) run the
// conformance corpus without a Rust toolchain and (b) drive UI previews of what
// a mode would do. It mirrors `crates/permission-engine`; the fixture at
// `fixtures/policy-cases.json` is the shared contract. Change both together.

export type Mode = "plan" | "manual" | "auto" | "bypass";

export type Request =
  | { kind: "read"; path: string }
  | { kind: "list"; path: string }
  | { kind: "search"; path: string }
  | { kind: "write"; path: string }
  | { kind: "exec"; command: string; cwd?: string }
  | { kind: "network"; url: string }
  | { kind: "ask" };

export type Verdict = "allow" | "ask" | "deny";

export type CommandClass =
  | "safe_dev"
  | "unknown"
  | "deploy"
  | "network_remote"
  | "sensitive"
  | "catastrophic";

export interface Context {
  root: string;
  home: string;
  workspaceId: string;
  caseInsensitive: boolean;
  allowlistedHosts: string[];
}

export interface Evaluation {
  verdict: Verdict;
  rule: string;
  class?: CommandClass;
}

const READ_ONLY = ["ls", "cat", "head", "tail", "wc", "rg", "grep", "fd", "find", "tree", "file", "stat", "which", "echo", "pwd", "true"];

const SAFE_SUBCOMMANDS: Record<string, string[]> = {
  pnpm: ["test", "lint", "build", "run", "install", "i", "ci", "fmt", "check", "exec", "dlx", "vitest", "tsc"],
  npm: ["test", "run", "install", "ci", "exec", "ls"],
  yarn: ["test", "run", "install", "build", "lint"],
  bun: ["test", "run", "install", "build", "lint"],
  cargo: ["build", "test", "check", "clippy", "fmt", "run", "doc", "bench", "metadata"],
  git: ["status", "diff", "log", "add", "commit", "branch", "show", "rev-parse", "stash", "switch", "checkout", "init", "worktree", "apply", "restore", "blame"],
  uv: ["run", "sync", "lock"],
  python3: ["-m"],
  node: ["--test"],
  tsc: [],
  vitest: [],
  pytest: [],
  make: [],
  go: ["build", "test", "vet"],
};

const SENSITIVE = ["sudo", "su", "doas", "apt", "apt-get", "dnf", "yum", "pacman", "brew", "port", "systemctl", "service", "launchctl", "crontab", "at", "defaults", "reg", "sc", "net", "iptables", "pfctl", "nft", "diskutil", "security", "csrutil", "chmod", "chown", "chgrp", "killall", "pkill"];

const NETWORK = ["ssh", "scp", "sftp", "telnet", "nc", "netcat", "ncat", "ngrok", "cloudflared", "tailscale", "rclone", "ftp", "socat", "curl", "wget"];

const DEPLOY: Record<string, string[]> = {
  git: ["push"],
  vercel: ["deploy", "--prod", "promote"],
  fly: ["deploy", "launch"],
  flyctl: ["deploy", "launch"],
  kubectl: ["apply", "delete", "rollout", "scale", "drain"],
  helm: ["upgrade", "install", "uninstall"],
  terraform: ["apply", "destroy", "import"],
  pulumi: ["up", "destroy"],
  netlify: ["deploy"],
  heroku: ["deploy", "pg:push"],
  aws: ["s3", "cloudformation", "ecs", "lambda"],
  gcloud: ["deploy", "app", "run"],
  docker: ["push"],
};

const SENSITIVE_IN_HOME = [".ssh", ".aws", ".gnupg", ".kube", ".docker", ".config", "Library/Keychains", "AppData/Roaming/Microsoft/Credentials"];
const SENSITIVE_ABSOLUTE = ["/etc", "/usr", "/System", "/bin", "/sbin", "/var/root", "C:\\Windows", "C:\\Program Files"];

/** Split on unquoted `|`, `&&`, `||`, `;`, newlines, `$( )` and backticks. */
export function splitSegments(input: string): string[] {
  const out: string[] = [];
  let current = "";
  let quote: string | null = null;
  const flush = (): void => {
    if (current.trim()) out.push(current.trim());
    current = "";
  };
  for (let i = 0; i < input.length; i += 1) {
    const ch = input[i];
    if (quote) {
      current += ch;
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      current += ch;
    } else if (ch === "`") {
      flush();
      const end = input.indexOf("`", i + 1);
      const inner = end === -1 ? input.slice(i + 1) : input.slice(i + 1, end);
      out.push(...splitSegments(inner));
      i = end === -1 ? input.length : end;
    } else if (ch === "$" && input[i + 1] === "(") {
      flush();
      let depth = 1;
      let inner = "";
      i += 2;
      for (; i < input.length; i += 1) {
        if (input[i] === "(") depth += 1;
        if (input[i] === ")" && --depth === 0) break;
        inner += input[i];
      }
      out.push(...splitSegments(inner));
    } else if ("|&;\n".includes(ch)) {
      flush();
      const next = input[i + 1];
      if ((ch === "&" && next === "&") || (ch === "|" && next === "|")) i += 1;
    } else if (ch === "\\") {
      current += input[i + 1] ?? "";
      i += 1;
    } else {
      current += ch;
    }
  }
  flush();
  return out;
}

export function words(segment: string): string[] {
  const out: string[] = [];
  let current = "";
  let quote: string | null = null;
  for (const ch of segment) {
    if (quote) {
      if (ch === quote) quote = null;
      else current += ch;
    } else if (ch === "'" || ch === '"') quote = ch;
    else if (/\s/.test(ch)) {
      if (current) out.push(current);
      current = "";
    } else current += ch;
  }
  if (current) out.push(current);
  return out;
}

const RANK: Record<CommandClass, number> = {
  safe_dev: 0,
  unknown: 1,
  deploy: 2,
  network_remote: 3,
  sensitive: 4,
  catastrophic: 5,
};

export function classify(command: string): CommandClass {
  return splitSegments(command)
    .map(classifySegment)
    .reduce<CommandClass>((worst, next) => (RANK[next] > RANK[worst] ? next : worst), "safe_dev");
}

function classifySegment(segment: string): CommandClass {
  const all = words(segment);
  const stripped = stripWrappers(all);
  if (stripped.length === 0) return "safe_dev";
  const program = stripped[0].split(/[\\/]/).pop() ?? stripped[0];
  const args = stripped.slice(1);

  if (isCatastrophic(program, args)) return "catastrophic";
  const inner = inlineScript(program, args);
  if (inner !== null) return classify(inner);
  if (SENSITIVE.includes(program)) return "sensitive";
  if (NETWORK.includes(program)) return "network_remote";
  if ((program === "rsync" || program === "scp") && args.some((a) => a.includes(":"))) return "network_remote";
  if (isDeploy(program, args)) return "deploy";
  if (isSafe(program, args)) return "safe_dev";
  return "unknown";
}

function stripWrappers(input: string[]): string[] {
  let i = 0;
  while (i < input.length) {
    if (["sudo", "doas", "env", "time", "nohup", "command", "builtin"].includes(input[i])) {
      i += 1;
      while (i < input.length && input[i].includes("=") && !input[i].startsWith("-")) i += 1;
    } else if (input[i] === "nice" || input[i] === "xargs") {
      i += 1;
      while (i < input.length && input[i].startsWith("-")) i += 1;
    } else break;
  }
  return input.slice(i);
}

function inlineScript(program: string, args: string[]): string | null {
  if (["sh", "bash", "zsh", "dash", "fish"].includes(program)) {
    const at = args.indexOf("-c");
    return at === -1 ? null : (args[at + 1] ?? null);
  }
  if (["python", "python3", "node", "perl", "ruby", "deno", "bun"].includes(program)) {
    const flag = program === "node" ? "-e" : "-c";
    const at = args.indexOf(flag);
    return at === -1 ? null : (args[at + 1] ?? null);
  }
  if (program === "eval") return args.join(" ");
  return null;
}

function isDeploy(program: string, args: string[]): boolean {
  if (args.some((a) => a === "--prod" || a === "--production")) return true;
  return (DEPLOY[program] ?? []).some((verb) => args.includes(verb));
}

function isSafe(program: string, args: string[]): boolean {
  if (READ_ONLY.includes(program)) return !args.some((a) => a === "-exec" || a === "-delete" || a === "-i");
  const verbs = SAFE_SUBCOMMANDS[program];
  if (verbs === undefined) return false;
  if (args.length === 0) return true;
  return verbs.length === 0 || verbs.includes(args[0]) || args[0].startsWith("-");
}

function isCatastrophic(program: string, args: string[]): boolean {
  const rootTarget = args.some((a) => ["/", "/*", "~", "~/", "/*/*", ".", ".."].includes(a) || a.startsWith("--no-preserve-root"));
  switch (program) {
    case "rm": {
      const recursive = args.some((a) => a.length > 1 && a.startsWith("-") && a.includes("r"));
      return recursive && rootTarget;
    }
    case "mkfs":
    case "mkfs.ext4":
    case "mkfs.xfs":
    case "format":
      return true;
    case "dd":
      return args.some((a) => a.startsWith("of=/dev/") || a.startsWith("of=\\\\"));
    case "chmod":
    case "chown":
      return args.some((a) => a === "-R" || a === "-r") && rootTarget;
    case "shutdown":
    case "reboot":
    case "halt":
    case "poweroff":
      return true;
    default:
      return program === ":" && args.join(" ").includes(":|:");
  }
}

export function normalize(path: string): string {
  const parts = path.replaceAll("\\", "/").split("/");
  const out: string[] = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") out.pop();
    else out.push(part);
  }
  return "/" + out.join("/");
}

export function expand(input: string, cwd: string, home: string): string {
  if (input === "~") return normalize(home);
  if (input.startsWith("~/")) return normalize(home + "/" + input.slice(2));
  return normalize(input.startsWith("/") ? input : `${cwd}/${input}`);
}

function within(child: string, parent: string, caseInsensitive: boolean): boolean {
  const norm = (p: string): string => (caseInsensitive ? normalize(p).toLowerCase() : normalize(p));
  const childParts = norm(child).split("/").filter(Boolean);
  const parentParts = norm(parent).split("/").filter(Boolean);
  if (parentParts.length === 0 || parentParts.length > childParts.length) return false;
  return parentParts.every((part, index) => childParts[index] === part);
}

export function isSensitive(path: string, ctx: Context): boolean {
  if (SENSITIVE_IN_HOME.some((entry) => within(path, `${ctx.home}/${entry}`, ctx.caseInsensitive))) return true;
  return SENSITIVE_ABSOLUTE.some((prefix) => within(path, prefix, ctx.caseInsensitive) || within(prefix, path, ctx.caseInsensitive) && normalize(path) === normalize(prefix));
}

export function zone(path: string, ctx: Context): "inside" | "outside" | "sensitive" {
  if (isSensitive(path, ctx)) return "sensitive";
  return within(path, ctx.root, ctx.caseInsensitive) ? "inside" : "outside";
}

function isSecretFile(path: string): boolean {
  const name = path.split(/[\\/]/).pop() ?? "";
  return name === ".env" || name.startsWith(".env.");
}

export function evaluate(request: Request, mode: Mode, ctx: Context): Evaluation {
  const own = "path" in request ? request.path : undefined;
  const resolved = own === undefined ? undefined : expand(own, ctx.root, ctx.home);
  const pathZone = resolved === undefined ? undefined : zone(resolved, ctx);
  const cmdClass = request.kind === "exec" ? classify(request.command) : undefined;

  if (cmdClass === "catastrophic") return { verdict: "deny", rule: "catastrophic", class: cmdClass };
  if (request.kind === "ask") return { verdict: "ask", rule: "ask_user" };

  if (mode === "bypass") return { verdict: "allow", rule: "bypass", class: cmdClass };

  const deny = (): Evaluation => ({ verdict: "deny", rule: denyRule(mode), class: cmdClass });
  const allow = (): Evaluation => ({ verdict: "allow", rule: allowRule(mode), class: cmdClass });
  const ask = (): Evaluation => ({ verdict: "ask", rule: "mode_ask", class: cmdClass });

  switch (request.kind) {
    case "read":
    case "list":
    case "search":
      if (pathZone === "inside") return allow();
      if (pathZone === "sensitive" && mode === "plan") return deny();
      return ask();

    case "write":
      if (mode === "plan") return deny();
      if (pathZone === "sensitive") return deny();
      if (mode === "auto" && pathZone === "inside") return isSecretFile(resolved ?? "") ? ask() : allow();
      return ask();

    case "exec": {
      const cls = cmdClass ?? "unknown";
      if (mode === "plan") return deny();
      if (cls === "safe_dev") return allow();
      if (cls === "unknown") return ask();
      if (mode === "auto") return deny();
      return ask();
    }

    case "network": {
      if (mode === "plan") return deny();
      const host = request.url.split("://").pop()!.split(/[/?#]/)[0].split("@").pop()!.split(":")[0];
      if (mode === "auto" && ctx.allowlistedHosts.includes(host)) return allow();
      return ask();
    }

    default:
      return ask();
  }
}

/** Mirror of `reason_rule` in crates/permission-engine/src/engine.rs. */
function denyRule(mode: Mode): string {
  if (mode === "plan") return "plan_denied";
  if (mode === "auto") return "auto_forbidden";
  return "mode_denied";
}


function allowRule(mode: Mode): string {
  switch (mode) {
    case "plan":
      return "plan_readonly";
    case "manual":
      return "mode_allow";
    case "auto":
      return "auto_sandbox";
    default:
      return "bypass";
  }
}

export function matchesFixtureCase(
  testCase: { mode: Mode; request: Record<string, unknown>; expect: Verdict; rule: string },
  ctx: Context,
): { ok: boolean; actual: Evaluation } {
  const actual = evaluate(testCase.request as unknown as Request, testCase.mode, ctx);
  return { ok: actual.verdict === testCase.expect && actual.rule === testCase.rule, actual };
}
