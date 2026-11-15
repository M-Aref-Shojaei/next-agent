//! `BuiltinExecutor`: the built-in OpenAI-compatible tool-calling loop
//! (ADR-0006). Talks to the LLM only through [`CompletionClient`] and performs
//! side effects only by returning [`AgentStep::Call`] to the runner/broker.
//!
//! Loop: send messages + tool schemas → stream `message_delta` → for each
//! tool call yield a `Call` → append the result as a `tool` message → repeat
//! until the model stops calling tools or the step budget is spent.
//!
//! On top of that loop (ports credited in each module):
//! - **Agent profiles** ([`crate::profiles`]): the task runs as `build` (or
//!   `plan` in plan mode) or a chosen profile. Denied tools are not offered;
//!   every call carries the profile chain's [`Floor`] to the broker, which can
//!   only tighten the permission engine's decision. A `steps` cap ends with
//!   one forced no-tools summary turn.
//! - **Context compaction** ([`crate::compaction`], opencode): before a turn
//!   whose estimated prompt reaches the ceiling, and after a provider "too
//!   long" rejection, older history becomes a checkpoint summary.
//! - **Subagents** ([`crate::subagent`], opencode + Hermes): the `subagent`
//!   tool runs child executors whose calls flow out through this executor's
//!   steps, so they hit the same broker and permission engine.
//! - **Prompt-cache invariants** (Hermes `agent/AGENTS.md`): the system
//!   prompt is fixed for the task (checked by hash before every request), new
//!   content enters only as user or tool messages, no two user or assistant
//!   messages are adjacent, and only compaction rewrites history. Every
//!   request is checked with [`validate_transcript`] before it is sent.

use crate::agent::{AgentExecutor, AgentStep, TaskSpec};
use crate::compaction::{self, CompactionConfig, CompactionError, Reason};
use crate::events::{Emitter, EventKind};
use crate::learning::sessions::Transcript;
use crate::learning::{self, Learning};
use crate::llm::{ChatMessage, ChatRequest, Completion, CompletionClient, TokenUsage, ToolCall};
use crate::profiles::{self, AgentProfile};
use crate::runner::Floor;
use crate::subagent::{self, Report, SubagentLimits, SubagentTask};
use crate::tools::{ToolRequest, ToolResult};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default number of model turns per task (Agent Execution: 50).
pub const DEFAULT_STEP_BUDGET: usize = 50;

/// Longest tool output fed back to the model.
pub const TOOL_RESULT_LIMIT: usize = 16 * 1024;

/// Context window assumed when the caller does not set one. Conservative:
/// a larger real window only means compacting earlier than necessary, and a
/// smaller one is still caught by the provider's "too long" rejection.
pub const DEFAULT_CONTEXT_WINDOW: u64 = 128_000;

/// Consecutive failed automatic compactions after which only a provider
/// overflow triggers compaction (upstream stops background summaries after 2).
pub const MAX_AUTO_COMPACTION_FAILURES: usize = 2;

/// Resolves a profile's `model_role` to a client and model id (wired by the
/// LLM gateway). `None` keeps the task's own client and model.
pub type RoleResolver =
    Arc<dyn Fn(&str) -> Option<(Arc<dyn CompletionClient>, String)> + Send + Sync>;

enum Queued {
    Emit(EventKind),
    Call {
        call_id: String,
        request: ToolRequest,
    },
    /// The model asked for something unparseable or not allowed; answer it
    /// without a call.
    Invalid {
        call_id: String,
        error: String,
    },
    Subagent {
        call_id: String,
        tasks: Vec<SubagentTask>,
    },
}

/// How the executor's loop ended (reported to a parent as `exit_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Running,
    Done,
    MaxSteps,
    Error,
}

impl Exit {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running | Self::Done => "done",
            Self::MaxSteps => "max_steps",
            Self::Error => "error",
        }
    }
}

enum ChildState {
    /// Not started yet: the first turn runs when the child is first driven,
    /// after its `subagent_started` event and with cancellation checked.
    Starting(TaskSpec),
    Ready(AgentStep),
    Waiting {
        outer: String,
        inner: String,
    },
    /// `None` only while a step is being taken.
    Finished(Option<Report>),
}

struct Child {
    id: String,
    agent: String,
    schema: Option<Value>,
    exec: Box<BuiltinExecutor>,
    state: ChildState,
}

struct SubagentRun {
    call_id: String,
    children: Vec<Child>,
    cursor: usize,
    pending: VecDeque<EventKind>,
}

