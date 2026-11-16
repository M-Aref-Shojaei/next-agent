// Ported from Hermes Agent (MIT, © 2025 Nous Research): cron/jobs.py, cron/jobs_time.py, cron/missed_runs.py, cron/scheduler_tick.py @ fa1aab3187fa9951fa7c3620e0aa6f922d6636f6
//! Cron jobs: a prompt (plus skills) that runs on a schedule as its own
//! task, only while the app is running.
//!
//! Schedules: a duration (`30m`, recurring), `every 2h`, `every monday
//! 9am` / `weekdays at 9:30`, `in 30m` (one-shot), a 5-field cron
//! expression, or an ISO date-time (one-shot). Times are civil times in the
//! job's IANA time zone (default: the system's).
//!
//! [`tick`] is the at-most-once step: for each due job it moves
//! `next_run_at` to the next slot with a compare-and-swap **before** the
//! run is handed to the caller, so a crash after the claim loses a run
//! instead of repeating it. A run that is late by more than the catch-up
//! window (half the period, clamped to 2 min..2 h, or `grace_minutes`) is
//! skipped and recorded as `missed`.
//!
//! The scheduling API for other modules (channels, the Settings page) is
//! [`schedule_job`], [`list_jobs`], [`update_job`], [`set_paused`] and
//! [`cancel_job`].

use crate::store::{CronJobRow, Store};
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde_json::json;

/// A one-shot may fire this late and still count as on time.
pub const ONESHOT_GRACE_SECONDS: i64 = 120;
const MIN_CATCH_UP: i64 = 120;
const MAX_CATCH_UP: i64 = 7_200;
const MINUTES_PER_HOUR: i64 = 60;
const MINUTES_PER_DAY: i64 = 1_440;
/// Characters of the prompt used as a job's default name.
const DEFAULT_NAME_CHARS: usize = 50;
/// Upper bound on `grace_minutes` (one day).
pub const MAX_GRACE_MINUTES: i64 = MINUTES_PER_DAY;

const HELP: &str = "Use:\n  - Interval: '30m', 'every 30m', 'every 2h' (recurring)\n  - One-shot delay: 'in 30m', 'in 2h' (fires once)\n  - Weekly/daily: 'every monday 9am', 'weekdays at 9am' (recurring)\n  - Cron: '0 9 * * *' (cron expression)\n  - Timestamp: '2026-02-03T14:00:00' (one-shot at time)";

/// One field of a cron expression as the set of values it allows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    allowed: Vec<bool>,
    /// The field was `*` (matters for the day-of-month / day-of-week rule).
    star: bool,
}

impl Field {
    fn has(&self, value: u32) -> bool {
        self.allowed.get(value as usize).copied().unwrap_or(false)
    }
}

/// A parsed 5-field cron expression (minute hour day-of-month month
/// day-of-week). Lists, ranges, steps and English month/day names work;
/// day-of-week 7 is Sunday. When both day fields are restricted, a day
/// matches if either does (Vixie cron).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    minute: Field,
    hour: Field,
    dom: Field,
    month: Field,
    dow: Field,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn parse_value(text: &str, names: &[&str], offset: u32) -> Result<u32, String> {
    let lower = text.to_lowercase();
    if let Some(index) = names.iter().position(|n| *n == lower) {
        return Ok(u32::try_from(index).unwrap_or(0) + offset);
    }
    text.parse::<u32>()
        .map_err(|_| format!("'{text}' is not a number"))
}

fn parse_field(
    text: &str,
    min: u32,
    max: u32,
    names: &[&str],
    offset: u32,
) -> Result<Field, String> {
    let mut allowed = vec![false; max as usize + 1];
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step.parse().map_err(|_| format!("bad step '{step}'"))?;
                if step == 0 {
                    return Err("step must be positive".into());
                }
                (range, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                parse_value(a, names, offset)?,
                parse_value(b, names, offset)?,
            )
        } else {
            let start = parse_value(range, names, offset)?;
            // `a/n` runs from a to the end of the range.
            (start, if part.contains('/') { max } else { start })
        };
        if lo < min || hi > max || lo > hi {
            return Err(format!("'{part}' is outside {min}-{max}"));
        }
        let mut v = lo;
        while v <= hi {
            allowed[v as usize] = true;
            v += step;
        }
    }
    // Vixie cron: a field starting with `*` (`*`, `*/2`) is unrestricted
    // for the day-of-month / day-of-week rule.
    Ok(Field {
        allowed,
        star: text.starts_with('*'),
    })
}

