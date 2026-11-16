//! The `tracing` layer: turns events into [`LogEntry`] values, keeping only
//! allowlisted fields (from the event and its spans) and redacting every
//! string.

use crate::fields::{is_allowed, is_free_text, INVALID};
use crate::{rank, redact, Inner, LogEntry};
use serde_json::{Map, Value};
use std::fmt;
use std::sync::Arc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_core::Interest;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// The areas an event's `target` is mapped to (docs/architecture/Logging.md).
pub const AREAS: &[&str] = &[
    "app",
    "router",
    "agent",
    "broker",
    "tasks",
    "store",
    "worktree",
    "vault",
    "notes",
    "boards",
    "design",
    "channels",
    "learning",
    "cron",
    "overlay",
    "upstreams",
    "updates",
    "ui",
];

/// Longest token-shaped value, in bytes.
const MAX_TOKEN: usize = 128;

pub(crate) struct DiagLayer {
    pub(crate) inner: Arc<Inner>,
}

/// Allowlisted span fields, stored in the span's extensions.
struct SpanFields(Map<String, Value>);

impl<S> Layer<S> for DiagLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    // The level changes at runtime, so no callsite interest is cached.
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        self.inner.enabled(*metadata.level(), metadata.target())
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visit = Collect::default();
        attrs.record(&mut visit);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(visit.fields));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut visit = Collect::default();
        values.record(&mut visit);
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            if let Some(SpanFields(map)) = ext.get_mut::<SpanFields>() {
                map.extend(visit.fields);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visit = Collect::default();
        event.record(&mut visit);
        let mut span = Map::new();
        if let Some(scope) = ctx.event_scope(event) {
            for s in scope.from_root() {
                if let Some(SpanFields(map)) = s.extensions().get::<SpanFields>() {
                    span.extend(map.clone());
                }
            }
        }
        let (event_name, message, bad_name) = name_event(visit.event, visit.message);
        self.inner.emit(LogEntry {
            ts: String::new(),
            level: level_name(*meta.level()).to_string(),
            target: area(meta.target()).to_string(),
            event: event_name,
            message,
            fields: visit.fields,
            span,
            dropped_fields: visit.dropped + u32::from(bad_name),
        });
    }
}

/// `event = "x"` wins; else a message that is itself a `snake_case` name is
/// the event name; else the event is `unnamed` and the message is kept
/// (redacted). Returns `(event, message, explicit name was invalid)`.
pub(crate) fn name_event(
    explicit: Option<String>,
    message: Option<String>,
) -> (String, Option<String>, bool) {
    let message = message.filter(|m| !m.is_empty());
    match explicit {
        Some(name) if is_event_name(&name) => (name, message.map(|m| redact(&m)), false),
        explicit => match message {
            Some(m) if is_event_name(&m) => (m, None, explicit.is_some()),
            m => (
                "unnamed".to_string(),
                m.map(|m| redact(&m)),
                explicit.is_some(),
            ),
        },
    }
}

/// A stable event name: `snake_case` ASCII, at most 64 characters.
#[must_use]
pub fn is_event_name(name: &str) -> bool {
    name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Map a `tracing` target (an area, or a module path) to an area.
#[must_use]
pub fn area(target: &str) -> &'static str {
    if let Some(found) = AREAS.iter().find(|a| **a == target) {
        return found;
    }
    for segment in target.split("::") {
        let segment = segment.strip_prefix("next_agent_").unwrap_or(segment);
        if let Some(found) = AREAS.iter().find(|a| **a == segment) {
            return found;
        }
        if segment.starts_with("coucou") {
            return "overlay";
        }
    }
    "app"
}

/// Our own events: from our crates (module-path targets starting
/// `next_agent` or `coucou`) or with an explicit one-word target. Other
/// crates' events (hyper, reqwest, tauri …) are kept only at warn and above.
pub(crate) fn is_ours(target: &str) -> bool {
    !target.contains("::") || target.starts_with("next_agent") || target.starts_with("coucou")
}