/// Built-in agent loop over an OpenAI-compatible router.
pub struct BuiltinExecutor {
    client: Arc<dyn CompletionClient>,
    root: PathBuf,
    budget: usize,
    model: String,
    plan_mode: bool,
    messages: Vec<ChatMessage>,
    queue: VecDeque<Queued>,
    turns: usize,
    finished: bool,
    last_text: String,
    events: Option<Emitter>,
    // Profiles.
    profiles: Vec<AgentProfile>,
    profile: Option<AgentProfile>,
    ancestors: Vec<AgentProfile>,
    chain: Vec<AgentProfile>,
    role_resolver: Option<RoleResolver>,
    // Prompt-cache invariants.
    tools: Vec<Value>,
    system_prompt: String,
    // Compaction.
    compaction: CompactionConfig,
    summary_client: Option<(Arc<dyn CompletionClient>, Option<String>)>,
    anchor: Option<(usize, TokenUsage)>,
    compactions: usize,
    auto_failures: usize,
    // Steps cap.
    exit: Exit,
    // Subagents.
    depth: usize,
    limits: SubagentLimits,
    spec: Option<TaskSpec>,
    sub_counter: usize,
    id_prefix: String,
    active: Option<SubagentRun>,
    // Memory, skills and session search (`crate::learning`).
    learning: Option<Learning>,
    /// The frozen memory snapshot and skills index of this task, put in
    /// front of the first user message (and of every checkpoint).
    preamble: Option<String>,
    transcript: Option<Box<dyn Transcript>>,
    recorded: usize,
}

impl std::fmt::Debug for BuiltinExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltinExecutor")
            .field("root", &self.root)
            .field("model", &self.model)
            .field("profile", &self.chain.last().map(|p| p.name.as_str()))
            .field("depth", &self.depth)
            .field("turns", &self.turns)
            .field("messages", &self.messages.len())
            .finish_non_exhaustive()
    }
}