impl CronExpr {
    /// Parse a 5-field expression.
    ///
    /// # Errors
    /// A message naming the bad field.
    pub fn parse(text: &str) -> Result<Self, String> {
        let fields: Vec<&str> = text.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!("Invalid cron expression '{text}': expected 5 fields (minute hour day month weekday), got {}", fields.len()));
        }
        let fail = |what: &str, e: String| format!("Invalid cron expression '{text}': {what}: {e}");
        let mut dow = parse_field(fields[4], 0, 7, &DAYS, 0).map_err(|e| fail("day of week", e))?;
        if dow.allowed[7] {
            dow.allowed[0] = true;
        }
        dow.allowed.truncate(7);
        Ok(Self {
            minute: parse_field(fields[0], 0, 59, &[], 0).map_err(|e| fail("minute", e))?,
            hour: parse_field(fields[1], 0, 23, &[], 0).map_err(|e| fail("hour", e))?,
            dom: parse_field(fields[2], 1, 31, &[], 0).map_err(|e| fail("day of month", e))?,
            month: parse_field(fields[3], 1, 12, &MONTHS, 1).map_err(|e| fail("month", e))?,
            dow,
        })
    }

    fn day_matches(&self, date: Date) -> bool {
        let dom = self.dom.has(u32::try_from(date.day()).unwrap_or(0));
        let dow = self
            .dow
            .has(u32::try_from(date.weekday().to_sunday_zero_offset()).unwrap_or(0));
        match (self.dom.star, self.dow.star) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    /// The first matching instant strictly after `after`, in `tz` (civil
    /// times that do not exist because of a DST gap move forward).
    #[must_use]
    pub fn next_after(&self, after: Timestamp, tz: &TimeZone) -> Option<Timestamp> {
        let start = after.to_zoned(tz.clone()).datetime();
        let mut at = start
            .with()
            .second(0)
            .subsec_nanosecond(0)
            .build()
            .ok()?
            .checked_add(1.minute())
            .ok()?;
        // Four years and a bit covers every leap-day expression.
        let limit = start.date().checked_add(1_500.days()).ok()?;
        while at.date() <= limit {
            if !self.month.has(u32::try_from(at.month()).unwrap_or(0)) {
                let first = Date::new(at.year(), at.month(), 1)
                    .ok()?
                    .checked_add(1.month())
                    .ok()?;
                at = first.to_datetime(Time::midnight());
                continue;
            }
            if !self.day_matches(at.date()) {
                at = at.date().tomorrow().ok()?.to_datetime(Time::midnight());
                continue;
            }
            if !self.hour.has(u32::try_from(at.hour()).unwrap_or(0)) {
                at = at
                    .with()
                    .minute(0)
                    .build()
                    .ok()?
                    .checked_add(1.hour())
                    .ok()?;
                continue;
            }
            if !self.minute.has(u32::try_from(at.minute()).unwrap_or(0)) {
                at = at.checked_add(1.minute()).ok()?;
                continue;
            }
            if let Ok(zoned) = tz.to_zoned(at) {
                if zoned.timestamp() > after {
                    return Some(zoned.timestamp());
                }
            }
            at = at.checked_add(1.minute()).ok()?;
        }
        None
    }
}

/// A parsed schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// Every `minutes`, anchored on the previous run.
    Interval {
        minutes: i64,
    },
    Cron {
        expr: CronExpr,
        text: String,
    },
    /// Once at this instant (unix seconds).
    Once {
        at: i64,
    },
}

