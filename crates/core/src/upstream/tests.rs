use super::github::{check_all, CheckError, GithubClient};
use super::*;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PIN: &str = "0d7fe58884d99f275f3238e8dbe29ddee69fa9e8";

fn coucou() -> Watched {
    Watched {
        id: "coucou".into(),
        repo: "Louis-CFM/coucou".into(),
        commit: PIN.into(),
    }
}

const COMPARE: &str = r#"{
  "html_url": "https://github.com/Louis-CFM/coucou/compare/0d7fe58...main",
  "ahead_by": 3,
  "base_commit": {"sha": "0d7fe58884d99f275f3238e8dbe29ddee69fa9e8", "html_url": "https://github.com/x",
    "commit": {"message": "pinned", "committer": {"date": "2026-11-14T08:00:00Z"}}},
  "commits": [
    {"sha": "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "html_url": "https://github.com/Louis-CFM/coucou/commit/1111111",
     "commit": {"message": "feat(island): bigger notch\n\nBody text", "committer": {"date": "2026-11-15T09:00:00Z"}}},
    {"sha": "2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "html_url": "https://github.com/Louis-CFM/coucou/commit/2222222",
     "commit": {"message": "Fix typo", "author": {"date": "2026-11-15T10:00:00Z"}}},
    {"sha": "3333333ccccccccccccccccccccccccccccccccc", "html_url": "https://github.com/Louis-CFM/coucou/commit/3333333",
     "commit": {"message": "fix(pill, flow)!: paste", "committer": {"date": "2026-11-15T11:00:00Z"}}}
  ],
  "files": [
    {"filename": "windows/src-tauri/src/lib.rs"},
    {"filename": "windows/src-tauri/src/island.rs"},
    {"filename": "windows/src/island/island.ts"},
    {"filename": "README.md"},
    {"filename": "docs/x.md"}
  ]
}"#;

const RELEASES: &str = r#"[
  {"tag_name": "v0.6.0", "name": "", "html_url": "https://github.com/Louis-CFM/coucou/releases/tag/v0.6.0",
   "published_at": "2026-11-15T12:00:00Z"},
  {"tag_name": "v0.6.0-draft", "name": "Draft", "html_url": "https://x", "published_at": null, "draft": true},
  {"tag_name": "v0.5.10", "name": "Coucou 0.5.10", "html_url": "https://github.com/Louis-CFM/coucou/releases/tag/v0.5.10",
   "published_at": "2026-11-14T10:00:00Z", "prerelease": true},
  {"tag_name": "v0.5.9", "name": "Coucou 0.5.9", "html_url": "https://x", "published_at": "2026-11-01T10:00:00Z"}
]"#;

#[test]
fn the_embedded_lock_file_watches_coucou_only() {
    let list = watched().expect("lock file parses");
    assert_eq!(list, vec![coucou()]);
}

#[test]
fn malformed_watch_entries_are_refused() {
    let bad_repo = r#"{"components":{"x":{"watch":{"repo":"../etc","commit":"0d7fe58884d99f275f3238e8dbe29ddee69fa9e8"}}}}"#;
    assert!(watched_from_lock(bad_repo).is_err());
    let bad_sha = r#"{"components":{"x":{"watch":{"repo":"a/b","commit":"main"}}}}"#;
    assert!(watched_from_lock(bad_sha).is_err());
    let none = r#"{"components":{"x":{"pin":"1"}}}"#;
    assert_eq!(watched_from_lock(none).unwrap(), vec![]);
    assert!(valid_repo("Louis-CFM/coucou"));
    assert!(!valid_repo("a/b/c"));
    assert!(!valid_repo("a/.."));
    assert!(!valid_repo("a b/c"));
}

#[test]
fn areas_group_paths_by_their_first_two_folders() {
    assert_eq!(area_of("windows/src-tauri/src/lib.rs"), "windows/src-tauri");
    assert_eq!(area_of("docs/x.md"), "docs");
    assert_eq!(area_of("README.md"), "(root)");
    let found = areas(["a/b/c.rs", "a/b/d.rs", "z/y.md", "top.txt"]);
    assert_eq!(
        found,
        vec![
            Area {
                path: "a/b".into(),
                files: 2
            },
            Area {
                path: "(root)".into(),
                files: 1
            },
            Area {
                path: "z".into(),
                files: 1
            },
        ]
    );
}

