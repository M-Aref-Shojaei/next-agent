// Ported from opencode (MIT, © 2025 opencode): packages/core/src/session/compaction.ts @ cbe71595e1f370ff91b3081872b96bbe56519c41
//! Context compaction for the built-in agent loop.
//!
//! **When.** The prompt size is estimated as the last provider-measured usage
//! (input + cache + output + reasoning) plus the characters/4 of everything
//! added since. Compaction runs once that reaches the ceiling:
//! `window − max(15% · window, 16k)`, where the 16k term applies only to
//! windows of at least 32k. A provider "too long" rejection also triggers it
//! ([`Reason::Overflow`]).
//!
//! **What the summariser sees.** Stored history is never edited. The
//! summariser first gets the whole conversation unchanged (same prefix, so it
//! reuses the prompt cache), then the history before the verbatim tail, and
//! only when neither fits a text transcript shrunk oldest-exchange-first in
//! five levels: whole → tool outputs clipped to 2,000 chars from each end →
//! tool outputs removed and inputs clipped → user text only →
//! `[N older exchanges omitted]`. The newest exchange is never omitted. When
//! the provider rejects the summary request as too long, the next attempt
//! targets 85%, 70%, 50% and then 35% of the rejected size before giving up.
//!
//! **What stays.** The newest ~15k tokens (starting at an exchange boundary so
//! tool calls and results stay paired) are kept verbatim as text beside the
//! summary. Later requests open with one user message:
//! `<conversation-checkpoint><summary>…</summary><recent-context>…</recent-context></conversation-checkpoint>`.
//!
//! **Format.** The summary must use the opencode template (Objective,
//! Requirements, Decisions, Work State, Next Move, Relevant Files ≤ 15,
//! Important Context). A reply with none of the headings gets one nudge, then
//! compaction fails. An existing checkpoint is *updated*: newer history wins.
//!
//! Deviation from upstream: a Next Agent task usually has one user prompt
//! followed by many tool rounds, so an exchange also starts at each assistant
//! message that follows tool results. Without that, a task would be a single
//! exchange and nothing could ever be compacted. Images and PDFs are not part
//! of [`ChatMessage`] yet; [`IMAGE_TOKEN_ESTIMATE`] / [`PDF_TOKEN_ESTIMATE`]
//! are kept for when they are. The optional "precompute" feature is skipped.

use crate::llm::{ChatMessage, ChatRequest, CompletionClient, LlmError, TokenUsage};
use serde_json::Value;

/// The least of the window kept free for the reply and the summary.
pub const RESERVE_MIN: u64 = 16_000;
/// Share of the window kept free before automatic compaction.
pub const RESERVE_SHARE: f64 = 0.15;
/// Window assumed for the summary request when the model's is unknown.
pub const UNKNOWN_WINDOW: u64 = 200_000;
/// Tokens of recent conversation kept verbatim beside the summary.
pub const KEEP_TOKENS: u64 = 15_000;
/// After each "too long" rejection, the next attempt aims at this share of
/// the first rejected request's size.
pub const SHRINK_STEPS: [f64; 4] = [0.85, 0.7, 0.5, 0.35];
/// Characters kept, from each end, of a clipped transcript entry.
pub const TRANSCRIPT_CLIP: usize = 2_000;
pub const IMAGE_TOKEN_ESTIMATE: u64 = 1_500;
pub const PDF_TOKEN_ESTIMATE: u64 = 2_000;
const CHARS_PER_TOKEN: f64 = 4.0;

const WHOLE: usize = 0;
const OUTPUTS_CLIPPED: usize = 1;
const OUTPUTS_REMOVED: usize = 2;
const USER_ONLY: usize = 3;
const OMITTED: usize = 4;
const LEVELS: [usize; 5] = [WHOLE, OUTPUTS_CLIPPED, OUTPUTS_REMOVED, USER_ONLY, OMITTED];
const LEVEL_COUNT: usize = LEVELS.len();

const CHECKPOINT_OPEN: &str = "<conversation-checkpoint>";
const CHECKPOINT_CLOSE: &str = "</conversation-checkpoint>";

pub const SUMMARY_TEMPLATE: &str = "You MUST use this format for your response (you may omit sections that aren't applicable). Do not include the <template> tags in your response.
<template>
## Objective
- [one or two brief sentences describing what the user is trying to accomplish]