/// `30m`, `2h`, `1d`, or a bare unit (`hour` = 1h).
fn parse_duration(text: &str) -> Option<i64> {
    let text = text.trim().to_lowercase();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let n: i64 = if number.is_empty() {
        1
    } else {
        number.parse().ok()?
    };
    let per = match unit.trim() {
        "m" | "min" | "mins" | "minute" | "minutes" => 1,
        "h" | "hr" | "hrs" | "hour" | "hours" => MINUTES_PER_HOUR,
        "d" | "day" | "days" => MINUTES_PER_DAY,
        _ => return None,
    };
    (n > 0).then(|| n.saturating_mul(per))
}

fn parse_clock(text: &str) -> Option<(u32, u32)> {
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    match text.as_str() {
        "noon" | "midday" => return Some((12, 0)),
        "midnight" => return Some((0, 0)),
        _ => {}
    }
    let (body, meridiem) = if let Some(b) = text.strip_suffix("am") {
        (b, Some(false))
    } else if let Some(b) = text.strip_suffix("pm") {
        (b, Some(true))
    } else {
        (text.as_str(), None)
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h, m),
        Some(_) => return None,
        None => (body, "0"),
    };
    if h.is_empty() || h.len() > 2 {
        return None;
    }
    let mut hour: u32 = h.parse().ok()?;
    let minute: u32 = m.parse().ok()?;
    if let Some(pm) = meridiem {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = hour % 12 + if pm { 12 } else { 0 };
    }
    (hour <= 23 && minute <= 59).then_some((hour, minute))
}

fn day_spec(word: &str) -> Option<String> {
    Some(match word {
        "day" | "daily" | "everyday" => "*".into(),
        "weekday" | "weekdays" => "1-5".into(),
        "weekend" | "weekends" => "0,6".into(),
        _ => return None,
    })
}

fn weekday(word: &str) -> Option<u32> {
    Some(match word {
        "sunday" | "sun" => 0,
        "monday" | "mon" => 1,
        "tuesday" | "tue" | "tues" => 2,
        "wednesday" | "wed" | "weds" => 3,
        "thursday" | "thu" | "thur" | "thurs" => 4,
        "friday" | "fri" => 5,
        "saturday" | "sat" => 6,
        _ => return None,
    })
}

/// `every monday 9am`, `weekdays at 9:30`, `daily at noon` → cron text.
fn natural_to_cron(rest: &str) -> Option<String> {
    let cleaned = rest.replace(',', " ");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    let first = tokens.first()?;
    let (dow, mut index) = if let Some(spec) = day_spec(first) {
        (spec, 1)
    } else {
        let mut days = Vec::new();
        let mut index = 0;
        while let Some(token) = tokens.get(index) {
            if *token == "and" {
                index += 1;
                continue;
            }
            match weekday(token) {
                Some(d) => days.push(d.to_string()),
                None => break,
            }
            index += 1;
        }
        if days.is_empty() {
            return None;
        }
        (days.join(","), index)
    };
    if tokens.get(index) == Some(&"at") {
        index += 1;
    }
    let clock = tokens[index..].join(" ");
    let (hour, minute) = if clock.is_empty() {
        (0, 0)
    } else {
        parse_clock(&clock)?
    };
    Some(format!("{minute} {hour} * * {dow}"))
}

fn is_cron_text(text: &str) -> bool {
    let fields: Vec<&str> = text.split_whitespace().collect();
    fields.len() == 5
        && fields.iter().all(|f| {
            f.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '*' | '-' | ',' | '/'))
        })
}