#[test]
fn scopes_come_from_conventional_commit_titles() {
    assert_eq!(title_scopes("feat(island): x"), vec!["island"]);
    assert_eq!(title_scopes("fix(pill, flow)!: x"), vec!["pill", "flow"]);
    assert_eq!(title_scopes("Fix typo"), Vec::<String>::new());
    assert_eq!(
        title_scopes("Merge (pull) request: x"),
        Vec::<String>::new()
    );
}

#[test]
fn compare_lists_commits_newest_first_with_the_pin_date() {
    let compared = parse_compare(COMPARE).unwrap();
    assert_eq!(compared.ahead_by, 3);
    assert_eq!(
        compared.pinned_date.as_deref(),
        Some("2026-11-14T08:00:00Z")
    );
    let titles: Vec<&str> = compared.commits.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "fix(pill, flow)!: paste",
            "Fix typo",
            "feat(island): bigger notch"
        ]
    );
    assert_eq!(
        compared.commits[1].date, "2026-11-15T10:00:00Z",
        "author date as fallback"
    );
    assert_eq!(compared.commits[2].short, "1111111");
    assert_eq!(compared.commits[2].areas, vec!["island"]);
    assert_eq!(
        compared.areas[0],
        Area {
            path: "windows/src-tauri".into(),
            files: 2
        }
    );
}

#[test]
fn releases_after_the_pin_only_and_no_drafts() {
    let releases = parse_releases(RELEASES, Some("2026-11-14T08:00:00Z")).unwrap();
    let tags: Vec<&str> = releases.iter().map(|r| r.tag.as_str()).collect();
    assert_eq!(tags, ["v0.6.0", "v0.5.10"]);
    assert_eq!(
        releases[0].name, "v0.6.0",
        "empty name falls back to the tag"
    );
    assert!(releases[1].prerelease);
    assert_eq!(parse_releases(RELEASES, None).unwrap(), vec![]);
}

#[test]
fn rate_limits_are_read_from_github_headers() {
    let headers = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_string())
        }
    };
    let now = 1_000;
    assert_eq!(rate_limit_until(200, headers(&[]), now), None);
    assert_eq!(rate_limit_until(404, headers(&[]), now), None);
    assert_eq!(
        rate_limit_until(
            403,
            headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "5000")
            ]),
            now
        ),
        Some(5000)
    );
    assert_eq!(
        rate_limit_until(429, headers(&[("retry-after", "30")]), now),
        Some(1030)
    );
    assert_eq!(rate_limit_until(429, headers(&[]), now), Some(1060));
    // A 403 with requests left may still be a secondary limit: a minute.
    assert_eq!(
        rate_limit_until(403, headers(&[("x-ratelimit-remaining", "42")]), now),
        Some(1060)
    );
    // Not a refusal at all: no limit.
    assert_eq!(rate_limit_until(404, headers(&[]), now), None);
    // A reset already in the past still waits a moment.
    assert_eq!(
        rate_limit_until(
            403,
            headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "10")]),
            now
        ),
        Some(1001)
    );
}

#[test]
fn the_state_knows_when_to_check_and_what_is_unseen() {
    let mut state = WatchState::default();
    assert!(state.due(100));
    state.last_check = Some(100);
    assert!(!state.due(100 + CHECK_INTERVAL_SECS - 1));
    assert!(state.due(100 + CHECK_INTERVAL_SECS));
    state.rate_limited_until = Some(100 + CHECK_INTERVAL_SECS + 10);
    assert!(!state.due(100 + CHECK_INTERVAL_SECS));
    assert!(state.rate_limited(100));

    let compared = parse_compare(COMPARE).unwrap();
    state.reports = vec![UpstreamReport {
        id: "coucou".into(),
        commits: compared.commits,
        ..UpstreamReport::default()
    }];
    assert_eq!(state.unseen(), vec!["coucou"]);
    state.mark_seen();
    assert_eq!(state.unseen(), Vec::<String>::new());
    // A newer commit is new again.
    state.reports[0].commits.insert(
        0,
        CommitInfo {
            sha: "4444".into(),
            short: "4444".into(),
            title: "new".into(),
            date: String::new(),
            url: String::new(),
            areas: vec![],
        },
    );
    assert_eq!(state.unseen(), vec!["coucou"]);
    // Nothing new at all is never unseen.
    state.reports[0].commits.clear();
    assert_eq!(state.unseen(), Vec::<String>::new());
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn mock_github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/Louis-CFM/coucou"))
        .and(header("accept", "application/vnd.github+json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"default_branch":"main"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/Louis-CFM/coucou/compare/{PIN}...main"
        )))
        .and(query_param("per_page", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_string(COMPARE))
        .mount(&server)
        .await;
    server
}