## Requirements
- [constraints, preferences, requirements, and scope boundaries stated by the user, or \"(none)\"]

## Decisions
- [decisions already made and why, or \"(none)\"]

## Work State
Break the objective into smaller goals and report which are completed, which are being worked on, and which are blocked.
### Completed
- [goals that have been completed; otherwise \"(none)\"]

### Active
- [goals currently being worked on; otherwise \"(none)\"]

### Blocked
- [anything blocking progress, and why; otherwise \"(none)\"]

## Next Move
1. [ordered list of next actions, or \"(none)\"]

## Relevant Files
List the files and directories, other than the current working directory, that another agent would need to open to continue this work. Include at most 15, most important first. Do not list every file that was read or changed. Include paths outside the current working directory when relevant. If none, write \"(none)\".
- `[file or directory path]`: [brief reason it matters]

## Important Context
- [facts the next agent cannot continue without and cannot easily find on its own; or \"(none)\"]
</template>";

const SUMMARY_RULES: &str = "Rules:
- Keep each section concise. Use terse, single-line bullets, not prose paragraphs or nested lists.
- Prefer short references over detailed restatement. It is fine to leave out information the next agent can recover from the code or the files listed above.
- Preserve exact file paths, symbols, commands, error strings, URLs, and identifiers.
- Carry forward only user questions or requests that remain unanswered or require further action. Do not repeat ones that newer history has answered or resolved. Preserve exact wording when carrying one forward.
- Preserve consequential workflow state, including whether changes are uncommitted, committed, pushed, under review, or merged.
- Do not mention the summary process or that context was compacted.";

/// Reminder sent once when the reply skips the template.
pub const NUDGE: &str = "The previous response did not fill in the required summary template. Do not call tools. Return the summary as text using the exact section headings from the template.";

/// The template's `##` headings, in order.
#[must_use]
pub fn headings() -> Vec<&'static str> {
    SUMMARY_TEMPLATE
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect()
}

/// Whether `text` fills the template (has at least one of its headings).
#[must_use]
pub fn filled(text: &str) -> bool {
    let wanted = headings();
    text.lines().any(|line| wanted.contains(&line.trim()))
}

/// The summary instruction; `update` when a checkpoint already exists.
#[must_use]
pub fn build_prompt(update: bool) -> String {
    let shared = [
        "Summarize only what the user and the assistant said and did. Leave out instructions and setup the assistant was given rather than told by the user: repository conventions, instruction files, and environment details like the task ID. The next agent receives current versions of all of these separately.",
        SUMMARY_TEMPLATE,
        SUMMARY_RULES,
        "Do not continue the task or call tools.",
        "Return only the structured summary in the requested format. Do not include a preamble, explanation, or other commentary.",
    ];
    let mut parts: Vec<&str> = Vec::new();
    if update {
        parts.extend([
            "Update the existing checkpoint in the conversation above into one consolidated summary.",
            "Newer history always takes precedence over the existing checkpoint. Preserve previous information unless newer history clearly contradicts, supersedes, resolves, or makes it stale. If something is no longer relevant to continuing the work, you may remove it.",
            "Incorporate newer requirements, decisions, progress, and context. Reconcile Work State and Next Move: move completed work out of Active, remove resolved blockers and answered questions, and preserve unresolved or pending work.",
            "Return only the updated Markdown sections. Do not reproduce the `<conversation-checkpoint>`, `<summary>`, or `<recent-context>` wrapper tags from the previous checkpoint.",
        ]);
    } else {
        parts.push("You MUST summarize the conversation above into a structured summary that will be given to another agent to resume the work.");
    }
    parts.extend(shared);
    parts.join("\n\n")
}

// ---------------------------------------------------------------------------
// Estimation
// ---------------------------------------------------------------------------

/// Characters / 4, rounded (upstream `Token.estimate`).
#[must_use]
pub fn estimate_tokens(text: &str) -> u64 {
    tokens_for_chars(text.chars().count())
}

fn tokens_for_chars(chars: usize) -> u64 {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let tokens = (chars as f64 / CHARS_PER_TOKEN).round() as u64;
    tokens
}

/// Estimated tokens of one message (text plus tool-call names and inputs).
#[must_use]
pub fn estimate_message(message: &ChatMessage) -> u64 {
    let text = message.content.as_deref().map_or(0, estimate_tokens);
    let calls: u64 = message
        .tool_calls
        .iter()
        .map(|call| {
            tokens_for_chars(
                call.function.name.chars().count() + call.function.arguments.chars().count(),
            )
        })
        .sum();
    text + calls
}

/// Estimated tokens of a whole request.
#[must_use]
pub fn estimate_request(messages: &[ChatMessage], tools: &[Value]) -> u64 {
    messages.iter().map(estimate_message).sum::<u64>()
        + tools
            .iter()
            .map(|tool| estimate_tokens(&tool.to_string()))
            .sum::<u64>()
}

/// A prompt size: provider-measured plus estimated since the measurement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Estimate {
    pub measured: u64,
    pub estimated: u64,
}

impl Estimate {
    #[must_use]
    pub fn total(&self) -> u64 {
        self.measured + self.estimated
    }
}

/// Size of the next prompt. `anchor` is the index of the newest assistant
/// message whose completion reported `usage`; its usage covers everything up
/// to and including that message, so only later messages are estimated.
#[must_use]
pub fn estimate_prompt(
    messages: &[ChatMessage],
    tools: &[Value],
    anchor: Option<(usize, TokenUsage)>,
) -> Estimate {
    match anchor {
        Some((index, usage)) if index < messages.len() && usage.total() > 0 => Estimate {
            measured: usage.total(),
            estimated: messages[index + 1..].iter().map(estimate_message).sum(),
        },
        _ => Estimate {
            measured: 0,
            estimated: estimate_request(messages, tools),
        },
    }
}

/// The largest prompt sent before compacting; `u64::MAX` for an unknown
/// (zero) window, where only a provider rejection can trigger compaction.
#[must_use]
pub fn ceiling(window: u64) -> u64 {
    if window == 0 {
        return u64::MAX;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let share = (window as f64 * RESERVE_SHARE).floor() as u64;
    let min = if window >= 2 * RESERVE_MIN {
        RESERVE_MIN
    } else {
        0
    };
    window - share.max(min)
}

/// The largest summary request that leaves `RESERVE_MIN` for the summary.
#[must_use]
pub fn summary_room(window: u64) -> u64 {
    window.saturating_sub(RESERVE_MIN)
}

/// Whether compaction is due before sending `estimate`.
#[must_use]
pub fn due(estimate: Estimate, window: u64) -> bool {
    estimate.total() >= ceiling(window)
}

/// Whether a transport error means "the prompt is too long".
#[must_use]
pub fn is_context_overflow(error: &LlmError) -> bool {
    let LlmError::Http { status, body } = error else {
        return false;
    };
    if *status == 413 {
        return true;
    }
    let body = body.to_ascii_lowercase();
    matches!(*status, 400 | 422)
        && [
            "context length",
            "context_length",
            "context window",
            "maximum context",
            "prompt is too long",
            "input is too long",
            "too many tokens",
            "too many input tokens",
        ]
        .iter()
        .any(|needle| body.contains(needle))
}

// ---------------------------------------------------------------------------
// Conversation shape
// ---------------------------------------------------------------------------

/// A previous checkpoint at the head of the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub summary: String,
    pub recent: String,
}

/// Render a checkpoint message body.
#[must_use]
pub fn checkpoint_text(summary: &str, recent: &str) -> String {
    let mut out = format!(
        "{CHECKPOINT_OPEN}\n<summary>\n{}\n</summary>\n",
        summary.trim()
    );
    if !recent.trim().is_empty() {
        out.push_str("<recent-context>\n");
        out.push_str(recent.trim());
        out.push_str("\n</recent-context>\n");
    }
    out.push_str(CHECKPOINT_CLOSE);
    out.push_str("\n\nContinue the task from this checkpoint.");
    out
}

/// Parse the checkpoint that opens `messages` (after the system prompt).
#[must_use]
pub fn previous_checkpoint(messages: &[ChatMessage]) -> Option<Checkpoint> {
    let first = messages.iter().find(|m| m.role != "system")?;
    if first.role != "user" {
        return None;
    }
    let text = first.content.as_deref()?;
    if !text.starts_with(CHECKPOINT_OPEN) {
        return None;
    }
    let between = |open: &str, close: &str| {
        let start = text.find(open)? + open.len();
        let end = text[start..].find(close)? + start;
        Some(text[start..end].trim().to_string())
    };
    Some(Checkpoint {
        summary: between("<summary>", "</summary>")?,
        recent: between("<recent-context>", "</recent-context>").unwrap_or_default(),
    })
}

/// Index of the first message after the system prompt (and after a previous
/// checkpoint, which is not part of any exchange).
fn body_start(messages: &[ChatMessage]) -> usize {
    let mut start = usize::from(messages.first().is_some_and(|m| m.role == "system"));
    if previous_checkpoint(messages).is_some() {
        start += 1;
    }
    start
}

/// Whether message `index` opens a new exchange: a user message, or an
/// assistant message right after tool results (a new round).
fn opens_exchange(messages: &[ChatMessage], index: usize) -> bool {
    let message = &messages[index];
    message.role == "user"
        || (message.role == "assistant" && index > 0 && messages[index - 1].role == "tool")
}

/// Plain-text rendering of one message (upstream `messageToText`).
#[must_use]
pub fn message_to_text(message: &ChatMessage) -> String {
    flatten_message(message, WHOLE)
}

/// Start of the verbatim tail (index into `messages`); `messages.len()` keeps
/// nothing. `None` when there is nothing to compact.
#[must_use]
pub fn split_conversation(messages: &[ChatMessage], keep: u64) -> Option<usize> {
    let start = body_start(messages);
    let entries: Vec<(usize, u64)> = (start..messages.len())
        .filter_map(|index| {
            let text = message_to_text(&messages[index]);
            (!text.is_empty()).then(|| (index, estimate_tokens(&text)))
        })
        .collect();
    if entries.is_empty() {
        // Nothing since the checkpoint, but its verbatim tail may be what
        // overflows: summarise the checkpoint itself and keep nothing.
        let kept_recent = previous_checkpoint(messages).is_some_and(|c| !c.recent.is_empty());
        return kept_recent.then_some(messages.len());
    }
    // Drop the oldest entries until the rest fit, but keep the newest one.
    let mut total = 0;
    let mut dropped = entries.len();
    while dropped > 0 {
        let next = total + entries[dropped - 1].1;
        if next > keep {
            break;
        }
        total = next;
        dropped -= 1;
    }
    let dropped = dropped.min(entries.len() - 1);
    let boundary = |limit: usize| {
        entries
            .iter()
            .enumerate()
            .filter(|(position, (index, _))| *position <= limit && opens_exchange(messages, *index))
            .map(|(position, _)| position)
            .next_back()
    };
    if let Some(position) = boundary(dropped).filter(|p| *p > 0) {
        return Some(entries[position].0);
    }
    // Everything fits: keep only the latest exchange.
    if let Some(position) = boundary(entries.len() - 1).filter(|p| *p > 0) {
        return Some(entries[position].0);
    }
    let kept_recent = previous_checkpoint(messages).is_some_and(|c| !c.recent.is_empty());
    Some(if kept_recent { start } else { messages.len() })
}

fn clip(value: &str) -> String {
    let count = value.chars().count();
    if count <= TRANSCRIPT_CLIP {
        return value.to_string();
    }
    let half = TRANSCRIPT_CLIP / 2;
    let head: String = value.chars().take(half).collect();
    let tail: String = value.chars().skip(count - half).collect();
    format!(
        "{head}\n[… {} characters omitted …]\n{tail}",
        count - TRANSCRIPT_CLIP
    )
}

/// One message as transcript lines at a shortening level.
fn flatten_message(message: &ChatMessage, level: usize) -> String {
    if message.role == "system" || level == OMITTED {
        return String::new();
    }
    if level >= USER_ONLY && message.role != "user" {
        return String::new();
    }
    let mut lines = Vec::new();
    let text = message.content.as_deref().unwrap_or_default();
    match message.role.as_str() {
        "tool" => {
            let label = if text.starts_with("error:") {
                "Tool error"
            } else {
                "Tool result"
            };
            if level >= OUTPUTS_REMOVED {
                lines.push(format!("[{label} omitted]"));
            } else if level >= OUTPUTS_CLIPPED {
                lines.push(format!("[{label}]: {}", clip(text)));
            } else {
                lines.push(format!("[{label}]: {text}"));
            }
        }
        role => {
            if !text.is_empty() {
                let shown = if level >= USER_ONLY {
                    clip(text)
                } else {
                    text.to_string()
                };
                let who = if role == "user" { "User" } else { "Assistant" };
                lines.push(format!("[{who}]: {shown}"));
            }
            for call in &message.tool_calls {
                let input = &call.function.arguments;
                let input = if level >= OUTPUTS_REMOVED {
                    clip(input)
                } else {
                    input.clone()
                };
                lines.push(format!(
                    "[Assistant tool call]: {}({input})",
                    call.function.name
                ));
            }
        }
    }
    lines.join("\n")
}

/// Rendered text of each exchange at every level in `LEVELS`, oldest first.
/// The carried-over `recent` summary (if any) comes first; exchanges with no
/// whole-level text are dropped.
fn exchanges(recent: &str, messages: &[ChatMessage], from: usize) -> Vec<[String; LEVEL_COUNT]> {
    let mut groups: Vec<Vec<&ChatMessage>> = Vec::new();
    for index in from..messages.len() {
        if groups.is_empty() || opens_exchange(messages, index) {
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push(&messages[index]);
        }
    }
    let mut exchanges: Vec<[String; LEVEL_COUNT]> = Vec::new();
    if !recent.is_empty() {
        exchanges.push(LEVELS.map(|level| {
            if level == OMITTED {
                String::new()
            } else {
                recent.to_string()
            }
        }));
    }
    for group in groups {
        let renders = LEVELS.map(|level| {
            group
                .iter()
                .map(|message| flatten_message(message, level))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        });
        if !renders[WHOLE].is_empty() {
            exchanges.push(renders);
        }
    }
    exchanges
}

/// Text transcript of `messages[from..]`, shrunk to fit `target` tokens
/// together with `lead`. `None` when even the most shortened form does not fit.
fn flatten_and_drop_oldest(
    lead: &[ChatMessage],
    recent: &str,
    messages: &[ChatMessage],
    from: usize,
    target: u64,
) -> Option<Vec<ChatMessage>> {
    let exchanges = exchanges(recent, messages, from);
    if exchanges.is_empty() {
        return None;
    }
    let note = |omitted: usize| {
        if omitted == 0 {
            String::new()
        } else {
            format!(
                "[{omitted} older {} omitted]\n\n",
                if omitted == 1 {
                    "exchange"
                } else {
                    "exchanges"
                }
            )
        }
    };
    let size = |text: &str| {
        if text.is_empty() {
            0
        } else {
            estimate_tokens(text) + 1
        }
    };
    let fixed = estimate_request(lead, &[]) + estimate_tokens(&note(exchanges.len()));
    let room = target.saturating_sub(fixed);
    let mut levels = vec![WHOLE; exchanges.len()];
    let mut total: u64 = exchanges.iter().map(|r| size(&r[WHOLE])).sum();
    let newest = exchanges.len() - 1;
    for &level in &LEVELS[1..] {
        for index in 0..exchanges.len() {
            if total <= room {
                break;
            }
            // The newest exchange is shortened but never left out.
            if index == newest && exchanges[index][level].is_empty() {
                continue;
            }
            total = total + size(&exchanges[index][level]) - size(&exchanges[index][levels[index]]);
            levels[index] = level;
        }
    }
    if total > room {
        return None;
    }
    let omitted = levels
        .iter()
        .enumerate()
        .filter(|(index, level)| exchanges[*index][**level].is_empty())
        .count();
    let text = exchanges
        .iter()
        .zip(&levels)
        .map(|(renders, level)| renders[*level].as_str())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut out = lead.to_vec();
    push_user(&mut out, format!("{}{text}", note(omitted)));
    Some(out)
}

/// Append a user message, merging into a trailing user message so no two
/// messages in a row share a role (prompt-cache invariant).
pub fn push_user(messages: &mut Vec<ChatMessage>, text: String) {
    if let Some(last) = messages.last_mut() {
        if last.role == "user" && last.tool_calls.is_empty() {
            let merged = match last.content.take() {
                Some(previous) if !previous.is_empty() => format!("{previous}\n\n{text}"),
                _ => text,
            };
            last.content = Some(merged);
            return;
        }
    }
    messages.push(ChatMessage::user(text));
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

/// Why compaction runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The estimate reached the ceiling.
    Auto,
    /// The provider rejected the prompt as too long.
    Overflow,
}

impl Reason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Overflow => "overflow",
        }
    }
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionConfig {
    /// The model's context window in tokens; 0 = unknown.
    pub window: u64,
    /// Tokens kept verbatim beside the summary.
    pub keep: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            window: 0,
            keep: KEEP_TOKENS,
        }
    }
}

/// How the summariser saw the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptMode {
    /// The whole conversation, unchanged (cache reuse).
    Unchanged,
    /// The conversation before the verbatim tail, unchanged.
    History,
    /// A shrunk text transcript.
    Text,
}