impl BuiltinExecutor {
    /// `workspace_root` resolves relative paths the model uses (the sandbox
    /// root, i.e. the worktree path in `auto` mode).
    #[must_use]
    pub fn new(client: Arc<dyn CompletionClient>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            client,
            root: workspace_root.into(),
            budget: DEFAULT_STEP_BUDGET,
            model: String::new(),
            plan_mode: false,
            messages: Vec::new(),
            queue: VecDeque::new(),
            turns: 0,
            finished: false,
            last_text: String::new(),
            events: None,
            profiles: profiles::builtin_profiles(),
            profile: None,
            ancestors: Vec::new(),
            chain: Vec::new(),
            role_resolver: None,
            tools: Vec::new(),
            system_prompt: String::new(),
            compaction: CompactionConfig {
                window: DEFAULT_CONTEXT_WINDOW,
                ..CompactionConfig::default()
            },
            summary_client: None,
            anchor: None,
            compactions: 0,
            auto_failures: 0,
            exit: Exit::Running,
            depth: 0,
            limits: SubagentLimits::default(),
            spec: None,
            sub_counter: 0,
            id_prefix: String::new(),
            active: None,
            learning: None,
            preamble: None,
            transcript: None,
            recorded: 0,
        }
    }

    /// Offer the memory, skills and session-search tools. The memory
    /// snapshot and skills index are read once, when the task starts.
    /// Subagents never inherit these tools.
    #[must_use]
    pub fn with_learning(mut self, learning: Learning) -> Self {
        self.learning = Some(learning);
        self
    }

    /// Record the conversation into the session store as it grows.
    #[must_use]
    pub fn with_transcript(mut self, transcript: Box<dyn Transcript>) -> Self {
        self.transcript = Some(transcript);
        self
    }

    /// The frozen context this task started with (tests).
    #[must_use]
    pub fn preamble(&self) -> Option<&str> {
        self.preamble.as_deref()
    }

    /// Hand new messages to the transcript (never the system prompt).
    fn flush_transcript(&mut self) {
        let start = self.recorded.max(1);
        if let Some(transcript) = self.transcript.as_mut() {
            if start < self.messages.len() {
                transcript.record(&self.messages[start..]);
            }
        }
        self.recorded = self.messages.len();
    }

    /// Put the frozen preamble in front of `text`.
    fn with_preamble(&self, text: &str) -> String {
        match &self.preamble {
            Some(preamble) => format!("{preamble}\n\n{text}"),
            None => text.to_string(),
        }
    }

    #[must_use]
    pub fn with_step_budget(mut self, budget: usize) -> Self {
        self.budget = budget.max(1);
        self
    }

    /// Run as this profile instead of `build` / `plan`.
    #[must_use]
    pub fn with_profile(mut self, profile: AgentProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    /// Profiles available to the `subagent` tool (default: the built-ins).
    /// Use [`profiles::load_profiles`] to include the workspace's files.
    #[must_use]
    pub fn with_profiles(mut self, profiles: Vec<AgentProfile>) -> Self {
        self.profiles = profiles;
        self
    }

    /// The model's context window (0 = unknown: compact only on overflow).
    #[must_use]
    pub fn with_context_window(mut self, window: u64) -> Self {
        self.compaction.window = window;
        self
    }

    #[must_use]
    pub fn with_compaction(mut self, config: CompactionConfig) -> Self {
        self.compaction = config;
        self
    }

    /// Client (and model; `None` = the task's) that writes compaction
    /// summaries — the gateway's `summary` role. Default: the task's client.
    #[must_use]
    pub fn with_summary_client(
        mut self,
        client: Arc<dyn CompletionClient>,
        model: Option<String>,
    ) -> Self {
        self.summary_client = Some((client, model));
        self
    }

    /// Resolves profile `model_role`s to a client and model.
    #[must_use]
    pub fn with_role_resolver(mut self, resolver: RoleResolver) -> Self {
        self.role_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub fn with_subagent_limits(mut self, limits: SubagentLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Conversation so far (for tests and task records).
    #[must_use]
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Tool schemas offered to the model for this task.
    #[must_use]
    pub fn offered_tools(&self) -> &[Value] {
        &self.tools
    }

    /// The profile this executor runs as (after `start`).
    #[must_use]
    pub fn profile_name(&self) -> Option<&str> {
        self.chain.last().map(|p| p.name.as_str())
    }

    /// How many compactions ran in this task.
    #[must_use]
    pub fn compactions(&self) -> usize {
        self.compactions
    }

    fn own_profile(&self) -> &str {
        self.profile_name().unwrap_or("build")
    }

    // -- requests ----------------------------------------------------------

    /// Send the conversation (after the cache-invariant check), compacting
    /// and retrying once if the provider says the prompt is too long.
    /// `with_tools: false` sends no tool schemas (the final summary turn).
    fn request(&mut self, with_tools: bool) -> Result<Completion, AgentStep> {
        let mut retried = false;
        self.flush_transcript();
        loop {
            if let Err(error) = self.check_invariants() {
                self.exit = Exit::Error;
                return Err(AgentStep::Fail {
                    error: format!("internal error: prompt-cache invariant violated: {error}"),
                });
            }
            let request = ChatRequest {
                model: self.model.clone(),
                messages: self.messages.clone(),
                tools: if with_tools {
                    self.tools.clone()
                } else {
                    Vec::new()
                },
            };
            let events = self.events.clone();
            let mut on_delta = |text: &str| {
                if let Some(events) = &events {
                    events.emit(EventKind::MessageDelta {
                        text: text.to_string(),
                    });
                }
            };
            match self.client.complete(&request, &mut on_delta) {
                Ok(completion) => return Ok(completion),
                Err(error) if compaction::is_context_overflow(&error) && !retried => {
                    retried = true;
                    if let Err(message) = self.compact(Reason::Overflow) {
                        self.exit = Exit::Error;
                        return Err(AgentStep::Fail {
                            error: format!("{error}; {message}"),
                        });
                    }
                }
                Err(error) => {
                    self.exit = Exit::Error;
                    return Err(AgentStep::Fail {
                        error: error.to_string(),
                    });
                }
            }
        }
    }

    fn check_invariants(&self) -> Result<(), String> {
        let system = self
            .messages
            .first()
            .and_then(|m| m.content.as_deref())
            .unwrap_or_default();
        if system != self.system_prompt {
            return Err("the system prompt changed during the task".into());
        }
        validate_transcript(&self.messages)
    }

    /// Replace older history with a checkpoint. Queues a `context_compacted`
    /// event either way (except when there is nothing to compact).
    fn compact(&mut self, reason: Reason) -> Result<(), String> {
        let (client, model) = match &self.summary_client {
            Some((client, model)) => (
                Arc::clone(client),
                model.clone().unwrap_or_else(|| self.model.clone()),
            ),
            None => (Arc::clone(&self.client), self.model.clone()),
        };
        let outcome = compaction::compact(
            client.as_ref(),
            &model,
            &self.messages,
            &self.tools,
            &self.compaction,
            reason,
        );
        match outcome {
            Ok(done) => {
                self.queue
                    .push_back(Queued::Emit(EventKind::ContextCompacted {
                        reason: reason.as_str().into(),
                        ok: true,
                        tokens_before: done.tokens_before,
                        tokens_after: done.tokens_after,
                        kept_messages: done.kept_messages,
                        summary_chars: done.summary.chars().count(),
                        error: None,
                    }));
                self.flush_transcript();
                self.messages = done.messages;
                // Compaction is the one place history changes; the frozen
                // preamble rides on the checkpoint so it is not lost.
                if let Some(preamble) = self.preamble.clone() {
                    if let Some(first) = self.messages.get_mut(1).filter(|m| m.role == "user") {
                        let text = first.content.take().unwrap_or_default();
                        first.content = Some(format!("{preamble}\n\n{text}"));
                    }
                }
                if let Some(transcript) = self.transcript.as_mut() {
                    transcript.compacted();
                }
                self.recorded = 1;
                self.anchor = None;
                self.compactions += 1;
                Ok(())
            }
            Err(CompactionError::NothingToCompact) => Err("nothing to compact".into()),
            Err(error) => {
                self.queue
                    .push_back(Queued::Emit(EventKind::ContextCompacted {
                        reason: reason.as_str().into(),
                        ok: false,
                        tokens_before: compaction::estimate_request(&self.messages, &self.tools),
                        tokens_after: 0,
                        kept_messages: 0,
                        summary_chars: 0,
                        error: Some(error.to_string()),
                    }));
                Err(error.to_string())
            }
        }
    }

    // -- turns ------------------------------------------------------------

    fn turn(&mut self) -> AgentStep {
        if self.turns >= self.budget {
            return self.final_summary_turn();
        }
        self.turns += 1;
        let estimate = compaction::estimate_prompt(&self.messages, &self.tools, self.anchor);
        if self.compaction.window > 0
            && self.auto_failures < MAX_AUTO_COMPACTION_FAILURES
            && compaction::due(estimate, self.compaction.window)
        {
            // Best effort: a failed automatic compaction is reported and the
            // request still goes out (an overflow then retries once). After
            // repeated failures only overflows trigger compaction.
            match self.compact(Reason::Auto) {
                Ok(()) => self.auto_failures = 0,
                Err(_) => self.auto_failures += 1,
            }
        }
        let completion = match self.request(true) {
            Ok(completion) => completion,
            Err(step) => return step,
        };
        self.absorb(&completion);
        self.advance()
    }

    /// The steps cap was reached: one last request without tools, asking for
    /// a summary (injected as a user message, never into the system prompt).
    fn final_summary_turn(&mut self) -> AgentStep {
        if self.exit == Exit::MaxSteps {
            return self.done();
        }
        self.exit = Exit::MaxSteps;
        compaction::push_user(
            &mut self.messages,
            format!(
                "You have reached the maximum of {} model turns for this agent. Do not call tools. Reply with a concise summary of what you did, what is left, and anything blocking.",
                self.budget
            ),
        );
        let completion = match self.request(false) {
            Ok(completion) => completion,
            Err(step) => return step,
        };
        let text = completion.content.trim().to_string();
        // Any tool calls are dropped: none were offered.
        self.messages.push(ChatMessage::assistant(
            Some(if text.is_empty() {
                "(no summary)".to_string()
            } else {
                completion.content.clone()
            }),
            Vec::new(),
        ));
        self.finished = true;
        if text.is_empty() {
            self.last_text = format!(
                "stopped: step budget of {} model turns reached",
                self.budget
            );
        } else {
            self.queue
                .push_back(Queued::Emit(EventKind::Message { text: text.clone() }));
            self.last_text = text;
        }
        self.advance()
    }

    fn absorb(&mut self, completion: &Completion) {
        let text = completion.content.trim().to_string();
        let index = self.messages.len();
        // Some providers repeat ids across parallel calls; make them unique
        // so every result pairs with exactly one call.
        let mut tool_calls = completion.tool_calls.clone();
        let mut seen: HashSet<String> = HashSet::new();
        for (position, call) in tool_calls.iter_mut().enumerate() {
            if call.id.is_empty() || !seen.insert(call.id.clone()) {
                call.id = format!("{}_{position}", call.id);
                seen.insert(call.id.clone());
            }
        }
        if let Some(usage) = completion.usage {
            self.anchor = Some((index, usage));
        }
        self.messages.push(ChatMessage::assistant(
            (!completion.content.is_empty()).then(|| completion.content.clone()),
            tool_calls.clone(),
        ));
        self.finished = tool_calls.is_empty();
        if !text.is_empty() {
            let kind = if self.finished && self.plan_mode {
                EventKind::PlanProposed {
                    markdown: text.clone(),
                }
            } else {
                EventKind::Message { text: text.clone() }
            };
            self.queue.push_back(Queued::Emit(kind));
            self.last_text = text;
        }
        for call in &tool_calls {
            let queued = self.classify(call);
            self.queue.push_back(queued);
        }
    }

    fn classify(&self, call: &ToolCall) -> Queued {
        let invalid = |error: String| Queued::Invalid {
            call_id: call.id.clone(),
            error,
        };
        if call.function.name == subagent::TOOL {
            if !self.offers_subagent() {
                return invalid(format!(
                    "tool `subagent` is not available to the `{}` agent",
                    self.own_profile()
                ));
            }
            return match parse_args(&call.function.arguments)
                .and_then(|args| subagent::parse_tasks(&args, &self.limits))
            {
                Ok(tasks) => Queued::Subagent {
                    call_id: call.id.clone(),
                    tasks,
                },
                Err(error) => invalid(error),
            };
        }
        if let Some(learning) = &self.learning {
            let args = match parse_args(&call.function.arguments) {
                Ok(args) => args,
                Err(error)
                    if learning::tools::TOOL_NAMES.contains(&call.function.name.as_str()) =>
                {
                    return invalid(error)
                }
                Err(_) => Value::Null,
            };
            match learning.request(&call.function.name, &args) {
                Some(Ok(request))
                    if !profiles::chain_offers(&self.chain, request.name(), request.kind()) =>
                {
                    return invalid(format!(
                        "tool `{}` is not available to the `{}` agent",
                        request.name(),
                        self.own_profile()
                    ));
                }
                Some(Ok(request)) => {
                    return Queued::Call {
                        call_id: call.id.clone(),
                        request,
                    }
                }
                Some(Err(error)) => return invalid(error),
                None => {}
            }
        }
        match parse_call(call, &self.root) {
            Ok(request) if !profiles::chain_offers(&self.chain, request.name(), request.kind()) => {
                invalid(format!(
                    "tool `{}` is not available to the `{}` agent",
                    request.name(),
                    self.own_profile()
                ))
            }
            Ok(request) => Queued::Call {
                call_id: call.id.clone(),
                request,
            },
            Err(error) => invalid(error),
        }
    }

    /// Spawning is allowed below the depth limit and only when no profile in
    /// the chain restricts the tool (`ask` fails closed: there is no broker
    /// call to put the prompt on; every child call is still gated).
    fn offers_subagent(&self) -> bool {
        self.depth < self.limits.max_depth
            && self.chain.iter().all(|p| {
                p.permission(subagent::TOOL, subagent::TOOL) == profiles::Permission::Allow
            })
    }

    fn done(&self) -> AgentStep {
        let summary = if self.last_text.is_empty() {
            "done".to_string()
        } else {
            crate::runner::truncate(&self.last_text, 500)
        };
        AgentStep::Done { summary }
    }

    fn advance(&mut self) -> AgentStep {
        loop {
            if self.active.is_some() {
                if let Some(step) = self.drive_subagents() {
                    return step;
                }
                continue;
            }
            let Some(item) = self.queue.pop_front() else {
                break;
            };
            match item {
                Queued::Emit(kind) => return AgentStep::Emit(kind),
                Queued::Call { call_id, request } => return AgentStep::Call { call_id, request },
                Queued::Invalid { call_id, error } => {
                    self.messages
                        .push(ChatMessage::tool(call_id, format!("error: {error}")));
                }
                Queued::Subagent { call_id, tasks } => self.spawn(call_id, &tasks),
            }
        }
        if self.finished {
            if self.exit == Exit::Running {
                self.exit = Exit::Done;
            }
            self.flush_transcript();
            return self.done();
        }
        self.turn()
    }

    // -- subagents ----------------------------------------------------------

    fn spawn(&mut self, call_id: String, tasks: &[SubagentTask]) {
        let mut chosen = Vec::new();
        for task in tasks {
            match profiles::find(&self.profiles, &task.agent) {
                Some(profile) if profile.mode.subagent() => chosen.push(profile.clone()),
                _ => {
                    let available: Vec<&str> = subagent::spawnable(&self.profiles)
                        .map(|p| p.name.as_str())
                        .collect();
                    self.messages.push(ChatMessage::tool(
                        call_id,
                        format!(
                            "error: unknown subagent `{}`; available: {}",
                            task.agent,
                            available.join(", ")
                        ),
                    ));
                    return;
                }
            }
        }
        let Some(parent_spec) = self.spec.clone() else {
            self.messages
                .push(ChatMessage::tool(call_id, "error: task not started"));
            return;
        };
        let mut run = SubagentRun {
            call_id: call_id.clone(),
            children: Vec::new(),
            cursor: 0,
            pending: VecDeque::new(),
        };
        for (task, profile) in tasks.iter().zip(chosen) {
            self.sub_counter += 1;
            let id = format!("{}sub{}", self.id_prefix, self.sub_counter);
            let exec = Box::new(self.child_executor(&profile, &id));
            let spec = TaskSpec {
                id: parent_spec.id.clone(),
                prompt: subagent::child_prompt(task),
                mode: parent_spec.mode,
                model: self.model.clone(),
            };
            run.pending.push_back(EventKind::SubagentStarted {
                subagent_id: id.clone(),
                parent_call_id: call_id.clone(),
                profile: profile.name.clone(),
                goal: crate::runner::truncate(&task.goal, 500),
                depth: self.depth + 1,
            });
            run.children.push(Child {
                id,
                agent: profile.name.clone(),
                schema: task.output_schema.clone(),
                exec,
                state: ChildState::Starting(spec),
            });
        }
        self.active = Some(run);
    }

    fn child_executor(&self, profile: &AgentProfile, id: &str) -> BuiltinExecutor {
        let mut child = BuiltinExecutor::new(Arc::clone(&self.client), self.root.clone());
        child.profiles.clone_from(&self.profiles);
        child.profile = Some(profile.clone());
        child.ancestors.clone_from(&self.chain);
        // Tools a child never inherits, as one more profile in its chain so
        // offering, refusing and floors all agree.
        child.ancestors.push(subagent::blocklist_profile());
        child.role_resolver.clone_from(&self.role_resolver);
        child.compaction = self.compaction;
        child.summary_client.clone_from(&self.summary_client);
        child.depth = self.depth + 1;
        child.limits = self.limits;
        child.budget = profile.steps.unwrap_or(self.limits.default_steps).max(1);
        child.id_prefix = format!("{id}.");
        child
    }

    /// Advance the running subagents by one step (round-robin). `None` once
    /// every child has finished and the reports were handed to the model.
    fn drive_subagents(&mut self) -> Option<AgentStep> {
        let limits = self.limits;
        let run = self.active.as_mut()?;
        if let Some(kind) = run.pending.pop_front() {
            return Some(AgentStep::Emit(kind));
        }
        let count = run.children.len();
        for offset in 0..count {
            let index = (run.cursor + offset) % count;
            let child = &mut run.children[index];
            let step = match std::mem::replace(&mut child.state, ChildState::Finished(None)) {
                ChildState::Ready(step) => step,
                ChildState::Starting(spec) => child.exec.start(&spec),
                other => {
                    child.state = other;
                    continue;
                }
            };
            run.cursor = (index + 1) % count;
            match step {
                AgentStep::Emit(kind) => {
                    child.state = ChildState::Ready(child.exec.next(None));
                    return Some(AgentStep::Emit(relabel(&child.id, kind)));
                }
                AgentStep::Call { call_id, request } => {
                    let outer = format!("{}.{call_id}", child.id);
                    child.state = ChildState::Waiting {
                        outer: outer.clone(),
                        inner: call_id,
                    };
                    return Some(AgentStep::Call {
                        call_id: outer,
                        request,
                    });
                }
                AgentStep::Done { .. } | AgentStep::Fail { .. } => {
                    let (exit, text) = match step {
                        AgentStep::Fail { error } => ("error", error),
                        _ => (child.exec.exit.as_str(), child.exec.last_text.clone()),
                    };
                    let report = Report::new(
                        &child.id,
                        &child.agent,
                        exit,
                        &text,
                        child.schema.as_ref(),
                        &limits,
                        child.exec.turns,
                    );
                    let event = EventKind::SubagentFinished {
                        subagent_id: child.id.clone(),
                        status: report.status.into(),
                        exit_reason: report.exit_reason.into(),
                        truncated: report.truncated,
                        schema_valid: report.schema_valid,
                        steps: report.steps,
                    };
                    child.state = ChildState::Finished(Some(report));
                    return Some(AgentStep::Emit(event));
                }
            }
        }
        if run
            .children
            .iter()
            .any(|child| matches!(child.state, ChildState::Waiting { .. }))
        {
            // Only reachable if the runner skipped a result; fail safe.
            self.exit = Exit::Error;
            self.active = None;
            return Some(AgentStep::Fail {
                error: "internal error: a subagent is waiting for a tool result".into(),
            });
        }
        let run = self.active.take()?;
        let reports: Vec<Report> = run
            .children
            .into_iter()
            .filter_map(|child| match child.state {
                ChildState::Finished(report) => report,
                _ => None,
            })
            .collect();
        self.messages.push(ChatMessage::tool(
            run.call_id,
            subagent::result_json(&reports),
        ));
        None
    }

    fn child_waiting(&self, call_id: &str) -> Option<(&Child, &str)> {
        self.active
            .as_ref()?
            .children
            .iter()
            .find_map(|child| match &child.state {
                ChildState::Waiting { outer, inner } if outer == call_id => {
                    Some((child, inner.as_str()))
                }
                _ => None,
            })
    }
}

/// Child events appear in the parent's timeline, labelled with the child.
fn relabel(id: &str, kind: EventKind) -> EventKind {
    match kind {
        EventKind::Message { text } => EventKind::Message {
            text: format!("[{id}] {text}"),
        },
        EventKind::PlanProposed { markdown } => EventKind::Message {
            text: format!("[{id}] {markdown}"),
        },
        other => other,
    }
}

impl AgentExecutor for BuiltinExecutor {
    fn id(&self) -> &'static str {
        "builtin"
    }

    fn attach(&mut self, events: &Emitter) {
        self.events = Some(events.clone());
    }

    fn start(&mut self, spec: &TaskSpec) -> AgentStep {
        let default = if spec.mode == "plan" { "plan" } else { "build" };
        let profile = self
            .profile
            .clone()
            .or_else(|| profiles::find(&self.profiles, default).cloned())
            .or_else(|| profiles::find(&profiles::builtin_profiles(), default).cloned())
            .unwrap_or_else(|| profiles::builtin_profiles().remove(0));
        self.chain = self.ancestors.clone();
        self.chain.push(profile.clone());
        if let Some(steps) = profile.steps {
            self.budget = self.budget.min(steps.max(1));
        }
        self.model.clone_from(&spec.model);
        if let Some(model) = &profile.model {
            self.model.clone_from(model);
        } else if let (Some(role), Some(resolver)) = (&profile.model_role, &self.role_resolver) {
            if let Some((client, model)) = resolver(role) {
                self.client = client;
                self.model = model;
            }
        }
        self.plan_mode = spec.mode == "plan" || profile.name == "plan";
        self.spec = Some(spec.clone());
        self.queue.clear();
        self.active = None;
        self.turns = 0;
        self.finished = false;
        self.exit = Exit::Running;
        self.anchor = None;
        self.last_text.clear();
        self.tools = tool_schemas_for(
            &self.chain,
            self.offers_subagent(),
            &self.profiles,
            &self.limits,
        );
        let mut system = system_prompt(spec.mode, &self.root, &profile, self.depth);
        self.preamble = None;
        if let Some(learning) = &self.learning {
            let offered: Vec<Value> = learning
                .schemas()
                .into_iter()
                .filter(|tool| {
                    let name = tool["function"]["name"].as_str().unwrap_or_default();
                    profiles::chain_offers(&self.chain, name, learning::tools::policy_kind(name))
                })
                .collect();
            if !offered.is_empty() {
                // Static text only: the system prompt stays byte-stable.
                if let Some(guidance) = learning.guidance() {
                    system.push_str("\n\n");
                    system.push_str(&guidance);
                }
                // Frozen now: writes during the task reach the disk, not
                // this task's prompt.
                self.preamble = learning.preamble();
                self.tools.extend(offered);
            }
        }
        self.system_prompt.clone_from(&system);
        let first = self.with_preamble(&spec.prompt);
        self.messages = vec![ChatMessage::system(system), ChatMessage::user(first)];
        self.recorded = 1;
        if let Some(transcript) = self.transcript.as_mut() {
            transcript.start(&self.system_prompt, &self.model);
        }
        self.turn()
    }

    fn floor(&self, call_id: &str, request: &ToolRequest) -> Option<Floor> {
        let own = profiles::floor_for(&self.chain, request);
        match self.child_waiting(call_id) {
            Some((child, inner)) => Floor::strictest(own, child.exec.floor(inner, request)),
            None => own,
        }
    }

    fn next(&mut self, result: Option<&ToolResult>) -> AgentStep {
        if let Some(result) = result {
            if let Some(run) = self.active.as_mut() {
                let target = run.children.iter_mut().find(|child| {
                    matches!(&child.state, ChildState::Waiting { outer, .. } if *outer == result.call_id)
                });
                if let Some(child) = target {
                    if let ChildState::Waiting { inner, .. } =
                        std::mem::replace(&mut child.state, ChildState::Finished(None))
                    {
                        let mut forwarded = result.clone();
                        forwarded.call_id = inner;
                        child.state = ChildState::Ready(child.exec.next(Some(&forwarded)));
                    }
                    return self.advance();
                }
            }
            let mut content = crate::runner::truncate(&result.output, TOOL_RESULT_LIMIT);
            if !result.ok {
                content = format!("error: {content}");
            }
            self.messages
                .push(ChatMessage::tool(result.call_id.clone(), content));
        }
        self.advance()
    }
}

/// Check a request transcript against the prompt-cache invariants:
/// exactly one system message, first; the first message after it is a user
/// message; no two adjacent user or assistant messages; every tool message
/// answers a pending call of the latest assistant message, once; and every
/// call is answered before the next user or assistant message.
///
/// # Errors
/// The first violation found.
pub fn validate_transcript(messages: &[ChatMessage]) -> Result<(), String> {
    let Some(first) = messages.first() else {
        return Err("empty transcript".into());
    };
    if first.role != "system" {
        return Err("the first message must be the system prompt".into());
    }
    if messages.get(1).is_some_and(|m| m.role != "user") {
        return Err("the first message after the system prompt must be a user message".into());
    }
    let mut pending: HashSet<&str> = HashSet::new();
    for (index, message) in messages.iter().enumerate().skip(1) {
        let previous = &messages[index - 1];
        match message.role.as_str() {
            "system" => return Err(format!("message {index}: a second system message")),
            "user" | "assistant" => {
                if previous.role == message.role {
                    return Err(format!(
                        "message {index}: two {} messages in a row",
                        message.role
                    ));
                }
                if !pending.is_empty() {
                    return Err(format!(
                        "message {index}: {} message before every tool call was answered",
                        message.role
                    ));
                }
                if message.role == "assistant" {
                    pending = message.tool_calls.iter().map(|c| c.id.as_str()).collect();
                }
            }
            "tool" => {
                let id = message.tool_call_id.as_deref().unwrap_or_default();
                if !pending.remove(id) {
                    return Err(format!(
                        "message {index}: tool result `{id}` answers no pending call"
                    ));
                }
            }
            other => return Err(format!("message {index}: unknown role `{other}`")),
        }
    }
    if !pending.is_empty() {
        return Err("the last assistant message has unanswered tool calls".into());
    }
    Ok(())
}

fn system_prompt(mode: &str, root: &Path, profile: &AgentProfile, depth: usize) -> String {
    let mode_text = match mode {
        "plan" => "PLAN mode: investigate with read-only tools only (read_file, list_dir, search). Do not write files or run mutating commands. Finish with a Markdown plan.",
        "auto" => "AUTO mode: you may edit files and run development commands; changes happen in an isolated git worktree the user reviews before merging.",
        "bypassPermissions" => "BYPASS mode: permission prompts are disabled; be careful and conservative.",
        _ => "MANUAL mode: mutating actions (writes, patches, commands) need the user's approval and may be denied; if denied, adapt instead of retrying.",
    };
    let mut prompt = format!(
        "You are Next Agent, a coding agent working in the workspace at {root}.\n\
         {mode_text}\n\
         Use the provided tools for every file or system interaction. Paths may be \
         relative to the workspace root. Prefer `edit` for small changes to existing \
         files. Keep answers concise; when done, reply without tool calls and \
         summarize what you did.\n\
         Agent profile: {name}.",
        root = root.display(),
        name = profile.name,
    );
    if !profile.prompt.trim().is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(profile.prompt.trim());
    }
    if depth > 0 {
        prompt.push_str(
            "\n\nYou are a subagent working for another agent. You have only the context in the task message. Your final message (without tool calls) is returned to that agent as your report.",
        );
    }
    prompt
}