/// Parse a schedule. `now` anchors `in 30m`; `tz` reads a naive ISO time.
///
/// # Errors
/// Upstream's messages, ending with the list of accepted forms.
pub fn parse_schedule(input: &str, now: i64, tz: &TimeZone) -> Result<Schedule, String> {
    let original = input.trim();
    let lower = original.to_lowercase();
    if lower.is_empty() {
        return Err(format!("Schedule is required. {HELP}"));
    }
    let rest = lower.strip_prefix("every ").unwrap_or(&lower).trim();
    if let Some(text) = natural_to_cron(rest) {
        let expr = CronExpr::parse(&text)?;
        return Ok(Schedule::Cron { expr, text });
    }
    if let Some(after) = lower.strip_prefix("in ") {
        let minutes = parse_duration(after).ok_or_else(|| {
            format!("Invalid duration '{after}' after 'in '. Use e.g. 'in 30m', 'in 2h'.")
        })?;
        return Ok(Schedule::Once {
            at: now.saturating_add(minutes.saturating_mul(60)),
        });
    }
    if lower.starts_with("every ") {
        let minutes = parse_duration(rest).ok_or_else(|| {
            format!("Invalid duration: '{rest}'. Use format like '30m', '2h', '1d', or a bare unit like 'hour' (defaults to 1).")
        })?;
        return Ok(Schedule::Interval { minutes });
    }
    if is_cron_text(original) {
        let expr = CronExpr::parse(original)?;
        return Ok(Schedule::Cron {
            expr,
            text: original.to_string(),
        });
    }
    if original.contains('T')
        || original
            .get(..10)
            .is_some_and(|d| d.parse::<Date>().is_ok())
    {
        let at = if let Ok(ts) = original.parse::<Timestamp>() {
            ts
        } else if let Ok(dt) = original.parse::<DateTime>() {
            tz.to_zoned(dt)
                .map_err(|e| format!("Invalid timestamp '{original}': {e}"))?
                .timestamp()
        } else if let Ok(date) = original.parse::<Date>() {
            tz.to_zoned(date.to_datetime(Time::midnight()))
                .map_err(|e| format!("Invalid timestamp '{original}': {e}"))?
                .timestamp()
        } else {
            return Err(format!(
                "Invalid timestamp '{original}': expected e.g. 2026-02-03T14:00:00"
            ));
        };
        return Ok(Schedule::Once { at: at.as_second() });
    }
    if let Some(minutes) = parse_duration(&lower) {
        return Ok(Schedule::Interval { minutes });
    }
    Err(format!("Invalid schedule '{original}'. {HELP}"))
}

/// A short label for lists (`every 30m`, `0 9 * * 1`, `once at …`).
#[must_use]
pub fn describe(schedule: &Schedule, tz: &TimeZone) -> String {
    match schedule {
        Schedule::Interval { minutes } => format!("every {minutes}m"),
        Schedule::Cron { text, .. } => text.clone(),
        Schedule::Once { at } => Timestamp::from_second(*at).map_or_else(
            |_| "once".into(),
            |t| {
                format!(
                    "once at {}",
                    t.to_zoned(tz.clone()).strftime("%Y-%m-%d %H:%M")
                )
            },
        ),
    }
}

/// The next slot strictly after `base` (`None` for a one-shot).
#[must_use]
pub fn next_after(schedule: &Schedule, base: i64, tz: &TimeZone) -> Option<i64> {
    match schedule {
        Schedule::Interval { minutes } => Some(base.saturating_add(minutes.saturating_mul(60))),
        Schedule::Cron { expr, .. } => Timestamp::from_second(base)
            .ok()
            .and_then(|t| expr.next_after(t, tz))
            .map(Timestamp::as_second),
        Schedule::Once { .. } => None,
    }
}

/// The first slot for a new (or resumed) job, or why there is none.
///
/// # Errors
/// A one-shot more than two minutes in the past.
pub fn first_run(schedule: &Schedule, now: i64, tz: &TimeZone) -> Result<i64, String> {
    match schedule {
        Schedule::Once { at } if now - at > ONESHOT_GRACE_SECONDS => Err(format!(
            "Requested one-shot time {} is more than {ONESHOT_GRACE_SECONDS}s in the past and cannot be scheduled.",
            describe(schedule, tz)
        )),
        Schedule::Once { at } => Ok(*at),
        other => next_after(other, now, tz).ok_or_else(|| "This schedule never fires.".into()),
    }
}