/// A finished compaction.
#[derive(Debug, Clone, PartialEq)]
pub struct Compacted {
    /// The new conversation: the original system prompt and one checkpoint
    /// user message.
    pub messages: Vec<ChatMessage>,
    pub summary: String,
    pub tokens_before: u64,
    pub tokens_after: u64,
    /// Messages folded into the verbatim recent context.
    pub kept_messages: usize,
    pub mode: TranscriptMode,
    /// Summary requests sent (including the nudge and shrink retries).
    pub requests: usize,
}

/// Why compaction did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionError {
    NothingToCompact,
    /// The summary did not fill the template, or could not be made to fit.
    Failed(String),
    /// A transport error other than "too long".
    Llm(String),
}

impl std::fmt::Display for CompactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToCompact => f.write_str("nothing to compact yet"),
            Self::Failed(message) => write!(f, "compaction failed: {message}"),
            Self::Llm(message) => write!(f, "compaction request failed: {message}"),
        }
    }
}

impl std::error::Error for CompactionError {}

enum Sent {
    Ok(String),
    TooLong,
    Err(CompactionError),
}

/// Summarise `messages` (system prompt first) and return the compacted
/// conversation. The system prompt is carried over byte for byte.
///
/// # Errors
/// [`CompactionError`] when there is nothing to compact, the summary never
/// fills the template, the request cannot be shrunk enough, or the
/// transport fails.
#[allow(clippy::too_many_lines)] // one retry loop, kept together to mirror upstream `deliver`
pub fn compact(
    client: &dyn CompletionClient,
    model: &str,
    messages: &[ChatMessage],
    tools: &[Value],
    config: &CompactionConfig,
    reason: Reason,
) -> Result<Compacted, CompactionError> {
    let keep_index =
        split_conversation(messages, config.keep).ok_or(CompactionError::NothingToCompact)?;
    let previous = previous_checkpoint(messages);
    let prompt = build_prompt(previous.is_some());
    let overhead = estimate_tokens(&prompt) + estimate_tokens(NUDGE);
    let window = if config.window == 0 {
        UNKNOWN_WINDOW
    } else {
        config.window
    };
    let budget = ceiling(window);
    let tokens_before = estimate_request(messages, tools);

    let full: Vec<ChatMessage> = messages.to_vec();
    let history: Vec<ChatMessage> = messages[..keep_index].to_vec();
    let system: Vec<ChatMessage> = messages
        .iter()
        .take_while(|m| m.role == "system")
        .cloned()
        .collect();
    let mut lead = system.clone();
    if let Some(c) = &previous {
        lead.push(ChatMessage::user(checkpoint_text(&c.summary, "")));
    }
    let recent = previous
        .as_ref()
        .map_or_else(String::new, |c| c.recent.clone());
    let body = body_start(messages);

    let mut target = budget.saturating_sub(overhead);
    let mut fit = match reason {
        Reason::Overflow => budget,
        Reason::Auto => budget.max(summary_room(window)),
    }
    .saturating_sub(overhead);
    let mut as_text = reason == Reason::Overflow;
    let mut rejections = 0;
    let mut rejected: Option<u64> = None;
    let mut requests = 0;

    loop {
        let unchanged = if as_text {
            None
        } else if estimate_request(&full, tools) <= fit {
            Some((full.clone(), TranscriptMode::Unchanged))
        } else if estimate_request(&history, tools) <= target {
            Some((history.clone(), TranscriptMode::History))
        } else {
            None
        };
        let chosen = unchanged
            .or_else(|| {
                flatten_and_drop_oldest(&lead, &recent, &history, body, target)
                    .map(|m| (m, TranscriptMode::Text))
            })
            .or_else(|| {
                if rejections > 0 {
                    None
                } else if as_text {
                    flatten_and_drop_oldest(&lead, &recent, &history, body, u64::MAX)
                        .map(|m| (m, TranscriptMode::Text))
                } else {
                    Some((history.clone(), TranscriptMode::History))
                }
            });
        let Some((request_messages, mode)) = chosen else {
            return Err(CompactionError::Failed(
                "the compaction request cannot be reduced further without losing the latest exchange or checkpoint".into(),
            ));
        };
        // Text transcripts carry no tool definitions (they no longer share a
        // cached prefix and the summary does not need them).
        let request_tools: &[Value] = if mode == TranscriptMode::Text {
            &[]
        } else {
            tools
        };
        let size = estimate_request(&request_messages, request_tools);
        match send_with_nudge(
            client,
            model,
            request_messages,
            request_tools,
            &prompt,
            &mut requests,
        ) {
            Sent::Ok(summary) => {
                let recent_text = messages[keep_index..]
                    .iter()
                    .map(message_to_text)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let mut compacted = system;
                compacted.push(ChatMessage::user(checkpoint_text(&summary, &recent_text)));
                let tokens_after = estimate_request(&compacted, tools);
                return Ok(Compacted {
                    messages: compacted,
                    summary,
                    tokens_before,
                    tokens_after,
                    kept_messages: messages.len() - keep_index,
                    mode,
                    requests,
                });
            }
            Sent::Err(error) => return Err(error),
            Sent::TooLong => {
                // An unchanged request the provider rejects goes out again as
                // text before the shrink ladder starts.
                if mode != TranscriptMode::Text && !as_text {
                    as_text = true;
                    continue;
                }
                as_text = true;
                let Some(step) = SHRINK_STEPS.get(rejections) else {
                    return Err(CompactionError::Failed(
                        "the provider kept rejecting the summary request as too long".into(),
                    ));
                };
                let base = *rejected.get_or_insert(size);
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                {
                    target = (base as f64 * step).floor() as u64;
                }
                fit = target;
                rejections += 1;
            }
        }
    }
}