/// `OpenAI` function schemas for the full tool set (no `subagent`).
#[must_use]
pub fn tool_schemas() -> Vec<Value> {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }
            }
        })
    };
    let path = json!({"type": "string", "description": "File or directory path (absolute or workspace-relative)"});
    vec![
        tool(
            "read_file",
            "Read a UTF-8 text file.",
            json!({"path": path}),
            &["path"],
        ),
        tool(
            "list_dir",
            "List a directory (directories end with /).",
            json!({"path": path}),
            &[],
        ),
        tool(
            "search",
            "Regex search over files (respects .gitignore). Returns path:line:text.",
            json!({"pattern": {"type": "string"}, "path": path}),
            &["pattern"],
        ),
        tool(
            "write_file",
            "Create or overwrite a file with the given contents.",
            json!({"path": path, "contents": {"type": "string"}}),
            &["path", "contents"],
        ),
        tool(
            "edit",
            "Replace exact text in a file. old_string must match once (add context to disambiguate) unless replace_all is true. Write line breaks as \\n; the file's CRLF/LF endings and BOM are preserved. If there is no exact match, typographic quotes/dashes/spaces and then trailing whitespace are tolerated.",
            json!({
                "path": path,
                "old_string": {"type": "string", "description": "Exact text to replace (no line-number prefixes)"},
                "new_string": {"type": "string", "description": "Replacement text (must differ)"},
                "replace_all": {"type": "boolean", "description": "Replace every occurrence (default false)"}
            }),
            &["path", "old_string", "new_string"],
        ),
        tool(
            "apply_patch",
            "Apply a unified diff (---/+++ headers and @@ hunks) to one file. Hunks whose context moved or differs in whitespace or quotes are located automatically.",
            json!({"path": path, "patch": {"type": "string"}}),
            &["path", "patch"],
        ),
        tool(
            "run_command",
            "Run a shell command; returns combined output and exit status.",
            json!({"command": {"type": "string"}, "cwd": path}),
            &["command"],
        ),
        tool(
            "http_fetch",
            "HTTP GET a URL and return the body.",
            json!({"url": {"type": "string"}}),
            &["url"],
        ),
    ]
}

