#![allow(clippy::assert_is_empty)]
use super::*;
use std::sync::Mutex;

fn service() -> (tempfile::TempDir, Arc<AppService>, String, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let svc = Arc::new(AppService::new(dir.path().join("db.sqlite")).expect("service"));
    let ws = dir.path().join("proj");
    std::fs::create_dir(&ws).expect("mkdir");
    let opened = svc
        .workspace_open(ws.to_str().expect("utf8"))
        .expect("open");
    (dir, svc, opened.id, PathBuf::from(opened.path))
}

#[test]
fn review_setting_defaults_on_and_round_trips() {
    let (_dir, svc, _, _) = service();
    assert!(svc.learning_settings_get().expect("get").background_review);
    svc.learning_settings_set(&LearningSettings {
        background_review: false,
    })
    .expect("set");
    assert!(!svc.learning_settings_get().expect("get").background_review);
}

#[test]
fn memory_editor_saves_per_workspace_with_caps() {
    let (dir, svc, ws, _) = service();
    let saved = svc
        .memory_save(&ws, "user", "Name: Sam\n§\nLikes tea")
        .expect("save");
    assert_eq!((saved.user.entries, saved.user.limit), (2, 1_375));
    assert!(saved
        .user
        .path
        .starts_with(dir.path().join("memory").to_str().expect("utf8")));
    assert!(svc.memory_save(&ws, "memory", &"x".repeat(3_000)).is_err());
    assert!(svc.memory_save(&ws, "other", "x").is_err());
    assert!(svc.memory_get("unknown").is_err());
}

#[test]
fn skills_can_be_pinned_archived_and_restored() {
    let (_dir, svc, ws, root) = service();
    let skill = root.join(".next-agent/skills/deploy");
    std::fs::create_dir_all(&skill).expect("dir");
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: deploy\ndescription: Use when shipping.\n---\nbody\n",
    )
    .expect("write");
    let list = svc.skills_overview(&ws).expect("list");
    assert_eq!(list.len(), 1);
    let skills_root = list[0].root.clone();
    svc.skill_pin(&ws, &skills_root, "deploy", true)
        .expect("pin");
    assert!(svc.skills_overview(&ws).expect("list")[0].pinned);
    assert!(svc.skill_pin(&ws, "/elsewhere", "deploy", true).is_err());
    assert!(svc
        .skill_read(&ws, "deploy")
        .expect("read")
        .contains("body"));
    svc.skill_archive(&ws, "deploy").expect("archive");
    assert_eq!(svc.skills_overview(&ws).expect("list")[0].state, "archived");
    svc.skill_restore(&ws, &skills_root, "deploy")
        .expect("restore");
    assert_eq!(svc.skills_overview(&ws).expect("list")[0].state, "active");
}

#[test]
fn cron_jobs_dispatch_as_their_own_tasks() {
    let (_dir, svc, ws, _) = service();
    let job = svc
        .cron_save(&CronInput {
            workspace_id: ws.clone(),
            prompt: "List files".into(),
            schedule: "every 1h".into(),
            mode: Some("plan".into()),
            ..CronInput::default()
        })
        .expect("save");
    assert_eq!(svc.list_jobs(Some(&ws)).expect("list").len(), 1);
    assert!(svc
        .cron_save(&CronInput {
            workspace_id: ws.clone(),
            prompt: "x".into(),
            schedule: "30m".into(),
            mode: Some("bypassPermissions".into()),
            ..CronInput::default()
        })
        .is_err());
    let paused = svc.cron_pause(&job.id, true).expect("pause");
    assert_eq!(
        (paused.state.as_str(), paused.next_run_at),
        ("paused", None)
    );
    svc.cron_pause(&job.id, false).expect("resume");

    let seen: Arc<Mutex<Vec<StreamMessage>>> = Arc::default();
    let log = Arc::clone(&seen);
    let sink: Sink = Arc::new(move |m| log.lock().expect("lock").push(m));
    let run = cron::run_now(&svc.store().expect("store"), &job.id, sessions::now()).expect("run");
    let task_id = svc.cron_dispatch(&run, sink).expect("dispatch");
    svc.task_wait(&task_id);
    let row = svc.list_jobs(None).expect("list").remove(0);
    assert_eq!(
        (
            row.last_status.as_deref(),
            row.last_task_id.as_deref(),
            row.run_count
        ),
        (Some("started"), Some(task_id.as_str()), 0)
    );
    assert!(!seen.lock().expect("lock").is_empty());
    assert!(svc.cancel_job(&job.id).expect("cancel"));
    assert!(svc.list_jobs(None).expect("list").is_empty());
}