fn send_with_nudge(
    client: &dyn CompletionClient,
    model: &str,
    mut messages: Vec<ChatMessage>,
    tools: &[Value],
    prompt: &str,
    requests: &mut usize,
) -> Sent {
    push_user(&mut messages, prompt.to_string());
    for attempt in 0..2 {
        *requests += 1;
        let request = ChatRequest {
            model: model.to_string(),
            messages: messages.clone(),
            tools: tools.to_vec(),
        };
        let reply = match client.complete(&request, &mut |_| {}) {
            Ok(reply) => reply,
            Err(error) if is_context_overflow(&error) => return Sent::TooLong,
            Err(error) => return Sent::Err(CompactionError::Llm(error.to_string())),
        };
        if reply.finish_reason.as_deref() == Some("length") {
            return Sent::Err(CompactionError::Failed(
                "the summary reached the output token limit".into(),
            ));
        }
        if filled(&reply.content) {
            return Sent::Ok(reply.content.trim().to_string());
        }
        if attempt == 0 {
            let shown = if reply.content.trim().is_empty() {
                "(no summary)".to_string()
            } else {
                reply.content.clone()
            };
            messages.push(ChatMessage::assistant(Some(shown), Vec::new()));
            messages.push(ChatMessage::user(NUDGE.to_string()));
        } else {
            let message = if reply.content.trim().is_empty() {
                "compaction produced no summary"
            } else {
                "compaction summary did not match the required template"
            };
            return Sent::Err(CompactionError::Failed(message.into()));
        }
    }
    Sent::Err(CompactionError::Failed("unreachable".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{Completion, FunctionCall, ToolCall};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    const SUMMARY: &str = "## Objective\n- fix the build\n\n## Next Move\n1. run tests";

    struct Mock {
        replies: Mutex<VecDeque<Result<Completion, LlmError>>>,
        seen: Mutex<Vec<ChatRequest>>,
    }

    impl Mock {
        fn new(replies: Vec<Result<Completion, LlmError>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                seen: Mutex::new(Vec::new()),
            }
        }
        fn seen(&self) -> Vec<ChatRequest> {
            self.seen.lock().expect("lock").clone()
        }
    }

    impl CompletionClient for Mock {
        fn complete(
            &self,
            request: &ChatRequest,
            _on_delta: &mut dyn FnMut(&str),
        ) -> Result<Completion, LlmError> {
            self.seen.lock().expect("lock").push(request.clone());
            self.replies
                .lock()
                .expect("lock")
                .pop_front()
                .unwrap_or_else(|| Ok(text(SUMMARY)))
        }
    }

    fn text(content: &str) -> Completion {
        Completion {
            content: content.into(),
            ..Completion::default()
        }
    }

    fn too_long() -> LlmError {
        LlmError::Http {
            status: 400,
            body: "This model's maximum context length is 8192 tokens".into(),
        }
    }

    fn call(id: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: args.into(),
            },
        }
    }

    /// system, user prompt, then `rounds` × (assistant call, tool result of
    /// `size` chars).
    fn conversation(rounds: usize, size: usize) -> Vec<ChatMessage> {
        let mut messages = vec![
            ChatMessage::system("You are Next Agent."),
            ChatMessage::user("fix the build"),
        ];
        for round in 0..rounds {
            let id = format!("c{round}");
            messages.push(ChatMessage::assistant(
                Some(format!("step {round}")),
                vec![call(&id, &format!("{{\"path\":\"f{round}.rs\"}}"))],
            ));
            messages.push(ChatMessage::tool(id, "x".repeat(size)));
        }
        messages
    }

    #[test]
    fn ceiling_reserves_share_or_16k() {
        assert_eq!(ceiling(200_000), 170_000);
        assert_eq!(ceiling(100_000), 84_000);
        assert_eq!(ceiling(32_000), 16_000);
        assert_eq!(ceiling(20_000), 17_000);
        assert_eq!(ceiling(0), u64::MAX);
        assert_eq!(summary_room(100_000), 84_000);
    }

    #[test]
    fn estimate_uses_last_measured_usage_plus_new_text() {
        let messages = conversation(2, 400);
        let usage = TokenUsage {
            input: 1_000,
            cache_read: 500,
            cache_write: 0,
            output: 50,
            reasoning: 10,
        };
        // Anchor on the second assistant message (index 4): only its tool
        // result (400 chars = 100 tokens) is new.
        let estimate = estimate_prompt(&messages, &[], Some((4, usage)));
        assert_eq!(estimate.measured, 1_560);
        assert_eq!(estimate.estimated, 100);
        let unmeasured = estimate_prompt(&messages, &[], None);
        assert_eq!(unmeasured.measured, 0);
        assert_eq!(unmeasured.estimated, estimate_request(&messages, &[]));
        assert!(due(
            Estimate {
                measured: 84_000,
                estimated: 0
            },
            100_000
        ));
        assert!(!due(
            Estimate {
                measured: 83_999,
                estimated: 0
            },
            100_000
        ));
    }

    #[test]
    fn overflow_errors_are_recognised() {
        assert!(is_context_overflow(&too_long()));
        assert!(is_context_overflow(&LlmError::Http {
            status: 413,
            body: String::new()
        }));
        assert!(!is_context_overflow(&LlmError::Http {
            status: 400,
            body: "bad json".into()
        }));
        assert!(!is_context_overflow(&LlmError::Unauthorized));
        for unrelated in [
            "tool name too long",
            "max_tokens exceeds the maximum of 4096",
        ] {
            assert!(!is_context_overflow(&LlmError::Http {
                status: 400,
                body: unrelated.into()
            }));
        }
    }

    #[test]
    fn tail_starts_at_an_exchange_and_keeps_pairs() {
        // Each round ≈ 1,005 tokens; keep 2,500 → the last two rounds stay.
        let messages = conversation(6, 4_000);
        let keep = split_conversation(&messages, 2_500).expect("split");
        assert_eq!(messages[keep].role, "assistant");
        assert_eq!(messages[keep - 1].role, "tool");
        assert_eq!(keep, messages.len() - 4);
        // Everything fits: keep only the latest exchange.
        let small = conversation(3, 10);
        let keep = split_conversation(&small, 1_000_000).expect("split");
        assert_eq!(keep, small.len() - 2);
        // Nothing but the system prompt: nothing to compact.
        assert_eq!(split_conversation(&small[..1], 10), None);
    }

    #[test]
    fn transcript_shrinks_oldest_first_and_never_drops_newest() {
        let messages = conversation(8, 12_000);
        let lead = vec![messages[0].clone()];
        let whole = flatten_and_drop_oldest(&lead, "", &messages, 1, u64::MAX).expect("whole");
        let whole_text = whole[1].content.clone().expect("text");
        assert!(!whole_text.contains("omitted"));
        // Medium target: tool outputs go before anything is omitted.
        let medium = flatten_and_drop_oldest(&lead, "", &messages, 1, 2_000).expect("medium");
        let text = medium[1].content.clone().expect("text");
        assert!(text.contains("[Tool result omitted]"), "{text}");
        assert!(!text.contains("older exchange"), "{text}");
        assert!(estimate_request(&medium, &[]) <= 2_000);
        // Tight target: whole exchanges are omitted, oldest first.
        let tight = flatten_and_drop_oldest(&lead, "", &messages, 1, 100).expect("tight");
        let text = tight[1].content.clone().expect("text");
        assert!(text.contains("older exchanges omitted"), "{text}");
        assert!(text.contains("step 7"), "newest exchange kept: {text}");
        assert!(text.contains("[User]: fix the build"), "{text}");
        // Impossible target.
        assert!(flatten_and_drop_oldest(&lead, "", &messages, 1, 5).is_none());
    }

    #[test]
    fn transcript_levels_render_as_specified() {
        let tool = ChatMessage::tool("c1", "y".repeat(5_000));
        assert!(flatten_message(&tool, WHOLE).len() > 5_000);
        let clipped = flatten_message(&tool, OUTPUTS_CLIPPED);
        assert!(clipped.contains("3000 characters omitted"), "{clipped}");
        assert_eq!(
            flatten_message(&tool, OUTPUTS_REMOVED),
            "[Tool result omitted]"
        );
        assert_eq!(flatten_message(&tool, USER_ONLY), "");
        let user = ChatMessage::user("hello");
        assert_eq!(flatten_message(&user, USER_ONLY), "[User]: hello");
        assert_eq!(flatten_message(&user, OMITTED), "");
        let error = ChatMessage::tool("c2", "error: boom");
        assert!(flatten_message(&error, WHOLE).starts_with("[Tool error]"));
    }

    #[test]
    fn compaction_sends_whole_conversation_then_builds_checkpoint() {
        let messages = conversation(6, 4_000);
        let client = Mock::new(vec![Ok(text(SUMMARY))]);
        let config = CompactionConfig {
            window: 100_000,
            keep: 2_500,
        };
        let out = compact(&client, "m", &messages, &[], &config, Reason::Auto).expect("compact");
        assert_eq!(out.mode, TranscriptMode::Unchanged);
        assert_eq!(out.requests, 1);
        assert_eq!(out.kept_messages, 4);
        // System prompt carried byte for byte; one checkpoint user message.
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0], messages[0]);
        let checkpoint = out.messages[1].content.clone().expect("checkpoint");
        assert!(checkpoint.starts_with("<conversation-checkpoint>\n<summary>\n## Objective"));
        assert!(checkpoint.contains("<recent-context>\n[Assistant]: step 4"));
        assert!(checkpoint.contains("</conversation-checkpoint>"));
        assert!(out.tokens_after < out.tokens_before);
        // The summariser saw the conversation plus the prompt, roles alternating.
        let seen = client.seen();
        let request = &seen[0].messages;
        assert_eq!(request.len(), messages.len() + 1);
        assert!(request
            .last()
            .and_then(|m| m.content.as_deref())
            .is_some_and(|c| c.contains("You MUST summarize")));
        // A second compaction is phrased as an update and parses the first.
        let previous = previous_checkpoint(&out.messages).expect("checkpoint");
        assert!(previous.summary.starts_with("## Objective"));
        assert!(previous.recent.contains("step 5"));
    }

    #[test]
    fn missing_headings_get_one_nudge_then_fail() {
        let messages = conversation(4, 4_000);
        let config = CompactionConfig {
            window: 100_000,
            keep: 1_500,
        };
        let nudged = Mock::new(vec![Ok(text("sure, here is a summary")), Ok(text(SUMMARY))]);
        let out = compact(&nudged, "m", &messages, &[], &config, Reason::Auto).expect("nudged");
        assert_eq!(out.requests, 2);
        let seen = nudged.seen();
        let second = &seen[1].messages;
        assert_eq!(second[second.len() - 2].role, "assistant");
        assert_eq!(
            second.last().and_then(|m| m.content.as_deref()),
            Some(NUDGE)
        );

        let stubborn = Mock::new(vec![Ok(text("no")), Ok(text("still no"))]);
        let err = compact(&stubborn, "m", &messages, &[], &config, Reason::Auto).expect_err("fail");
        assert!(matches!(err, CompactionError::Failed(m) if m.contains("template")));
    }

    #[test]
    fn rejected_requests_shrink_down_the_ladder() {
        let messages = conversation(10, 8_000);
        let config = CompactionConfig {
            window: 100_000,
            keep: 2_500,
        };
        let client = Mock::new(vec![
            Err(too_long()),
            Err(too_long()),
            Err(too_long()),
            Ok(text(SUMMARY)),
        ]);
        let out = compact(&client, "m", &messages, &[], &config, Reason::Auto).expect("compact");
        assert_eq!(out.mode, TranscriptMode::Text);
        let sizes: Vec<u64> = client
            .seen()
            .iter()
            .map(|r| estimate_request(&r.messages, &r.tools))
            .collect();
        assert_eq!(sizes.len(), 4);
        assert!(sizes[2] < sizes[1], "{sizes:?}");
        assert!(sizes[3] < sizes[2], "{sizes:?}");

        let hopeless = Mock::new((0..8).map(|_| Err(too_long())).collect());
        let err =
            compact(&hopeless, "m", &messages, &[], &config, Reason::Overflow).expect_err("fail");
        assert!(matches!(err, CompactionError::Failed(_)));
        assert!(hopeless.seen().len() <= 6);
    }

    #[test]
    fn overflow_compaction_goes_straight_to_text() {
        let messages = conversation(4, 4_000);
        let client = Mock::new(vec![Ok(text(SUMMARY))]);
        let config = CompactionConfig {
            window: 100_000,
            keep: 1_500,
        };
        let out = compact(&client, "m", &messages, &[], &config, Reason::Overflow).expect("ok");
        assert_eq!(out.mode, TranscriptMode::Text);
        let request = &client.seen()[0];
        assert_eq!(request.tools, Vec::<Value>::new());
        // System + one merged user message (transcript + prompt).
        assert_eq!(request.messages.len(), 2);
    }

    #[test]
    fn recompaction_keeps_previous_summary_in_front() {
        let mut messages = vec![
            ChatMessage::system("You are Next Agent."),
            ChatMessage::user(checkpoint_text(
                "## Objective\n- old goal",
                "[User]: earlier",
            )),
        ];
        messages.extend(conversation(5, 4_000).into_iter().skip(2));
        let client = Mock::new(vec![Ok(text(SUMMARY))]);
        let config = CompactionConfig {
            window: 100_000,
            keep: 1_500,
        };
        let out = compact(&client, "m", &messages, &[], &config, Reason::Overflow).expect("ok");
        let request = &client.seen()[0].messages;
        let body = request[1].content.clone().expect("body");
        assert!(body.contains("old goal"));
        assert!(body.contains("[User]: earlier"));
        assert!(body.contains("Update the existing checkpoint"));
        assert_eq!(out.messages.len(), 2);
    }

    #[test]
    fn an_oversized_checkpoint_can_be_compacted_again() {
        let messages = vec![
            ChatMessage::system("s"),
            ChatMessage::user(checkpoint_text(
                "## Objective\n- x",
                &"[Tool result]: y".repeat(500),
            )),
        ];
        assert_eq!(split_conversation(&messages, 1_000), Some(2));
        let client = Mock::new(vec![Ok(text(SUMMARY))]);
        let config = CompactionConfig {
            window: 100_000,
            keep: 1_000,
        };
        let out = compact(&client, "m", &messages, &[], &config, Reason::Overflow).expect("ok");
        assert!(out.tokens_after < out.tokens_before);
        assert!(previous_checkpoint(&out.messages).is_some_and(|c| c.recent.is_empty()));
        // A checkpoint without kept text and nothing after it: nothing to do.
        assert_eq!(split_conversation(&out.messages, 1_000), None);
    }

    #[test]
    fn push_user_merges_consecutive_user_messages() {
        let mut messages = vec![ChatMessage::system("s"), ChatMessage::user("a")];
        push_user(&mut messages, "b".into());
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content.as_deref(), Some("a\n\nb"));
        messages.push(ChatMessage::assistant(Some("x".into()), Vec::new()));
        push_user(&mut messages, "c".into());
        assert_eq!(messages.len(), 4);
    }

    #[test]
    fn template_has_the_seven_headings() {
        assert_eq!(
            headings(),
            [
                "## Objective",
                "## Requirements",
                "## Decisions",
                "## Work State",
                "## Next Move",
                "## Relevant Files",
                "## Important Context"
            ]
        );
        assert!(filled("## Objective\n- x"));
        assert!(!filled("Objective: x"));
        assert!(SUMMARY_TEMPLATE.contains("at most 15"));
    }
}
