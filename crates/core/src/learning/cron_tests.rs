#![allow(clippy::assert_is_empty, clippy::bool_assert_comparison)]
use super::*;

fn utc() -> TimeZone {
    TimeZone::UTC
}

fn ts(text: &str) -> i64 {
    text.parse::<Timestamp>().expect("timestamp").as_second()
}

fn next(expr: &str, after: &str, tz: &TimeZone) -> String {
    let expr = CronExpr::parse(expr).expect("expr");
    let at = expr
        .next_after(after.parse().expect("after"), tz)
        .expect("next");
    at.to_zoned(tz.clone())
        .strftime("%Y-%m-%dT%H:%M%:z")
        .to_string()
}

#[test]
fn durations_and_intervals() {
    let now = 1_000;
    assert_eq!(
        parse_schedule("30m", now, &utc()),
        Ok(Schedule::Interval { minutes: 30 })
    );
    assert_eq!(
        parse_schedule("every 2h", now, &utc()),
        Ok(Schedule::Interval { minutes: 120 })
    );
    assert_eq!(
        parse_schedule("every hour", now, &utc()),
        Ok(Schedule::Interval { minutes: 60 })
    );
    assert_eq!(
        parse_schedule("1d", now, &utc()),
        Ok(Schedule::Interval { minutes: 1_440 })
    );
    assert_eq!(
        parse_schedule("in 30m", now, &utc()),
        Ok(Schedule::Once { at: now + 1_800 })
    );
    assert!(parse_schedule("every 5 parsecs", now, &utc())
        .expect_err("bad")
        .starts_with("Invalid duration"));
    assert!(parse_schedule("in soon", now, &utc())
        .expect_err("bad")
        .contains("after 'in '"));
    assert!(parse_schedule("whenever", now, &utc())
        .expect_err("bad")
        .contains("Interval: '30m'"));
    assert!(parse_schedule("0m", now, &utc()).is_err());
}

#[test]
fn natural_weekday_and_time_forms() {
    let cron = |text: &str| match parse_schedule(text, 0, &utc()).expect(text) {
        Schedule::Cron { text, .. } => text,
        other => panic!("{other:?}"),
    };
    assert_eq!(cron("every monday 9am"), "0 9 * * 1");
    assert_eq!(cron("weekdays at 9:30"), "30 9 * * 1-5");
    assert_eq!(cron("every day at noon"), "0 12 * * *");
    assert_eq!(cron("every mon, wed and fri at 5:15pm"), "15 17 * * 1,3,5");
    assert_eq!(cron("weekends 12am"), "0 0 * * 0,6");
    assert!(parse_schedule("every monday 13pm", 0, &utc()).is_err());
    assert!(parse_schedule("every monday 25:00", 0, &utc()).is_err());
}

#[test]
fn cron_expressions_and_iso_one_shots() {
    assert!(matches!(
        parse_schedule("*/15 9-17 * * mon-fri", 0, &utc()),
        Ok(Schedule::Cron { .. })
    ));
    assert!(parse_schedule("61 * * * *", 0, &utc())
        .expect_err("bad")
        .contains("minute"));
    assert!(CronExpr::parse("* * * *").is_err());
    assert!(CronExpr::parse("*/0 * * * *").is_err());
    assert_eq!(
        parse_schedule("2030-01-02T03:04:05Z", 0, &utc()),
        Ok(Schedule::Once {
            at: ts("2030-01-02T03:04:05Z")
        })
    );
    // A naive time is read in the job's zone.
    let paris = TimeZone::get("Europe/Paris").expect("tz");
    assert_eq!(
        parse_schedule("2030-01-02T10:00:00", 0, &paris),
        Ok(Schedule::Once {
            at: ts("2030-01-02T09:00:00Z")
        })
    );
    assert_eq!(
        parse_schedule("2030-01-02", 0, &utc()),
        Ok(Schedule::Once {
            at: ts("2030-01-02T00:00:00Z")
        })
    );
}

