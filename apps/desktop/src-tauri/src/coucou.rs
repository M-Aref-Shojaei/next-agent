//! Next Agent's tasks in Coucou's island.
//!
//! Coucou (vendored whole in `apps/desktop/coucou`, ADR-0011) learns what
//! agents do from Claude Code hook events: `coucou-hook` relays each event over
//! a socket, and Coucou's `pipe.rs` turns it into the island's `hook` event.
//! That path stays as it is, so Coucou keeps working with Claude Code.
//!
//! This adapter puts Next Agent's own task events on the same path: each
//! `task-stream` event becomes the hook event Claude Code would have sent,
//! written to an in-process stream that `pipe.rs` reads like a relay
//! connection ([`coucou_lib::hook_input`]):
//!
//! | Next Agent event        | Coucou hook event                    |
//! |-------------------------|--------------------------------------|
//! | `task_started`          | `SessionStart`, `UserPromptSubmit`   |
//! | `tool_requested`        | `PreToolUse`                         |
//! | `approval_requested`    | `PermissionRequest` (waits)          |
//! | `tool_finished`         | `PostToolUse` / `PostToolUseFailure` |
//! | `task_completed`        | `Stop`                               |
//! | `task_failed`           | `StopFailure`                        |
//! | `task_cancelled`        | `Interrupt`                          |
//!
//! A `PermissionRequest` stream stays open: Coucou writes the island's
//! decision back on it (`allow` / `deny`), and the adapter hands it to
//! `approval_respond`, so Next Agent's permission engine still decides. An
//! approval answered in the main window (`approval_resolved`) or a task that
//! ends closes the stream, and Coucou takes its card down.
//!
//! Every event is tagged `coucou_agent: "next-agent"`: the island gives Next
//! Agent a pill of its own (`apps/desktop/coucou/src/island/agents.ts`).

use crate::service::{AppService, Sink};
use crate::wire::StreamMessage;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

/// The agent tag Coucou routes Next Agent's events by.
pub const AGENT: &str = "next-agent";

/// What one Next Agent event becomes on Coucou's side.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// A hook event: written, read, done.
    Hook(Value),
    /// A permission request: the decision comes back for `approval_id`.
    Permission {
        task_id: String,
        approval_id: String,
        payload: Value,
    },
    /// The approval was answered elsewhere: the card goes.
    Settled { approval_id: String },
    /// The task ended: whatever it still waits for goes.
    Ended { task_id: String },
}

/// Per-task memory the translation needs: the tools by call id (an approval
/// and a result only name the call), the prompt and the workspace folder.
#[derive(Debug, Default)]
pub struct Translator {
    tools: HashMap<String, (String, Value)>,
}

/// The context of one task, given when it starts.
#[derive(Debug, Clone, Default)]
pub struct TaskContext {
    pub prompt: String,
    pub cwd: String,
}

fn hook(name: &str, task_id: &str, ctx: &TaskContext, fields: Value) -> Value {
    let mut out = Map::new();
    out.insert("hook_event_name".into(), json!(name));
    out.insert("session_id".into(), json!(task_id));
    out.insert("cwd".into(), json!(ctx.cwd));
    out.insert("coucou_agent".into(), json!(AGENT));
    if let Value::Object(extra) = fields {
        out.extend(extra);
    }
    Value::Object(out)
}

impl Translator {
    /// Translate one `task-stream` envelope (`wire::envelope`): one arm per
    /// row of the table at the top of this file.
    #[allow(clippy::too_many_lines)]
    pub fn translate(&mut self, ctx: &TaskContext, event: &Value) -> Vec<Out> {
        let task_id = event["task_id"].as_str().unwrap_or_default();
        let data = &event["data"];
        let text = |key: &str| data[key].as_str().unwrap_or_default().to_string();
        match event["kind"].as_str().unwrap_or_default() {
            "task_started" => vec![
                Out::Hook(hook(
                    "SessionStart",
                    task_id,
                    ctx,
                    json!({ "source": "startup" }),
                )),
                Out::Hook(hook(
                    "UserPromptSubmit",
                    task_id,
                    ctx,
                    json!({ "prompt": ctx.prompt }),
                )),
            ],
            "tool_requested" => {
                let call = text("call_id");
                let tool = text("tool");
                let args = data["args"].clone();
                self.tools.insert(call, (tool.clone(), args.clone()));
                vec![Out::Hook(hook(
                    "PreToolUse",
                    task_id,
                    ctx,
                    json!({ "tool_name": tool, "tool_input": args }),
                ))]
            }
            "approval_requested" => {
                let (tool, args) = self
                    .tools
                    .get(&text("call_id"))
                    .cloned()
                    .unwrap_or_else(|| ("Tool".into(), json!({ "description": text("summary") })));
                vec![Out::Permission {
                    task_id: task_id.to_string(),
                    approval_id: text("approval_id"),
                    payload: hook(
                        "PermissionRequest",
                        task_id,
                        ctx,
                        json!({ "tool_name": tool, "tool_input": args, "message": text("summary") }),
                    ),
                }]
            }
            "approval_resolved" => vec![Out::Settled {
                approval_id: text("approval_id"),
            }],
            "tool_finished" => {
                let (tool, args) = self
                    .tools
                    .remove(&text("call_id"))
                    .unwrap_or_else(|| ("Tool".into(), json!({})));
                let ok = data["ok"].as_bool().unwrap_or(true);
                let name = if ok {
                    "PostToolUse"
                } else {
                    "PostToolUseFailure"
                };
                vec![Out::Hook(hook(
                    name,
                    task_id,
                    ctx,
                    json!({
                        "tool_name": tool,
                        "tool_input": args,
                        "tool_response": { "ok": ok, "exit_code": data["exit_code"], "output": text("summary") },
                    }),
                ))]
            }
            "task_completed" => vec![
                Out::Hook(hook(
                    "Stop",
                    task_id,
                    ctx,
                    json!({ "last_assistant_message": text("summary") }),
                )),
                Out::Ended {
                    task_id: task_id.to_string(),
                },
            ],
            "task_failed" => vec![
                Out::Hook(hook(
                    "StopFailure",
                    task_id,
                    ctx,
                    json!({ "message": text("error") }),
                )),
                Out::Ended {
                    task_id: task_id.to_string(),
                },
            ],
            "task_cancelled" => vec![
                Out::Hook(hook("Interrupt", task_id, ctx, json!({}))),
                Out::Ended {
                    task_id: task_id.to_string(),
                },
            ],
            _ => Vec::new(),
        }
    }
}

