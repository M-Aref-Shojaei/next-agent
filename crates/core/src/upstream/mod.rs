//! Upstream watch: which vendored or ported upstreams moved since their pin.
//!
//! Report only. Nothing here applies, downloads or merges code: it lists the
//! commits and releases an upstream published after the commit recorded in
//! `upstream.lock.json`, so the owner can decide what to bring in.
//!
//! The pure parts (the watch list, parsing, changed areas, rate limits,
//! scheduling) live here; the one HTTP adapter is [`github`].

pub mod github;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The lock file, embedded at build time so the pins match the code shipped.
pub const LOCK_JSON: &str = include_str!("../../../../upstream.lock.json");

/// How often the background check runs while the app is open.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// At most this many commits and releases are kept per upstream.
pub const MAX_ITEMS: usize = 100;

/// One upstream to watch: its GitHub repository and the commit we are pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watched {
    /// The key in `upstream.lock.json` (`coucou`, `9router`…).
    pub id: String,
    /// `owner/name` on GitHub.
    pub repo: String,
    /// Full sha of the pinned commit.
    pub commit: String,
}

/// The upstreams that have a `watch` entry in `lock_json`.
///
/// # Errors
/// The lock file is not valid JSON, or a `watch` entry is malformed.
pub fn watched_from_lock(lock_json: &str) -> Result<Vec<Watched>, String> {
    #[derive(Deserialize)]
    struct Lock {
        components: BTreeMap<String, Component>,
    }
    #[derive(Deserialize)]
    struct Component {
        watch: Option<WatchEntry>,
    }
    #[derive(Deserialize)]
    struct WatchEntry {
        repo: String,
        commit: String,
    }
    let lock: Lock =
        serde_json::from_str(lock_json).map_err(|e| format!("upstream.lock.json: {e}"))?;
    let mut out = Vec::new();
    for (id, component) in lock.components {
        let Some(watch) = component.watch else {
            continue;
        };
        if !valid_repo(&watch.repo) {
            return Err(format!("upstream `{id}`: watch.repo must be owner/name"));
        }
        if !valid_sha(&watch.commit) {
            return Err(format!("upstream `{id}`: watch.commit must be a full sha"));
        }
        out.push(Watched {
            id,
            repo: watch.repo,
            commit: watch.commit.to_ascii_lowercase(),
        });
    }
    Ok(out)
}

/// The upstreams embedded in this build.
///
/// # Errors
/// See [`watched_from_lock`].
pub fn watched() -> Result<Vec<Watched>, String> {
    watched_from_lock(LOCK_JSON)
}

fn valid_name(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `owner/name`, with GitHub's characters only (it ends up in a URL path).
#[must_use]
pub fn valid_repo(repo: &str) -> bool {
    matches!(repo.split_once('/'), Some((owner, name)) if valid_name(owner) && valid_name(name))
}

/// A full 40-hex-digit sha.
#[must_use]
pub fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// A commit published after the pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub short: String,
    /// First line of the message.
    pub title: String,
    /// ISO-8601, as GitHub gives it.
    pub date: String,
    pub url: String,
    /// What the commit touches, from its conventional-commit scope
    /// (`feat(island): …` → `island`). Per-file areas would cost one request
    /// per commit; the whole range's areas are on [`UpstreamReport::areas`].
    pub areas: Vec<String>,
}

/// A release published after the pinned commit's date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag: String,
    pub name: String,
    pub date: String,
    pub url: String,
    pub prerelease: bool,
}

/// A changed area of the whole range and how many files changed in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Area {
    pub path: String,
    pub files: usize,
}

/// What one check of one upstream found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamReport {
    pub id: String,
    pub repo: String,
    pub pinned: String,
    /// Date of the pinned commit, when GitHub knew it.
    pub pinned_date: Option<String>,
    pub branch: String,
    /// Commits on `branch` after the pin (may exceed `commits.len()`).
    pub ahead_by: usize,
    /// Newest first.
    pub commits: Vec<CommitInfo>,
    /// Newest first.
    pub releases: Vec<ReleaseInfo>,
    /// Changed areas across the whole range, most files first.
    pub areas: Vec<Area>,
    pub compare_url: String,
    pub checked_at: u64,
    /// Set when this upstream could not be checked.
    pub error: Option<String>,
}