#[test]
fn cron_runs_get_skills_in_front_of_the_prompt() {
    let (_dir, svc, _ws, root) = service();
    let skill = root.join(".next-agent/skills/deploy");
    std::fs::create_dir_all(&skill).expect("dir");
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: deploy\ndescription: d\n---\nRun make ship.\n",
    )
    .expect("write");
    let prompt = svc.with_skills(&root, &["deploy".into(), "missing".into()], "Ship it.");
    assert!(prompt.contains("<skill name=\"deploy\">"));
    assert!(prompt.contains("Run make ship."));
    assert!(prompt.contains("`missing` could not be loaded"));
    assert!(prompt.ends_with("Ship it."));
    assert_eq!(svc.with_skills(&root, &[], "plain"), "plain");
}

fn null_sink() -> Sink {
    Arc::new(|_| {})
}

#[test]
fn a_bypass_job_written_behind_the_apps_back_never_runs() {
    let (_dir, svc, ws, _) = service();
    let job = svc
        .cron_save(&CronInput {
            workspace_id: ws,
            prompt: "p".into(),
            schedule: "every 1h".into(),
            mode: Some("plan".into()),
            ..CronInput::default()
        })
        .expect("save");
    let mut row = svc
        .store()
        .expect("store")
        .cron_get(&job.id)
        .expect("get")
        .expect("row");
    row.mode = "bypassPermissions".into();
    svc.store().expect("store").cron_put(&row).expect("tamper");
    let run = cron::run_now(&svc.store().expect("store"), &job.id, sessions::now()).expect("run");
    let error = svc.cron_dispatch(&run, null_sink()).expect_err("refused");
    assert!(error.contains("never run"), "{error}");
    let row = svc.list_jobs(None).expect("list").remove(0);
    assert_eq!(row.last_status.as_deref(), Some("error"));
    assert_eq!(row.last_task_id, None);
}

#[cfg(unix)]
#[test]
fn a_job_workdir_cannot_leave_the_workspace_through_a_symlink() {
    let (dir, svc, ws, root) = service();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, root.join("out")).expect("link");
    std::fs::create_dir(root.join("inside")).expect("inside");
    let job = svc
        .cron_save(&CronInput {
            workspace_id: ws.clone(),
            prompt: "p".into(),
            schedule: "every 1h".into(),
            mode: Some("plan".into()),
            workdir: Some("out".into()),
            ..CronInput::default()
        })
        .expect("save");
    let run = cron::run_now(&svc.store().expect("store"), &job.id, sessions::now()).expect("run");
    let error = svc.cron_dispatch(&run, null_sink()).expect_err("refused");
    assert!(error.contains("inside the workspace"), "{error}");
    // `..` and absolute paths are refused when the job is saved.
    for bad in ["../x", "/etc"] {
        assert!(
            svc.cron_save(&CronInput {
                workspace_id: ws.clone(),
                prompt: "p".into(),
                schedule: "1h".into(),
                workdir: Some(bad.into()),
                ..CronInput::default()
            })
            .is_err(),
            "{bad}"
        );
    }
    // A real folder inside works.
    let ok = svc
        .cron_save(&CronInput {
            workspace_id: ws.clone(),
            prompt: "p".into(),
            schedule: "every 1h".into(),
            mode: Some("plan".into()),
            workdir: Some("inside".into()),
            ..CronInput::default()
        })
        .expect("save");
    let run = cron::run_now(&svc.store().expect("store"), &ok.id, sessions::now()).expect("run");
    let task = svc.cron_dispatch(&run, null_sink()).expect("dispatch");
    svc.task_wait(&task);
    // Unknown workspaces are refused.
    assert!(svc
        .cron_save(&CronInput {
            workspace_id: "nope".into(),
            prompt: "p".into(),
            schedule: "1h".into(),
            ..CronInput::default()
        })
        .is_err());
}