#[test]
fn a_check_reports_commits_and_releases_without_credentials() {
    runtime().block_on(async {
        let server = mock_github().await;
        Mock::given(method("GET"))
            .and(path("/repos/Louis-CFM/coucou/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RELEASES))
            .mount(&server)
            .await;
        let client = GithubClient::new(&server.uri()).unwrap();
        let report = client.check(&coucou(), 42).await.unwrap();
        assert_eq!(report.branch, "main");
        assert_eq!(report.ahead_by, 3);
        assert_eq!(report.commits.len(), 3);
        assert_eq!(report.releases.len(), 2);
        assert_eq!(report.checked_at, 42);
        assert!(report.error.is_none());
        for request in server.received_requests().await.unwrap() {
            assert!(request.headers.get("authorization").is_none());
            assert!(request.headers.get("cookie").is_none());
            assert_eq!(
                request.headers.get("user-agent").unwrap().to_str().unwrap(),
                "next-agent-upstream-watch"
            );
        }
    });
}

#[test]
fn a_rate_limit_stops_the_run_and_keeps_old_findings() {
    runtime().block_on(async {
        let server = mock_github().await;
        Mock::given(method("GET"))
            .and(path("/repos/Louis-CFM/coucou/releases"))
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("x-ratelimit-remaining", "0")
                    .insert_header("x-ratelimit-reset", "9999"),
            )
            .mount(&server)
            .await;
        let client = GithubClient::new(&server.uri()).unwrap();
        assert_eq!(
            client.check(&coucou(), 1).await,
            Err(CheckError::RateLimited(9999))
        );

        let old = UpstreamReport {
            id: "coucou".into(),
            pinned: PIN.into(),
            ahead_by: 7,
            ..UpstreamReport::default()
        };
        let (reports, until) = check_all(&client, &[coucou()], std::slice::from_ref(&old), 1).await;
        assert_eq!(until, Some(9999));
        assert_eq!(reports, vec![old]);
    });
}

#[test]
fn other_failures_become_the_upstreams_error() {
    runtime().block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let client = GithubClient::new(&server.uri()).unwrap();
        let (reports, until) = check_all(&client, &[coucou()], &[], 5).await;
        assert_eq!(until, None);
        assert_eq!(reports.len(), 1);
        assert!(reports[0].error.as_deref().unwrap().contains("404"));
        assert_eq!(reports[0].checked_at, 5);
    });
}

/// A compare page of `n` commits titled `{prefix}{i}`, oldest first, as GitHub sends it.
fn compare_page(ahead_by: usize, prefix: &str, n: usize) -> String {
    let commits: Vec<String> = (0..n)
        .map(|i| {
            format!(
                r#"{{"sha": "{i:0>40}", "html_url": "https://github.com/x", "commit": {{"message": "{prefix}{i}", "committer": {{"date": "2026-11-15T00:00:00Z"}}}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"ahead_by": {ahead_by}, "commits": [{}], "files": []}}"#,
        commits.join(",")
    )
}

#[test]
fn past_one_page_the_newest_commits_come_from_the_last_pages() {
    runtime().block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/Louis-CFM/coucou"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"default_branch":"main"}"#),
            )
            .mount(&server)
            .await;
        // 250 commits ahead: pages 1 and 2 are full, page 3 holds the newest 50.
        for (page, prefix, n) in [("1", "p1-", 100), ("2", "p2-", 100), ("3", "p3-", 50)] {
            Mock::given(method("GET"))
                .and(path(format!(
                    "/repos/Louis-CFM/coucou/compare/{PIN}...main"
                )))
                .and(query_param("page", page))
                .respond_with(
                    ResponseTemplate::new(200).set_body_string(compare_page(250, prefix, n)),
                )
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/repos/Louis-CFM/coucou/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;
        let client = GithubClient::new(&server.uri()).unwrap();
        let report = client.check(&coucou(), 1).await.unwrap();
        assert_eq!(report.ahead_by, 250);
        assert_eq!(report.commits.len(), 100);
        // Newest first: the last page reversed, then the newest half of page 2.
        assert_eq!(report.commits[0].title, "p3-49");
        assert_eq!(report.commits[49].title, "p3-0");
        assert_eq!(report.commits[50].title, "p2-99");
        assert_eq!(report.commits[99].title, "p2-50");
    });
}