/// One event and the task it belongs to.
struct Msg {
    ctx: Arc<TaskContext>,
    event: Value,
}

/// Feeds Next Agent's task events to Coucou, in order, on one worker.
#[derive(Clone)]
pub struct Adapter {
    tx: mpsc::UnboundedSender<Msg>,
}

impl Adapter {
    /// Start the worker. Decisions go to `service.approval_respond`.
    #[must_use]
    pub fn start(app: AppHandle, service: Arc<AppService>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(run(app, service, rx));
        Self { tx }
    }

    /// Wrap a task's sink so its events also reach Coucou.
    #[must_use]
    pub fn wrap(&self, ctx: TaskContext, direct: Sink) -> Sink {
        let tx = self.tx.clone();
        let ctx = Arc::new(ctx);
        Arc::new(move |message| {
            if let StreamMessage::Event { event } = &message {
                let _ = tx.send(Msg {
                    ctx: Arc::clone(&ctx),
                    event: event.clone(),
                });
            }
            direct(message);
        })
    }
}

/// Write one line on a fresh hook stream.
async fn open(app: &AppHandle, payload: &Value) -> Option<tokio::io::DuplexStream> {
    let mut stream = coucou_lib::hook_input(app);
    let mut line = payload.to_string();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.ok()?;
    stream.flush().await.ok()?;
    Some(stream)
}

async fn run(app: AppHandle, service: Arc<AppService>, mut rx: mpsc::UnboundedReceiver<Msg>) {
    let mut translators: HashMap<String, Translator> = HashMap::new();
    // approval id → (task id, the way to take its card down).
    let mut waiting: HashMap<String, (String, oneshot::Sender<()>)> = HashMap::new();
    while let Some(Msg { ctx, event }) = rx.recv().await {
        let task = event["task_id"].as_str().unwrap_or_default().to_string();
        let outs = translators
            .entry(task.clone())
            .or_default()
            .translate(&ctx, &event);
        for out in outs {
            match out {
                Out::Hook(payload) => {
                    // Read to the end: Coucou has handled it before the next
                    // event goes, so the island sees them in order.
                    if let Some(stream) = open(&app, &payload).await {
                        let mut rest = Vec::new();
                        let _ = tokio::io::AsyncReadExt::read_to_end(
                            &mut BufReader::new(stream),
                            &mut rest,
                        )
                        .await;
                    }
                }
                Out::Permission {
                    task_id,
                    approval_id,
                    payload,
                } => {
                    let Some(stream) = open(&app, &payload).await else {
                        continue;
                    };
                    let (cancel, cancelled) = oneshot::channel();
                    waiting.insert(approval_id.clone(), (task_id.clone(), cancel));
                    tauri::async_runtime::spawn(wait_for_decision(
                        Arc::clone(&service),
                        stream,
                        cancelled,
                        task_id,
                        approval_id,
                    ));
                }
                Out::Settled { approval_id } => {
                    if let Some((_, cancel)) = waiting.remove(&approval_id) {
                        let _ = cancel.send(());
                    }
                }
                Out::Ended { task_id } => {
                    translators.remove(&task_id);
                    waiting.retain(|_, (t, _)| *t != task_id);
                }
            }
        }
    }
}