/// How late a run may start and still run: `grace_minutes` (at least
/// 2 min), else half the period clamped to 2 min..2 h; 2 min for one-shots.
#[must_use]
pub fn catch_up_window(schedule: &Schedule, grace_minutes: Option<i64>, tz: &TimeZone) -> i64 {
    if let Some(grace) = grace_minutes {
        return (grace * 60).max(MIN_CATCH_UP);
    }
    let period = match schedule {
        Schedule::Interval { minutes } => Some(minutes * 60),
        Schedule::Cron { .. } => {
            let now = crate::learning::sessions::now();
            next_after(schedule, now, tz).and_then(|a| next_after(schedule, a, tz).map(|b| b - a))
        }
        Schedule::Once { .. } => None,
    };
    period.map_or(MIN_CATCH_UP, |p| (p / 2).clamp(MIN_CATCH_UP, MAX_CATCH_UP))
}

/// A job's time zone: its own IANA name, else the system's.
///
/// # Errors
/// An unknown time zone name.
pub fn zone(name: Option<&str>) -> Result<TimeZone, String> {
    match name.filter(|n| !n.trim().is_empty()) {
        Some(name) => TimeZone::get(name).map_err(|_| {
            format!("Unknown time zone '{name}'. Use an IANA name such as Europe/Paris.")
        }),
        None => Ok(TimeZone::system()),
    }
}

/// What a caller asks to schedule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSpec {
    pub workspace_id: String,
    /// Defaults to the first 50 characters of the prompt.
    pub name: Option<String>,
    pub prompt: String,
    pub schedule: String,
    pub skills: Vec<String>,
    /// Gateway role for the run's model (`chat` when unset).
    pub model_role: Option<String>,
    /// `plan`, `manual` or `auto` (default). Bypass is refused.
    pub mode: Option<String>,
    /// Workspace-relative folder the run starts in.
    pub workdir: Option<String>,
    pub timezone: Option<String>,
    pub grace_minutes: Option<i64>,
    /// Runs before the job completes (`None` = forever).
    pub repeat: Option<i64>,
    /// Who scheduled it (`user`, `agent`, `channel:…`).
    pub origin: String,
}

fn validate(spec: &JobSpec, now: i64) -> Result<(Schedule, TimeZone, i64), String> {
    if spec.workspace_id.trim().is_empty() {
        return Err("A job needs a workspace.".into());
    }
    if spec.prompt.trim().is_empty() && spec.skills.is_empty() {
        return Err("A job needs a prompt or at least one skill.".into());
    }
    match spec.mode.as_deref() {
        None | Some("plan" | "manual" | "auto") => {}
        Some("bypassPermissions") => return Err("Scheduled jobs never run in bypass mode.".into()),
        Some(other) => return Err(format!("Unknown mode '{other}'. Use plan, manual or auto.")),
    }
    for skill in &spec.skills {
        crate::learning::skills::validate_name(skill)?;
    }
    if let Some(dir) = &spec.workdir {
        let path = std::path::Path::new(dir);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(
                "workdir must be a folder inside the workspace (relative, no '..').".into(),
            );
        }
    }
    let tz = zone(spec.timezone.as_deref())?;
    let schedule = parse_schedule(&spec.schedule, now, &tz)?;
    if let Some(grace) = spec.grace_minutes {
        if !matches!(schedule, Schedule::Cron { .. }) {
            return Err("grace_minutes only applies to cron and weekday/time schedules.".into());
        }
        if !(0..=MAX_GRACE_MINUTES).contains(&grace) {
            return Err(format!(
                "grace_minutes must be between 0 and {MAX_GRACE_MINUTES}."
            ));
        }
    }
    if spec.repeat.is_some_and(|n| n < 1) {
        return Err("repeat must be at least 1 (omit it to run forever).".into());
    }
    let first = first_run(&schedule, now, &tz)?;
    Ok((schedule, tz, first))
}

fn new_id(seed: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    crate::hash::sha256_hex(format!("{seed}:{nanos}:{n}").as_bytes())[..12].to_string()
}