#[test]
fn next_fire_times() {
    let tz = utc();
    assert_eq!(
        next("0 9 * * *", "2026-03-10T08:59:00Z", &tz),
        "2026-03-10T09:00+00:00"
    );
    assert_eq!(
        next("0 9 * * *", "2026-03-10T09:00:00Z", &tz),
        "2026-03-11T09:00+00:00"
    );
    assert_eq!(
        next("*/15 * * * *", "2026-03-10T10:07:30Z", &tz),
        "2026-03-10T10:15+00:00"
    );
    assert_eq!(
        next("0 0 29 2 *", "2026-03-01T00:00:00Z", &tz),
        "2028-02-29T00:00+00:00"
    );
    assert_eq!(
        next("0 0 1 jan *", "2026-06-01T00:00:00Z", &tz),
        "2027-01-01T00:00+00:00"
    );
    // Sunday as 0 and as 7.
    assert_eq!(
        next("0 12 * * 7", "2026-03-10T00:00:00Z", &tz),
        "2026-03-15T12:00+00:00"
    );
    // Both day fields restricted: either matches (the 13th, or a Friday).
    assert_eq!(
        next("0 0 13 * 5", "2026-03-10T00:00:00Z", &tz),
        "2026-03-13T00:00+00:00"
    );
    assert_eq!(
        next("0 0 13 * 5", "2026-03-13T00:00:00Z", &tz),
        "2026-03-20T00:00+00:00"
    );
    // `*/2` in day-of-week still counts as unrestricted for that rule.
    assert_eq!(
        next("0 0 1 * */2", "2026-03-10T00:00:00Z", &tz),
        "2026-04-01T00:00+00:00"
    );
}

#[test]
fn time_zones_and_daylight_saving() {
    let ny = TimeZone::get("America/New_York").expect("tz");
    // 9am New York in winter is 14:00 UTC, in summer 13:00 UTC.
    assert_eq!(
        next("0 9 * * *", "2026-01-10T00:00:00Z", &ny),
        "2026-01-10T09:00-05:00"
    );
    assert_eq!(
        next("0 9 * * *", "2026-07-10T00:00:00Z", &ny),
        "2026-07-10T09:00-04:00"
    );
    // 02:30 does not exist on 2026-03-08 in New York: the run moves forward
    // instead of being lost or repeated.
    let gap = next("30 2 * * *", "2026-03-08T05:00:00Z", &ny);
    assert!(gap.starts_with("2026-03-08T03:30"), "{gap}");
    assert_eq!(
        next("30 2 * * *", "2026-03-08T08:00:00Z", &ny),
        "2026-03-09T02:30-04:00"
    );
    assert!(zone(Some("Mars/Olympus")).is_err());
    assert!(zone(None).is_ok());
}

#[test]
fn catch_up_windows() {
    let tz = utc();
    assert_eq!(
        catch_up_window(&Schedule::Interval { minutes: 1 }, None, &tz),
        120
    );
    assert_eq!(
        catch_up_window(&Schedule::Interval { minutes: 60 }, None, &tz),
        1_800
    );
    assert_eq!(
        catch_up_window(&Schedule::Interval { minutes: 1_440 }, None, &tz),
        7_200
    );
    let hourly = parse_schedule("0 * * * *", 0, &tz).expect("cron");
    assert_eq!(catch_up_window(&hourly, None, &tz), 1_800);
    assert_eq!(catch_up_window(&hourly, Some(0), &tz), 120);
    assert_eq!(catch_up_window(&hourly, Some(30), &tz), 1_800);
    assert_eq!(catch_up_window(&Schedule::Once { at: 0 }, None, &tz), 120);
}

fn spec(schedule: &str) -> JobSpec {
    JobSpec {
        workspace_id: "ws".into(),
        prompt: "Summarise yesterday's commits".into(),
        schedule: schedule.into(),
        timezone: Some("UTC".into()),
        origin: "user".into(),
        ..JobSpec::default()
    }
}

#[test]
fn scheduling_validates_specs() {
    let store = Store::open_in_memory().expect("store");
    let now = ts("2026-03-10T08:00:00Z");
    let job = schedule_job(&store, &spec("every 30m"), now).expect("job");
    assert_eq!(job.next_run_at, Some(now + 1_800));
    assert_eq!(job.name, "Summarise yesterday's commits");
    assert_eq!(job.mode, "auto");
    let bypass = JobSpec {
        mode: Some("bypassPermissions".into()),
        ..spec("30m")
    };
    assert!(schedule_job(&store, &bypass, now)
        .expect_err("bypass")
        .contains("never run in bypass"));
    let grace = JobSpec {
        grace_minutes: Some(5),
        ..spec("30m")
    };
    assert!(schedule_job(&store, &grace, now)
        .expect_err("grace")
        .contains("only applies to cron"));
    let outside = JobSpec {
        workdir: Some("../x".into()),
        ..spec("30m")
    };
    assert!(schedule_job(&store, &outside, now).is_err());
    let past = spec("2020-01-01T00:00:00Z");
    assert!(schedule_job(&store, &past, now)
        .expect_err("past")
        .contains("in the past"));
    let empty = JobSpec {
        prompt: " ".into(),
        ..spec("30m")
    };
    assert!(schedule_job(&store, &empty, now).is_err());
    let bad_skill = JobSpec {
        skills: vec!["Bad Name".into()],
        ..spec("30m")
    };
    assert!(schedule_job(&store, &bad_skill, now).is_err());
    assert_eq!(list_jobs(&store, Some("ws")).expect("list").len(), 1);
    assert!(cancel_job(&store, &job.id).expect("cancel"));
    assert!(list_jobs(&store, None).expect("list").is_empty());
}