/// The island's answer for one approval, or nothing when the card went
/// (answered elsewhere, timed out, or the island could not show it).
async fn wait_for_decision(
    service: Arc<AppService>,
    stream: tokio::io::DuplexStream,
    cancelled: oneshot::Receiver<()>,
    task_id: String,
    approval_id: String,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    tokio::select! {
        read = reader.read_line(&mut line) => {
            let word = line.trim();
            if read.is_ok() && matches!(word, "allow" | "deny") {
                if let Err(error) = service.approval_respond(&task_id, &approval_id, word) {
                    eprintln!("[coucou] approval {approval_id}: {error}");
                }
            }
        }
        // Dropping the stream (here, with `reader`) is the relay going away.
        _ = cancelled => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::needless_pass_by_value)] // the tests build `data` inline
    fn envelope(kind: &str, data: Value) -> Value {
        json!({ "v": 1, "task_id": "t1", "seq": 1, "kind": kind, "data": data })
    }

    fn ctx() -> TaskContext {
        TaskContext {
            prompt: "Add a demo note".into(),
            cwd: "/work/demo".into(),
        }
    }

    #[test]
    fn a_task_reads_like_a_claude_code_session() {
        let mut t = Translator::default();
        let started = t.translate(
            &ctx(),
            &envelope(
                "task_started",
                json!({ "mode": "manual", "executor": "demo", "model": "" }),
            ),
        );
        let Out::Hook(start) = &started[0] else {
            panic!("{started:?}")
        };
        assert_eq!(start["hook_event_name"], "SessionStart");
        assert_eq!(start["session_id"], "t1");
        assert_eq!(start["cwd"], "/work/demo");
        assert_eq!(start["coucou_agent"], AGENT);
        let Out::Hook(prompt) = &started[1] else {
            panic!()
        };
        assert_eq!(prompt["hook_event_name"], "UserPromptSubmit");
        assert_eq!(prompt["prompt"], "Add a demo note");
    }

    #[test]
    fn an_approval_carries_its_tool_and_ends_with_the_task() {
        let mut t = Translator::default();
        let args = json!({ "path": "/work/demo/NOTE.md", "content": "hi" });
        let pre = t.translate(
            &ctx(),
            &envelope(
                "tool_requested",
                json!({ "call_id": "c1", "tool": "write_file", "args": args }),
            ),
        );
        let Out::Hook(pre) = &pre[0] else { panic!() };
        assert_eq!(pre["hook_event_name"], "PreToolUse");
        assert_eq!(pre["tool_input"]["path"], "/work/demo/NOTE.md");

        let ask = t.translate(
            &ctx(),
            &envelope(
                "approval_requested",
                json!({ "approval_id": "a1", "call_id": "c1", "summary": "write NOTE.md", "options": ["allow", "deny"] }),
            ),
        );
        let Out::Permission {
            task_id,
            approval_id,
            payload,
        } = &ask[0]
        else {
            panic!("{ask:?}")
        };
        assert_eq!((task_id.as_str(), approval_id.as_str()), ("t1", "a1"));
        assert_eq!(payload["hook_event_name"], "PermissionRequest");
        assert_eq!(payload["tool_name"], "write_file");
        assert_eq!(payload["tool_input"]["content"], "hi");

        assert_eq!(
            t.translate(
                &ctx(),
                &envelope(
                    "approval_resolved",
                    json!({ "approval_id": "a1", "choice": "allow", "by": "user" })
                )
            ),
            vec![Out::Settled {
                approval_id: "a1".into()
            }]
        );
        let done = t.translate(
            &ctx(),
            &envelope(
                "tool_finished",
                json!({ "call_id": "c1", "ok": false, "exit_code": 1, "summary": "denied" }),
            ),
        );
        let Out::Hook(done) = &done[0] else { panic!() };
        assert_eq!(done["hook_event_name"], "PostToolUseFailure");
        assert_eq!(done["tool_name"], "write_file");

        let end = t.translate(
            &ctx(),
            &envelope("task_completed", json!({ "summary": "Done." })),
        );
        let Out::Hook(stop) = &end[0] else { panic!() };
        assert_eq!(stop["hook_event_name"], "Stop");
        assert_eq!(stop["last_assistant_message"], "Done.");
        assert_eq!(
            end[1],
            Out::Ended {
                task_id: "t1".into()
            }
        );
    }

    #[test]
    fn failures_and_cancels_map_to_coucous_events() {
        let mut t = Translator::default();
        let failed = t.translate(&ctx(), &envelope("task_failed", json!({ "error": "boom" })));
        let Out::Hook(f) = &failed[0] else { panic!() };
        assert_eq!(f["hook_event_name"], "StopFailure");
        let cancelled = t.translate(&ctx(), &envelope("task_cancelled", json!({})));
        let Out::Hook(c) = &cancelled[0] else {
            panic!()
        };
        assert_eq!(c["hook_event_name"], "Interrupt");
        assert_eq!(
            t.translate(&ctx(), &envelope("message_delta", json!({ "text": "x" }))),
            Vec::<Out>::new()
        );
    }
}