/// Schemas offered under a profile chain: tools any profile denies are
/// dropped, and `subagent` is added when spawning is allowed.
#[must_use]
pub fn tool_schemas_for(
    chain: &[AgentProfile],
    offer_subagent: bool,
    profiles: &[AgentProfile],
    limits: &SubagentLimits,
) -> Vec<Value> {
    let mut tools: Vec<Value> = tool_schemas()
        .into_iter()
        .filter(|tool| {
            let name = tool["function"]["name"].as_str().unwrap_or_default();
            profiles::chain_offers(chain, name, profiles::policy_kind(name))
        })
        .collect();
    if offer_subagent && subagent::spawnable(profiles).next().is_some() {
        tools.push(subagent::tool_schema(profiles, limits));
    }
    tools
}

/// Turn a model tool call into a broker request, resolving relative paths.
///
/// # Errors
/// Returns a message for unknown tools, malformed JSON or missing arguments.
pub fn parse_call(call: &ToolCall, root: &Path) -> Result<ToolRequest, String> {
    let args = parse_args(&call.function.arguments)?;
    let text = |key: &str| -> Result<String, String> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing string argument `{key}`"))
    };
    let path_or_root = |key: &str| -> PathBuf {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map_or_else(|| root.to_path_buf(), |value| resolve(root, value))
    };
    let path = |key: &str| text(key).map(|value| resolve(root, &value));
    Ok(match call.function.name.as_str() {
        "read_file" => ToolRequest::Read {
            path: path("path")?,
        },
        "list_dir" => ToolRequest::ListDir {
            path: path_or_root("path"),
        },
        "search" => ToolRequest::Search {
            pattern: text("pattern")?,
            path: path_or_root("path"),
        },
        "write_file" => ToolRequest::Write {
            path: path("path")?,
            contents: text("contents")?,
        },
        "edit" => ToolRequest::Edit {
            path: path("path")?,
            old_string: text("old_string")?,
            new_string: text("new_string")?,
            replace_all: args
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        "apply_patch" => ToolRequest::ApplyPatch {
            path: path("path")?,
            patch: text("patch")?,
        },
        "run_command" => ToolRequest::Exec {
            command: text("command")?,
            cwd: path_or_root("cwd"),
        },
        "http_fetch" => ToolRequest::Network { url: text("url")? },
        other => return Err(format!("unknown tool `{other}`")),
    })
}

/// Tool-call arguments as JSON; empty arguments are `{}`.
fn parse_args(raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(raw).map_err(|error| format!("arguments are not JSON: {error}"))
}

fn resolve(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
#[path = "builtin_tests.rs"]
mod builtin_tests;
