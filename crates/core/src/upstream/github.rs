//! The upstream watch's one HTTP adapter: GitHub's public REST API.
//!
//! Unauthenticated on purpose: no token, no cookie, nothing about the user.
//! Each request carries a generic User-Agent and GitHub's JSON media type,
//! and asks only for public data of a repository named in
//! `upstream.lock.json`. Three requests per upstream per check (repository,
//! compare, releases), well under the 60 an hour GitHub allows without a token.

use super::{
    parse_compare, parse_default_branch, parse_releases, rate_limit_until, UpstreamReport, Watched,
    MAX_ITEMS,
};
use futures_util::StreamExt;
use std::time::Duration;

/// GitHub's API.
pub const API_BASE: &str = "https://api.github.com";

/// Bodies larger than this are refused (a compare carries file patches).
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Why a check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// GitHub asked us to wait until this unix time.
    RateLimited(u64),
    Other(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited(until) => write!(f, "GitHub rate limit; next try after {until}"),
            Self::Other(message) => f.write_str(message),
        }
    }
}

/// Client for GitHub's public API.
#[derive(Debug, Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    base: String,
}

impl GithubClient {
    /// A client for `base` (`API_BASE`, or a mock server in tests).
    ///
    /// # Errors
    /// The HTTP client could not be built.
    pub fn new(base: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(3))
            .user_agent("next-agent-upstream-watch")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
        })
    }

    async fn get(&self, path: &str, now: u64) -> Result<String, CheckError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| CheckError::Other(format!("GitHub: {e}")))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        if let Some(until) = rate_limit_until(status, header, now) {
            return Err(CheckError::RateLimited(until));
        }
        if !response.status().is_success() {
            return Err(CheckError::Other(format!(
                "GitHub answered {status} for {path}"
            )));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| CheckError::Other(format!("GitHub: {e}")))?;
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(CheckError::Other(format!(
                    "GitHub's answer for {path} is too large"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| CheckError::Other("GitHub sent invalid UTF-8".into()))
    }

    /// Commits and releases of `watched` since its pin. `now` is unix seconds.
    ///
    /// # Errors
    /// GitHub refused (rate limit), could not be reached, or sent unexpected JSON.
    pub async fn check(&self, watched: &Watched, now: u64) -> Result<UpstreamReport, CheckError> {
        if !super::valid_repo(&watched.repo) || !super::valid_sha(&watched.commit) {
            return Err(CheckError::Other(format!(
                "bad watch entry for {}",
                watched.id
            )));
        }
        let repo = &watched.repo;
        let branch = parse_default_branch(&self.get(&format!("/repos/{repo}"), now).await?)
            .map_err(CheckError::Other)?;
        if !branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        {
            return Err(CheckError::Other(format!(
                "unexpected branch name for {repo}"
            )));
        }
        let compare = parse_compare(
            &self
                .get(
                    &format!(
                        "/repos/{repo}/compare/{}...{branch}?per_page={MAX_ITEMS}",
                        watched.commit
                    ),
                    now,
                )
                .await?,
        )
        .map_err(CheckError::Other)?;
        let releases = parse_releases(
            &self
                .get(&format!("/repos/{repo}/releases?per_page=30"), now)
                .await?,
            compare.pinned_date.as_deref(),
        )
        .map_err(CheckError::Other)?;
        Ok(UpstreamReport {
            id: watched.id.clone(),
            repo: repo.clone(),
            pinned: watched.commit.clone(),
            pinned_date: compare.pinned_date,
            compare_url: compare.url.unwrap_or_else(|| {
                format!(
                    "https://github.com/{repo}/compare/{}...{branch}",
                    watched.commit
                )
            }),
            branch,
            ahead_by: compare.ahead_by,
            commits: compare.commits,
            releases,
            areas: compare.areas,
            checked_at: now,
            error: None,
        })
    }
}

/// Check every upstream in `list`. A rate limit stops the run (the rest would
/// be refused too) and is returned; other failures become that upstream's
/// `error`, keeping its previous findings from `previous`.
pub async fn check_all(
    client: &GithubClient,
    list: &[Watched],
    previous: &[UpstreamReport],
    now: u64,
) -> (Vec<UpstreamReport>, Option<u64>) {
    let mut reports = Vec::new();
    for watched in list {
        let old = previous
            .iter()
            .find(|r| r.id == watched.id && r.pinned == watched.commit);
        match client.check(watched, now).await {
            Ok(report) => reports.push(report),
            Err(CheckError::RateLimited(until)) => {
                // Keep what we had for this and the remaining upstreams.
                for rest in list.iter().skip(reports.len()) {
                    if let Some(old) = previous
                        .iter()
                        .find(|r| r.id == rest.id && r.pinned == rest.commit)
                    {
                        reports.push(old.clone());
                    }
                }
                return (reports, Some(until));
            }
            Err(CheckError::Other(message)) => {
                let mut report = old.cloned().unwrap_or_else(|| UpstreamReport {
                    id: watched.id.clone(),
                    repo: watched.repo.clone(),
                    pinned: watched.commit.clone(),
                    ..UpstreamReport::default()
                });
                report.error = Some(message);
                report.checked_at = now;
                reports.push(report);
            }
        }
    }
    (reports, None)
}