#[test]
fn tick_claims_each_slot_at_most_once() {
    let store = Store::open_in_memory().expect("store");
    let now = ts("2026-03-10T08:00:00Z");
    let job = schedule_job(&store, &spec("every 1h"), now).expect("job");
    assert!(tick(&store, now).expect("tick").is_empty(), "not due yet");
    let due_at = now + 3_600;
    let first = tick(&store, due_at + 10).expect("tick");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].scheduled_for, due_at);
    assert_eq!(first[0].lateness, 10);
    // next_run_at moved before the run was handed out.
    let row = store.cron_get(&job.id).expect("get").expect("row");
    assert_eq!(row.next_run_at, Some(due_at + 10 + 3_600));
    // A second tick (or a crashed-and-restarted app) does not run it again.
    assert!(tick(&store, due_at + 20).expect("tick").is_empty());
}

#[test]
fn late_runs_inside_the_window_catch_up_once_and_older_ones_are_missed() {
    let store = Store::open_in_memory().expect("store");
    let now = ts("2026-03-10T08:00:00Z");
    let job = schedule_job(&store, &spec("every 1h"), now).expect("job");
    let slot = now + 3_600;
    // 20 minutes late: inside the 30-minute window, runs once.
    assert_eq!(tick(&store, slot + 1_200).expect("tick").len(), 1);
    // The app was closed for a day: the run is skipped and recorded.
    let row = store.cron_get(&job.id).expect("get").expect("row");
    let later = row.next_run_at.expect("next") + 86_400;
    assert!(tick(&store, later).expect("tick").is_empty());
    let row = store.cron_get(&job.id).expect("get").expect("row");
    assert_eq!(row.last_status.as_deref(), Some("missed"));
    assert_eq!(row.next_run_at, Some(later + 3_600));
    assert_eq!(row.state, "scheduled");
}

#[test]
fn one_shots_and_repeat_counts_complete() {
    let store = Store::open_in_memory().expect("store");
    let now = ts("2026-03-10T08:00:00Z");
    let once = schedule_job(&store, &spec("in 10m"), now).expect("once");
    assert_eq!(once.repeat_times, Some(1));
    let due = tick(&store, now + 600).expect("tick");
    assert_eq!(due.len(), 1);
    let row = store.cron_get(&once.id).expect("get").expect("row");
    assert_eq!((row.state.as_str(), row.next_run_at), ("completed", None));
    assert!(tick(&store, now + 700).expect("tick").is_empty());

    let twice = schedule_job(
        &store,
        &JobSpec {
            repeat: Some(2),
            ..spec("every 1h")
        },
        now,
    )
    .expect("twice");
    for hour in 1..=2 {
        let due = tick(&store, now + hour * 3_600).expect("tick");
        assert_eq!(due.len(), 1, "hour {hour}");
        store
            .cron_record_run(&twice.id, now, "started", None, Some("t"))
            .expect("record");
    }
    let row = store.cron_get(&twice.id).expect("get").expect("row");
    assert_eq!(row.state, "completed");
    assert!(tick(&store, now + 3 * 3_600).expect("tick").is_empty());
}

#[test]
fn paused_jobs_do_not_run_and_resume_from_now() {
    let store = Store::open_in_memory().expect("store");
    let now = ts("2026-03-10T08:00:00Z");
    let job = schedule_job(&store, &spec("every 1h"), now).expect("job");
    set_paused(&store, &job.id, true, now).expect("pause");
    assert!(tick(&store, now + 7_200).expect("tick").is_empty());
    let resumed = set_paused(&store, &job.id, false, now + 7_200).expect("resume");
    assert_eq!(resumed.next_run_at, Some(now + 7_200 + 3_600));
    let updated = update_job(
        &store,
        &job.id,
        &JobSpec {
            name: Some("renamed".into()),
            ..spec("every 2h")
        },
        now + 7_200,
    )
    .expect("update");
    assert_eq!(
        (updated.name.as_str(), updated.next_run_at),
        ("renamed", Some(now + 7_200 + 7_200))
    );
    assert_eq!(run_now(&store, &job.id, now).expect("run").job.id, job.id);
}
