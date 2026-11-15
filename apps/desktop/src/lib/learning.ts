// Memory, skills and cron jobs (Settings → Memory and skills). Wire types
// match `apps/desktop/src-tauri/src/learning.rs`; helpers are pure so they
// can be tested without a window.

export interface LearningSettings {
  backgroundReview: boolean;
}
export interface MemoryFile {
  target: "memory" | "user";
  /** Entries joined by the `§` separator line, as on disk. */
  text: string;
  entries: number;
  used: number;
  limit: number;
  path: string;
}
export interface MemoryState {
  memory: MemoryFile;
  user: MemoryFile;
}
export interface SkillInfo {
  name: string;
  description: string;
  category: string | null;
  scope: "workspace" | "user";
  root: string;
  state: "active" | "stale" | "archived";
  pinned: boolean;
  createdBy: string | null;
  useCount: number;
  viewCount: number;
  patchCount: number;
  lastActivityAt: number | null;
}
export interface CuratorReport {
  checked: number;
  markedStale: string[];
  archived: string[];
  reactivated: string[];
}
export interface CronJob {
  id: string;
  workspaceId: string;
  name: string;
  prompt: string;
  schedule: string;
  scheduleLabel: string;
  skills: string[];
  modelRole: string | null;
  mode: string;
  workdir: string | null;
  timezone: string | null;
  graceMinutes: number | null;
  repeat: number | null;
  runCount: number;
  state: "scheduled" | "paused" | "completed" | "error";
  nextRunAt: number | null;
  lastRunAt: number | null;
  lastStatus: string | null;
  lastError: string | null;
  lastTaskId: string | null;
  origin: string;
}
export interface CronInput {
  id?: string | null;
  workspaceId: string;
  name?: string | null;
  prompt: string;
  schedule: string;
  skills: string[];
  modelRole?: string | null;
  mode?: string | null;
  workdir?: string | null;
  timezone?: string | null;
  graceMinutes?: number | null;
  repeat?: number | null;
}

/** Separator between memory entries on disk. */
export const ENTRY_DELIMITER = "\n§\n";

/** Character caps of MEMORY.md and USER.md (mirrors the Rust constants). */
export const MEMORY_LIMITS = { memory: 2200, user: 1375 } as const;

/** Characters the editor text would use once saved (trimmed, deduplicated entries). */
export function memoryUsage(text: string): { entries: string[]; used: number } {
  const entries: string[] = [];
  for (const raw of text.replaceAll("\r\n", "\n").split(ENTRY_DELIMITER)) {
    const entry = raw.trim();
    if (entry && !entries.includes(entry)) entries.push(entry);
  }
  return { entries, used: [...entries.join(ENTRY_DELIMITER)].length };
}

/** Percentage of the cap in use, 0..100. */
export function usagePercent(used: number, limit: number): number {
  return limit <= 0 ? 0 : Math.min(100, Math.floor((used * 100) / limit));
}

/** "in 5 min", "3 h ago", "—" for a unix-seconds time relative to `now` (ms). */
export function relativeTime(secs: number | null, now = Date.now()): string {
  if (secs === null) return "—";
  const delta = Math.round(secs - now / 1000);
  const abs = Math.abs(delta);
  const [n, unit] =
    abs < 60 ? [abs, "s"] : abs < 3600 ? [Math.round(abs / 60), "min"] : abs < 86_400 ? [Math.round(abs / 3600), "h"] : [Math.round(abs / 86_400), "d"];
  return delta >= 0 ? `in ${n} ${unit}` : `${n} ${unit} ago`;
}

/** Group skills for the list: active and stale first, archived last, then by name. */
export function sortSkills(skills: SkillInfo[]): SkillInfo[] {
  const rank = (s: SkillInfo) => (s.state === "archived" ? 1 : 0);
  return [...skills].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

/** Blank form for a new cron job. */
export function emptyCronInput(workspaceId: string): CronInput {
  return { id: null, workspaceId, name: "", prompt: "", schedule: "every day at 9am", skills: [], modelRole: "", mode: "auto", workdir: "", timezone: "", graceMinutes: null, repeat: null };
}

/** The editable fields of an existing job. */
export function cronInputOf(job: CronJob): CronInput {
  return {
    id: job.id,
    workspaceId: job.workspaceId,
    name: job.name,
    prompt: job.prompt,
    schedule: job.schedule,
    skills: job.skills,
    modelRole: job.modelRole ?? "",
    mode: job.mode,
    workdir: job.workdir ?? "",
    timezone: job.timezone ?? "",
    graceMinutes: job.graceMinutes,
    repeat: job.repeat,
  };
}