pub(crate) fn level_name(level: Level) -> &'static str {
    match rank(level) {
        1 => "error",
        2 => "warn",
        3 => "info",
        _ => "debug",
    }
}

/// Clean one allowlisted value: strings are redacted, and a token-shaped
/// field (anything but free text) must not hold whitespace or long text.
pub(crate) fn clean(name: &str, value: Value) -> Option<Value> {
    match value {
        Value::String(s) => {
            if !is_free_text(name) && (s.len() > MAX_TOKEN || s.chars().any(char::is_whitespace)) {
                return Some(Value::String(INVALID.to_string()));
            }
            Some(Value::String(redact(&s)))
        }
        Value::Number(_) | Value::Bool(_) => Some(value),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Collects allowlisted fields; counts the rest.
#[derive(Default)]
pub(crate) struct Collect {
    pub(crate) fields: Map<String, Value>,
    pub(crate) dropped: u32,
    message: Option<String>,
    event: Option<String>,
}

impl Collect {
    pub(crate) fn put(&mut self, name: &str, value: Value) {
        match name {
            "message" => self.message = Some(as_text(value)),
            "event" => self.event = Some(as_text(value)),
            _ if is_allowed(name) => match clean(name, value) {
                Some(v) => {
                    self.fields.insert(name.to_string(), v);
                }
                None => self.dropped += 1,
            },
            _ => self.dropped += 1,
        }
    }
}

fn as_text(value: Value) -> String {
    match value {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

impl Visit for Collect {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let value = serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number);
        self.put(field.name(), value);
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), value.into());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), value.into());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), value.into());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), value.into());
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.put(field.name(), value.to_string().into());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // Skip formatting fields that will be dropped anyway.
        let name = field.name();
        if name == "message" || name == "event" || is_allowed(name) {
            self.put(name, format!("{value:?}").into());
        } else {
            self.dropped += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_map_to_areas() {
        assert_eq!(area("router"), "router");
        assert_eq!(area("next_agent_core::router::gateway"), "router");
        assert_eq!(area("next_agent_core::broker"), "broker");
        assert_eq!(area("next_agent_channels::telegram::api"), "channels");
        assert_eq!(area("next_agent_desktop::learning"), "learning");
        assert_eq!(area("coucou_lib::relay"), "overlay");
        assert_eq!(area("http_fetch"), "app");
        assert_eq!(area("hyper_util::client"), "app");
    }

    #[test]
    fn ownership_of_targets() {
        assert!(is_ours("router"));
        assert!(is_ours("next_agent_core::events"));
        assert!(is_ours("coucou_lib::x"));
        assert!(!is_ours("hyper_util::client::legacy"));
    }

    #[test]
    fn event_names() {
        assert_eq!(
            name_event(None, Some("app_started".into())),
            ("app_started".into(), None, false)
        );
        assert_eq!(
            name_event(Some("provider_call".into()), Some("call done".into())),
            ("provider_call".into(), Some("call done".into()), false)
        );
        assert_eq!(
            name_event(Some("Bad Name".into()), Some("x y".into())),
            ("unnamed".into(), Some("x y".into()), true)
        );
        assert!(!is_event_name("App"));
        assert!(!is_event_name(&"a".repeat(65)));
    }

    #[test]
    fn token_fields_reject_free_text_but_free_text_fields_keep_it() {
        assert_eq!(
            clean("model", "write my essay".into()),
            Some(Value::String(INVALID.into()))
        );
        assert_eq!(
            clean("model", "claude-sonnet-4-5".into()),
            Some("claude-sonnet-4-5".into())
        );
        assert_eq!(
            clean("error", "connection refused".into()),
            Some("connection refused".into())
        );
        assert_eq!(clean("count", 3.into()), Some(3.into()));
        assert_eq!(clean("count", Value::Null), None);
    }
}