impl UpstreamReport {
    /// The newest thing in the report, used to know whether the owner saw it.
    #[must_use]
    pub fn latest_marker(&self) -> Option<String> {
        let commit = self.commits.first().map(|c| c.sha.clone());
        let release = self.releases.first().map(|r| format!("release:{}", r.tag));
        match (commit, release) {
            (None, None) => None,
            (c, r) => Some(format!(
                "{}|{}",
                c.unwrap_or_default(),
                r.unwrap_or_default()
            )),
        }
    }
}

/// Everything the watch remembers between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchState {
    pub reports: Vec<UpstreamReport>,
    /// Unix seconds of the last completed check.
    pub last_check: Option<u64>,
    /// GitHub said to wait until then (unix seconds).
    pub rate_limited_until: Option<u64>,
    /// Upstream id → the `latest_marker` the owner marked as seen.
    pub seen: BTreeMap<String, String>,
}

impl WatchState {
    /// Whether the background check should run now.
    #[must_use]
    pub fn due(&self, now: u64) -> bool {
        if self.rate_limited(now) {
            return false;
        }
        self.last_check
            .is_none_or(|last| now.saturating_sub(last) >= CHECK_INTERVAL_SECS || last > now)
    }

    /// Whether GitHub's rate limit still applies at `now`.
    #[must_use]
    pub fn rate_limited(&self, now: u64) -> bool {
        self.rate_limited_until.is_some_and(|until| until > now)
    }

    /// Ids of the upstreams with something the owner has not marked as seen.
    #[must_use]
    pub fn unseen(&self) -> Vec<String> {
        self.reports
            .iter()
            .filter(|r| {
                r.latest_marker()
                    .is_some_and(|m| self.seen.get(&r.id) != Some(&m))
            })
            .map(|r| r.id.clone())
            .collect()
    }

    /// Mark everything currently reported as seen.
    pub fn mark_seen(&mut self) {
        for report in &self.reports {
            match report.latest_marker() {
                Some(marker) => self.seen.insert(report.id.clone(), marker),
                None => self.seen.remove(&report.id),
            };
        }
    }
}

/// Top-level area of a changed path: its first two segments for a folder
/// (`windows/src-tauri/src/lib.rs` → `windows/src-tauri`), its first for a
/// shallow path, and `(root)` for a file at the root.
#[must_use]
pub fn area_of(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 | 1 => "(root)".to_string(),
        2 => parts[0].to_string(),
        _ => format!("{}/{}", parts[0], parts[1]),
    }
}

/// The areas of `paths`, most files first, then by name.
#[must_use]
pub fn areas<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<Area> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in paths {
        *counts.entry(area_of(path)).or_default() += 1;
    }
    let mut out: Vec<Area> = counts
        .into_iter()
        .map(|(path, files)| Area { path, files })
        .collect();
    out.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.path.cmp(&b.path)));
    out
}

/// Conventional-commit scopes of a title: `fix(island, pill): …` → `island`, `pill`.
#[must_use]
pub fn title_scopes(title: &str) -> Vec<String> {
    let head = title.split(':').next().unwrap_or("");
    let Some((kind, rest)) = head.split_once('(') else {
        return Vec::new();
    };
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
        return Vec::new();
    }
    let Some(scope) = rest.strip_suffix(')').or_else(|| rest.strip_suffix(")!")) else {
        return Vec::new();
    };
    scope
        .split([',', '/'])
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 40)
        .map(str::to_string)
        .collect()
}

/// When to try again after a refused response, in unix seconds, or `None`
/// when the response was not a rate limit. `headers` returns a header value.
#[must_use]
pub fn rate_limit_until(
    status: u16,
    header: impl Fn(&str) -> Option<String>,
    now: u64,
) -> Option<u64> {
    if status != 403 && status != 429 {
        return None;
    }
    let number = |name: &str| header(name).and_then(|v| v.trim().parse::<u64>().ok());
    if let Some(after) = number("retry-after") {
        return Some(now + after.max(1));
    }
    let exhausted = number("x-ratelimit-remaining") == Some(0);
    match (exhausted, number("x-ratelimit-reset")) {
        (true, Some(reset)) => Some(reset.max(now + 1)),
        // A 429, or a 403 secondary limit without headers: back off a minute
        // at least, as GitHub's docs ask.
        (true, None) => Some(now + 60),
        (false, _) if status == 429 => Some(now + 60),
        _ => None,
    }
}