/// Copy a spec's fields onto a row (shared by create and edit).
fn apply_spec(row: CronJobRow, spec: &JobSpec, schedule: &Schedule) -> CronJobRow {
    let once = matches!(schedule, Schedule::Once { .. });
    CronJobRow {
        name: spec
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(row.name.clone()),
        prompt: spec.prompt.trim().to_string(),
        schedule: spec.schedule.trim().to_string(),
        skills: json!(spec.skills).to_string(),
        model_role: spec.model_role.clone().filter(|r| !r.is_empty()),
        mode: spec.mode.clone().unwrap_or_else(|| "auto".into()),
        workdir: spec.workdir.clone().filter(|w| !w.is_empty()),
        timezone: spec.timezone.clone().filter(|t| !t.is_empty()),
        grace_minutes: spec.grace_minutes,
        repeat_times: if once { Some(1) } else { spec.repeat },
        ..row
    }
}

fn get_job(store: &Store, id: &str) -> Result<CronJobRow, String> {
    store
        .cron_get(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("No job '{id}'."))
}

/// Create a job.
///
/// # Errors
/// Invalid spec (schedule, mode, time zone, …) or a database error.
pub fn schedule_job(store: &Store, spec: &JobSpec, now: i64) -> Result<CronJobRow, String> {
    let (schedule, _, first) = validate(spec, now)?;
    let default_name: String = spec
        .prompt
        .trim()
        .chars()
        .take(DEFAULT_NAME_CHARS)
        .collect();
    let default_name = if default_name.is_empty() {
        spec.skills
            .first()
            .cloned()
            .unwrap_or_else(|| "cron job".into())
    } else {
        default_name
    };
    let fresh = CronJobRow {
        id: new_id(&spec.workspace_id),
        workspace_id: spec.workspace_id.clone(),
        name: default_name,
        state: "scheduled".into(),
        next_run_at: Some(first),
        origin: if spec.origin.is_empty() {
            "user".into()
        } else {
            spec.origin.clone()
        },
        created_at: now,
        ..CronJobRow::default()
    };
    let row = apply_spec(fresh, spec, &schedule);
    store.cron_put(&row).map_err(|e| e.to_string())?;
    Ok(row)
}

/// Replace a job's definition, keeping its id, history and pause state.
///
/// # Errors
/// Unknown job, invalid spec, or a database error.
pub fn update_job(store: &Store, id: &str, spec: &JobSpec, now: i64) -> Result<CronJobRow, String> {
    let old = get_job(store, id)?;
    let (schedule, _, first) = validate(spec, now)?;
    let paused = old.state == "paused";
    let kept = CronJobRow {
        // A one-shot rescheduled from scratch starts its count again.
        run_count: if matches!(schedule, Schedule::Once { .. }) {
            0
        } else {
            old.run_count
        },
        state: if paused {
            "paused".into()
        } else {
            "scheduled".into()
        },
        next_run_at: if paused { None } else { Some(first) },
        ..old
    };
    let row = apply_spec(kept, spec, &schedule);
    store.cron_put(&row).map_err(|e| e.to_string())?;
    Ok(row)
}

/// Every job, or one workspace's.
///
/// # Errors
/// A database error.
pub fn list_jobs(store: &Store, workspace_id: Option<&str>) -> Result<Vec<CronJobRow>, String> {
    store.cron_list(workspace_id).map_err(|e| e.to_string())
}

/// Delete a job. Returns whether it existed. A run already started keeps
/// going (cancel its task to stop it).
///
/// # Errors
/// A database error.
pub fn cancel_job(store: &Store, id: &str) -> Result<bool, String> {
    store.cron_delete(id).map_err(|e| e.to_string())
}

/// Pause (no runs) or resume (next slot from now) a job.
///
/// # Errors
/// Unknown job, a one-shot whose time has passed, or a database error.
pub fn set_paused(store: &Store, id: &str, paused: bool, now: i64) -> Result<CronJobRow, String> {
    let mut row = get_job(store, id)?;
    if paused {
        row.state = "paused".into();
        row.next_run_at = None;
    } else {
        let tz = zone(row.timezone.as_deref())?;
        let schedule = parse_schedule(&row.schedule, row.created_at, &tz)?;
        row.next_run_at = Some(first_run(&schedule, now, &tz)?);
        row.state = "scheduled".into();
    }
    store.cron_put(&row).map_err(|e| e.to_string())?;
    Ok(row)
}

