//! JSON wire format sent to the UI. Matches `EventEnvelope` in `packages/contracts`.

use next_agent_core::{AgentEvent, EventKind};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// Message pushed over the task `Channel`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamMessage {
    /// A sequenced event (reconciled by `(task_id, seq)` in the UI).
    Event { event: Value },
    /// The task runs in an isolated git worktree (auto mode).
    Worktree {
        task_id: String,
        branch: String,
        root: String,
    },
    /// Informational line for the timeline (mode fallback, demo agent, …).
    Notice {
        task_id: String,
        tone: &'static str,
        text: String,
    },
    /// Changes ready for review once the task has finished.
    Review {
        task_id: String,
        branch: String,
        files: Vec<crate::review::DiffFileDto>,
    },
    /// The task record was written to the vault.
    Documented { task_id: String, path: String },
}

/// Event payload fields for each kind (everything except the envelope).
#[must_use]
pub fn kind_data(kind: &EventKind) -> Value {
    // Single source of truth: the core's serde payload, so new event fields
    // reach the UI without touching this adapter.
    kind.data()
}

/// Full envelope for one event.
#[must_use]
pub fn envelope(event: &AgentEvent) -> Value {
    json!({
        "v": 1,
        "id": format!("{}:{}", event.task_id, event.seq),
        "task_id": event.task_id.as_str(),
        "seq": event.seq,
        "ts": event.ts,
        "kind": event.kind.kind(),
        "data": kind_data(&event.kind),
    })
}

/// Current UTC time as RFC 3339 (second precision), without a time crate.
#[must_use]
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_unix(secs)
}

#[must_use]
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub fn format_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_agent_core::TaskId;

    #[test]
    fn formats_known_timestamps() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(format_unix(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn envelope_matches_contract() {
        let event = AgentEvent {
            task_id: TaskId::from_seed("t"),
            seq: 3,
            ts: "2026-11-12T00:00:00Z".into(),
            kind: EventKind::ToolFinished {
                call_id: "c1".into(),
                ok: true,
                exit_code: Some(0),
                summary: String::new(),
            },
        };
        let value = envelope(&event);
        assert_eq!(value["v"], 1);
        assert_eq!(value["seq"], 3);
        assert_eq!(value["kind"], "tool_finished");
        assert_eq!(value["data"]["call_id"], "c1");
        assert_eq!(value["data"]["ok"], true);
        assert_eq!(value["ts"], "2026-11-12T00:00:00Z");
    }

    #[test]
    fn stream_messages_are_tagged() {
        let message = StreamMessage::Documented {
            task_id: "t".into(),
            path: "/v/x.md".into(),
        };
        let value = serde_json::to_value(message).expect("serialize");
        assert_eq!(value["type"], "documented");
    }
}