// ── GitHub's JSON ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GhRepo {
    default_branch: String,
}

#[derive(Deserialize)]
struct GhPerson {
    date: Option<String>,
}

#[derive(Deserialize)]
struct GhCommitBody {
    message: String,
    committer: Option<GhPerson>,
    author: Option<GhPerson>,
}

#[derive(Deserialize)]
struct GhCommit {
    sha: String,
    html_url: String,
    commit: GhCommitBody,
}

#[derive(Deserialize)]
struct GhFile {
    filename: String,
}

#[derive(Deserialize)]
struct GhCompare {
    #[serde(default)]
    ahead_by: usize,
    html_url: Option<String>,
    base_commit: Option<GhCommit>,
    #[serde(default)]
    commits: Vec<GhCommit>,
    #[serde(default)]
    files: Vec<GhFile>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The repository's default branch.
///
/// # Errors
/// Not GitHub's repository JSON.
pub fn parse_default_branch(json: &str) -> Result<String, String> {
    let repo: GhRepo = serde_json::from_str(json).map_err(|e| format!("repository: {e}"))?;
    Ok(repo.default_branch)
}

fn commit_date(body: &GhCommitBody) -> String {
    body.committer
        .as_ref()
        .and_then(|p| p.date.clone())
        .or_else(|| body.author.as_ref().and_then(|p| p.date.clone()))
        .unwrap_or_default()
}

fn commit_info(commit: GhCommit) -> CommitInfo {
    let title = commit
        .commit
        .message
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    CommitInfo {
        short: commit.sha.chars().take(7).collect(),
        areas: title_scopes(&title),
        date: commit_date(&commit.commit),
        title,
        url: commit.html_url,
        sha: commit.sha,
    }
}

/// What a compare from the pin to the branch says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compared {
    pub ahead_by: usize,
    pub pinned_date: Option<String>,
    /// Newest first, at most [`MAX_ITEMS`].
    pub commits: Vec<CommitInfo>,
    pub areas: Vec<Area>,
    pub url: Option<String>,
}

/// Parse `GET /repos/{repo}/compare/{pin}...{branch}`.
///
/// # Errors
/// Not GitHub's compare JSON.
pub fn parse_compare(json: &str) -> Result<Compared, String> {
    let compare: GhCompare = serde_json::from_str(json).map_err(|e| format!("compare: {e}"))?;
    let pinned_date = compare
        .base_commit
        .as_ref()
        .map(|c| commit_date(&c.commit))
        .filter(|d| !d.is_empty());
    let areas = areas(compare.files.iter().map(|f| f.filename.as_str()));
    // GitHub lists the range oldest first; the newest matter most.
    let mut commits: Vec<CommitInfo> = compare.commits.into_iter().map(commit_info).collect();
    commits.reverse();
    commits.truncate(MAX_ITEMS);
    Ok(Compared {
        ahead_by: compare.ahead_by,
        pinned_date,
        commits,
        areas,
        url: compare.html_url,
    })
}

/// Parse `GET /repos/{repo}/releases`, keeping the published, non-draft ones
/// after `since` (an ISO-8601 date; `None` keeps none, since without the
/// pin's date nothing can be said to be newer).
///
/// # Errors
/// Not GitHub's releases JSON.
pub fn parse_releases(json: &str, since: Option<&str>) -> Result<Vec<ReleaseInfo>, String> {
    let releases: Vec<GhRelease> =
        serde_json::from_str(json).map_err(|e| format!("releases: {e}"))?;
    let Some(since) = since else {
        return Ok(Vec::new());
    };
    let mut out: Vec<ReleaseInfo> = releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let date = r.published_at?;
            // GitHub's dates are all `YYYY-MM-DDTHH:MM:SSZ`, so they sort as text.
            (date.as_str() > since).then(|| ReleaseInfo {
                name: r
                    .name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| r.tag_name.clone()),
                tag: r.tag_name,
                date,
                url: r.html_url,
                prerelease: r.prerelease,
            })
        })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(MAX_ITEMS);
    Ok(out)
}

#[cfg(test)]
mod tests;
