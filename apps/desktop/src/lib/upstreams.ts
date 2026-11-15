// Settings → Upstreams: the shapes the Rust upstream watch returns
// (apps/desktop/src-tauri/src/upstreams.rs) and pure helpers for the page.

export interface WatchedUpstream {
  id: string;
  repo: string;
  commit: string;
}
export interface UpstreamCommit {
  sha: string;
  short: string;
  title: string;
  date: string;
  url: string;
  areas: string[];
}
export interface UpstreamRelease {
  tag: string;
  name: string;
  date: string;
  url: string;
  prerelease: boolean;
}
export interface UpstreamArea {
  path: string;
  files: number;
}
export interface UpstreamReport {
  id: string;
  repo: string;
  pinned: string;
  pinnedDate: string | null;
  branch: string;
  aheadBy: number;
  commits: UpstreamCommit[];
  releases: UpstreamRelease[];
  areas: UpstreamArea[];
  compareUrl: string;
  checkedAt: number;
  error: string | null;
}
export interface WatchView {
  watched: WatchedUpstream[];
  reports: UpstreamReport[];
  lastCheck: number | null;
  rateLimitedUntil: number | null;
  unseen: string[];
  checking: boolean;
  error: string | null;
}

/** Unix seconds → local date and time, or "never". */
export function formatUnix(seconds: number | null | undefined): string {
  if (!seconds) return "never";
  return new Date(seconds * 1000).toLocaleString();
}

/** GitHub's ISO date → local date, or "" when missing or invalid. */
export function formatIso(iso: string | null | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleDateString();
}

/** The rate-limit line, or null when GitHub is not making us wait. */
export function rateLimitNote(view: WatchView | null, nowSeconds: number): string | null {
  const until = view?.rateLimitedUntil;
  if (!until || until <= nowSeconds) return null;
  const minutes = Math.max(1, Math.ceil((until - nowSeconds) / 60));
  return `GitHub's rate limit: next check possible in about ${minutes} min.`;
}

/** The report for `id`, if one was made. */
export function reportFor(view: WatchView | null, id: string): UpstreamReport | undefined {
  return view?.reports.find((r) => r.id === id);
}

/** "3 new commits, 1 new release since the pin." */
export function summary(report: UpstreamReport | undefined): string {
  if (!report) return "Not checked yet.";
  const commits = report.aheadBy;
  const releases = report.releases.length;
  if (commits === 0 && releases === 0) return "Up to date with the pin.";
  const parts: string[] = [];
  if (commits > 0) parts.push(`${commits} new commit${commits === 1 ? "" : "s"}`);
  if (releases > 0) parts.push(`${releases} new release${releases === 1 ? "" : "s"}`);
  return `${parts.join(", ")} since the pin.`;
}