/// A run [`tick`] claimed; the caller starts a task for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueRun {
    pub job: CronJobRow,
    /// The slot this run is for (unix seconds).
    pub scheduled_for: i64,
    /// Seconds after the slot when it was claimed.
    pub lateness: i64,
}

/// Claim every due job (at most once per slot) and skip the ones that
/// missed their catch-up window. Call it periodically while the app runs.
///
/// # Errors
/// A database error while listing jobs (claims that fail are skipped).
pub fn tick(store: &Store, now: i64) -> Result<Vec<DueRun>, String> {
    let mut due = Vec::new();
    for job in store.cron_due(now).map_err(|e| e.to_string())? {
        let Some(slot) = job.next_run_at.filter(|_| job.state == "scheduled") else {
            continue;
        };
        if slot > now {
            continue;
        }
        let Ok(tz) = zone(job.timezone.as_deref()) else {
            claim(store, &job.id, slot, None, "error", false);
            record_run(
                store,
                &job.id,
                now,
                "error",
                Some("unknown time zone"),
                None,
            );
            continue;
        };
        let schedule = match parse_schedule(&job.schedule, job.created_at, &tz) {
            Ok(schedule) => schedule,
            Err(error) => {
                claim(store, &job.id, slot, None, "error", false);
                record_run(store, &job.id, now, "error", Some(&error), None);
                continue;
            }
        };
        let lateness = now - slot;
        let window = match schedule {
            Schedule::Once { .. } => ONESHOT_GRACE_SECONDS,
            _ => catch_up_window(&schedule, job.grace_minutes, &tz),
        };
        let runs = lateness <= window;
        let finished = match schedule {
            Schedule::Once { .. } => true,
            _ => job
                .repeat_times
                .is_some_and(|n| job.run_count + i64::from(runs) >= n),
        };
        let next = if finished {
            None
        } else {
            next_after(&schedule, now, &tz)
        };
        let state = if finished {
            "completed"
        } else if next.is_none() {
            "error"
        } else {
            "scheduled"
        };
        // The claim: whoever moves next_run_at off this slot owns the run.
        if !claim(store, &job.id, slot, next, state, runs) {
            continue;
        }
        if runs {
            due.push(DueRun {
                job: CronJobRow {
                    next_run_at: next,
                    state: state.into(),
                    ..job
                },
                scheduled_for: slot,
                lateness,
            });
        } else {
            let note = format!("missed by {lateness}s (catch-up window {window}s)");
            record_run(store, &job.id, now, "missed", Some(&note), None);
        }
    }
    Ok(due)
}

/// Move a job off `slot`; `true` only if this caller now owns the slot. A
/// database error is logged and counts as not owned, so the run is skipped
/// (at most once).
fn claim(store: &Store, id: &str, slot: i64, next: Option<i64>, state: &str, runs: bool) -> bool {
    store
        .cron_claim(id, slot, next, state, runs)
        .unwrap_or_else(|error| {
            tracing::warn!(cron_id = %id, kind = "cron_claim", %error, "cron slot could not be claimed; skipping it");
            false
        })
}

/// Record a run (and the task it started, if any); a database error is
/// logged, never returned, so recording cannot change what ran.
pub fn record_run(
    store: &Store,
    id: &str,
    at: i64,
    status: &str,
    error: Option<&str>,
    task: Option<&str>,
) {
    if let Err(write) = store.cron_record_run(id, at, status, error, task) {
        tracing::warn!(cron_id = %id, kind = "cron_record_run", status, error = %write, "cron run could not be recorded");
    }
}

/// Skills named by a job.
#[must_use]
pub fn job_skills(job: &CronJobRow) -> Vec<String> {
    serde_json::from_str(&job.skills).unwrap_or_default()
}

/// Run a job now, outside its schedule (Settings "Run now"). Does not move
/// `next_run_at`.
///
/// # Errors
/// Unknown job or a database error.
pub fn run_now(store: &Store, id: &str, now: i64) -> Result<DueRun, String> {
    let job = get_job(store, id)?;
    Ok(DueRun {
        job,
        scheduled_for: now,
        lateness: 0,
    })
}

#[cfg(test)]
#[path = "cron_tests.rs"]
mod cron_tests;
