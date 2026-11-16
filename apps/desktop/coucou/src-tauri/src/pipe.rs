// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/pipe.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
// Relay server for next-agent-hook.
//
// Windows: the named pipe `\\.\pipe\next-agent-<sid>`, one instance per connection.
// Linux: the Unix socket `$XDG_RUNTIME_DIR/next-agent.sock`. Every hook event is
// forwarded to the island as a `hook` event. `PermissionRequest` keeps its
// connection open: it waits for the island's decision and writes it back on the
// same connection, which is how approving from the island works. Antigravity's
// `PreToolUse` (tagged `agy_pre_tool`) is the other one: it is answered `ask` at
// once, or `pending` and then the click when its command is put on the card
// (agy_pre_tool below, agy_approvals.rs).
//
// Claude Code is never blocked by us. Three things guarantee it:
//   * next-agent-hook gives the connection 300 ms and exits cleanly if we are closed;
//   * we only wait for a human once the island has *confirmed* the card is on
//     screen, so a paused island or a webview that is not listening costs a few
//     hundred milliseconds, not two minutes;
//   * whatever happens we drop the connection after the decision timeout, and
//     the terminal takes over.
//
// What we write back is the bare word `allow` or `deny` — or, when the request
// was Claude Code asking a question, `{"answers":{…}}` with what was picked on
// the island. Turning either into the documented hookSpecificOutput JSON is
// next-agent-hook's job, so the wire format Claude Code expects lives in exactly
// one place.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;

use crate::agy_approvals;
use crate::island::WINDOW_LABEL;
use crate::session_window;

/// Slightly under next-agent-hook's own 110 s wait, so we always answer first.
const DECISION_TIMEOUT: Duration = Duration::from_secs(108);
/// How long the island gets to say "the card is up". This is the whole of B4:
/// without it, an island that is paused, hidden behind a crashed webview or
/// simply not listening would leave Claude Code staring at a prompt nobody can
/// see for nearly two minutes.
const ACK_TIMEOUT: Duration = Duration::from_millis(800);
const MAX_PAYLOAD: usize = 1 << 20;
/// Next Agent: relay limits (security review): a relay writes its one line at once. A connection that has
/// not finished it by then is dropped, so a stuck or hostile local client
/// cannot hold tasks open.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Next Agent: relay limits (security review): relay connections served at once. Every Claude Code hook is one
/// short connection, and a waiting permission request one more; anything above
/// this is refused until one ends.
const MAX_CONNECTIONS: usize = 64;

/// Next Agent: relay limits (security review): the connections being served.
static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_CONNECTIONS);

/// Next Agent: relay limits (security review): serves one accepted connection in a slot of its own, or
/// refuses it when all are taken.
fn serve(app: &AppHandle, relay: impl Relay + Send + 'static) {
    let Ok(slot) = SLOTS.try_acquire() else {
        // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
        tracing::warn!(target: "overlay", decision = "refused", reason = "too many connections at once", "relay_connection");
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        handle(app, relay).await;
        drop(slot);
    });
}

/// What the island can say about a permission request.
pub enum Reply {
    /// The card is on screen and a human can act on it.
    Ack,
    /// A human clicked: `allow` or `deny`.
    Decision(String),
    /// Nobody can act on it — paused, or another request already holds the card.
    Decline,
}

/// Permission requests the island has been told about.
#[derive(Default)]
pub struct Pending(pub Mutex<HashMap<String, mpsc::Sender<Reply>>>);

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// `\\.\pipe\next-agent-<sid>` — must match next-agent-hook's `pipe_path()` exactly.
#[cfg(windows)]
pub fn pipe_name() -> String {
    let key = crate::platform::current_user_sid()
        .unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_else(|_| "user".into()));
    format!(r"\\.\pipe\next-agent-{key}")
}

#[cfg(windows)]
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let name = pipe_name();
        // first_pipe_instance also means we refuse to join a pipe somebody else
        // already owns under our name, rather than serving on top of it.
        let mut server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
            Ok(s) => s,
            Err(err) => {
                // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                tracing::warn!(target: "overlay", decision = "failed", error = %err, reason = "cannot open the pipe", "relay_connection");
                return;
            }
        };
        loop {
            if server.connect().await.is_err() {
                // A failed instance never connects again (#853): listen on a fresh one.
                if let Ok(fresh) = ServerOptions::new().create(&name) {
                    server = fresh;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            // Hand the connected instance to a task and listen on a fresh one.
            let next = match ServerOptions::new().create(&name) {
                Ok(s) => s,
                Err(err) => {
                    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                    tracing::warn!(target: "overlay", decision = "failed", error = %err, reason = "cannot reopen the pipe", "relay_connection");
                    return;
                }
            };
            let connected = std::mem::replace(&mut server, next);
            // Next Agent: relay limits (security review): only a relay run by this same user (the SID check
            // next-agent-hook does the other way round), and not too many at once.
            if !connected_client_is_us(&connected) {
                // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                tracing::warn!(target: "overlay", decision = "refused", reason = "another user", "relay_connection");
                continue;
            }
            serve(&app, connected);
        }
    });
}

/// Next Agent: whether a relay peer is known to be another user. An unreadable
/// peer (it already hung up) is not: the socket's 0600 mode inside a private
/// 0700 directory (`relay_socket_path`) already keeps other users out.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn is_other_user<E>(peer: Result<u32, E>, uid: u32) -> bool {
    matches!(peer, Ok(peer) if peer != uid)
}

#[cfg(any(target_os = "linux", target_os = "macos"))] // Next Agent: macOS too (platform/macos.rs)
pub fn start(app: AppHandle) {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    tauri::async_runtime::spawn(async move {
        let Some(path) = crate::platform::relay_socket_path() else {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::warn!(target: "overlay", decision = "failed", reason = "no private runtime directory", "relay_connection");
            return;
        };
        // A socket file left behind by a crash answers nothing and can go. One
        // that answers belongs to a Coucou that is still running: like
        // first_pipe_instance on Windows, we refuse to serve on top of it.
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                tracing::warn!(target: "overlay", decision = "failed", reason = "another instance serves the socket", "relay_connection");
                return;
            }
            let _ = std::fs::remove_file(&path);
        }
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(err) => {
                // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                tracing::warn!(target: "overlay", decision = "failed", error = %err, reason = "cannot open the socket", "relay_connection");
                return;
            }
        };
        // The runtime directory is already 0700; this is belt and braces.
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        #[cfg(target_os = "linux")]
        #[allow(unsafe_code)]
        let uid = unsafe { libc::getuid() };
        #[cfg(target_os = "macos")]
        let uid = crate::platform::current_uid();
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            // Only the relay run by our own user may drive the island.
            // Next Agent: the relay writes its event and hangs up at once, and on
            // macOS the peer's credentials can no longer be read after that
            // (getpeereid fails), so every fire-and-forget hook was refused.
            // Refuse a peer that is known to be someone else; an unreadable one
            // passes, because the socket is 0600 inside our 0700 runtime
            // directory, so only this user could have connected.
            if is_other_user(stream.peer_cred().map(|c| c.uid()), uid) {
                // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                tracing::warn!(target: "overlay", decision = "refused", reason = "another user", "relay_connection");
                continue;
            }
            // Next Agent: relay limits (security review): not too many at once.
            serve(&app, stream);
        }
    });
}

/// Next Agent: relay limits (security review): the process on the other end of the pipe runs as this user.
#[cfg(windows)]
fn connected_client_is_us(server: &NamedPipeServer) -> bool {
    use std::os::windows::io::AsRawHandle;
    let Some(pid) = crate::platform::pipe_client_pid(server.as_raw_handle()) else { return false };
    static OURS: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let ours = OURS.get_or_init(crate::platform::current_user_sid);
    matches!((crate::platform::process_user_sid(pid), ours), (Some(theirs), Some(ours)) if &theirs == ours)
}

/// One accepted relay connection, whatever carries it.
trait Relay: AsyncRead + AsyncWrite + Unpin {
    /// Ends the conversation once everything has been written.
    fn finish(&mut self) {}
    /// The relay process on the other end, where the OS says.
    fn client_pid(&self) -> Option<u32> {
        None
    }
}

#[cfg(windows)]
impl Relay for NamedPipeServer {
    fn finish(&mut self) {
        let _ = self.disconnect();
    }
    fn client_pid(&self) -> Option<u32> {
        use std::os::windows::io::AsRawHandle;
        crate::platform::pipe_client_pid(self.as_raw_handle())
    }
}

/// Dropping the stream closes it; the relay reads up to our newline first.
#[cfg(any(target_os = "linux", target_os = "macos"))] // Next Agent: macOS too
impl Relay for tokio::net::UnixStream {
    /// SO_PEERCRED: the process that connected, as the kernel saw it.
    fn client_pid(&self) -> Option<u32> {
        let pid = self.peer_cred().ok()?.pid()?;
        u32::try_from(pid).ok().filter(|pid| *pid > 1)
    }
}

async fn handle(app: AppHandle, mut pipe: impl Relay) {
    // Next Agent: relay limits (security review): the line must arrive within READ_TIMEOUT.
    let Ok(Some(buf)) = tokio::time::timeout(READ_TIMEOUT, read_line(&mut pipe)).await else {
        // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
        tracing::warn!(target: "overlay", decision = "refused", reason = "no complete line in time", "relay_connection");
        return;
    };
    let line = match buf.iter().position(|b| *b == b'\n') {
        Some(i) => &buf[..i],
        None => &buf[..],
    };
    let Ok(mut payload) = serde_json::from_slice::<Value>(line) else { return };
    if !payload.is_object() {
        return;
    }

    let event = payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    note_session_window(&pipe, &payload, &event);
    crate::herdr::observe(&payload, &event);
    // Counts for the weekly recap — never the command, path or prompt itself.
    crate::recap::observe(&app, &payload);

    // Antigravity's PreToolUse: its relay waits for one line (agy_approvals.rs).
    if event == "PreToolUse" && payload.get("coucou_kind").and_then(Value::as_str) == Some(agy_approvals::KIND) {
        agy_pre_tool(app, pipe, payload).await;
        return;
    }

    if event != "PermissionRequest" {
        // The status line relay calls in with every Claude Code update: not log-worthy.
        if event != "StatusLine" {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = %event, "hook_event");
        }
        let _ = app.emit_to(WINDOW_LABEL, "hook", payload);
        pipe.finish();
        return;
    }

    let id = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let (tx, mut rx) = mpsc::channel::<Reply>(4);
    {
        let pending = app.state::<Pending>();
        pending.0.lock().unwrap().insert(id.clone(), tx);
    }
    crate::recap::note_request(&app, &id, &payload);
    payload["request_id"] = json!(id);
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "PermissionRequest", approval_id = %id, "hook_event");
    let _ = app.emit_to(WINDOW_LABEL, "hook", payload);

    // Answered in the terminal (or the app) instead: Claude Code ends the relay,
    // its end of the connection closes, and the island must drop the card at
    // once rather than keep offering a question that is already settled.
    let decision = tokio::select! {
        d = wait_for_decision(&id, &mut rx) => d,
        () = client_gone(&mut pipe) => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "answered_elsewhere", approval_id = %id, "hook_event");
            crate::recap::forget_request(&app, &id);
            None
        }
    };
    app.state::<Pending>().0.lock().unwrap().remove(&id);

    // No decision: say nothing at all. next-agent-hook then writes nothing to stdout
    // and Claude Code asks in the terminal, exactly as if Coucou were closed.
    // The island is told, so a card that came up late (or is still up after the
    // long wait) goes instead of holding the view with nobody to answer (#857).
    match decision {
        Some(d) => {
            let _ = pipe.write_all(format!("{d}\n").as_bytes()).await;
            let _ = pipe.flush().await;
        }
        None => {
            let _ = app.emit_to(WINDOW_LABEL, "approval-expired", &id);
        }
    }
    pipe.finish();
}

/// Antigravity is about to run a tool and its relay waits for a line. The island always
/// gets the event, as for any agent. Then, when Approve commands from the notch is on and
/// none of Antigravity's rules covers the command, the card comes up as a permission
/// request: once the island says it is on screen the relay is told `pending` (it then
/// waits for the click instead of a few seconds), and Allow, Deny or Reply in terminal
/// (`ask`) follow. Anything else — no card, no ack, no click in time — is `ask`, and
/// Antigravity shows its own prompt. The same flow as the Mac's processAgyApproval.
async fn agy_pre_tool(app: AppHandle, mut pipe: impl Relay, payload: Value) {
    let enabled = app.state::<crate::Shared>().settings.lock().unwrap().agy_approvals_enabled;
    let gate = agy_approvals::gate(&payload, enabled, &crate::platform::home_dir());
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "PreToolUse", "hook_event");
    let _ = app.emit_to(WINDOW_LABEL, "hook", payload.clone());
    let Some(gate) = gate else {
        reply_line(&mut pipe, "ask").await;
        pipe.finish();
        return;
    };

    let id = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let (tx, mut rx) = mpsc::channel::<Reply>(4);
    app.state::<Pending>().0.lock().unwrap().insert(id.clone(), tx);
    crate::recap::note_request(&app, &id, &payload);
    let mut card = payload;
    card["hook_event_name"] = json!("PermissionRequest");
    card["request_id"] = json!(id);
    card["tool_input"]["command"] = json!(gate.command);
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "agy_waits_for_island", approval_id = %id, "hook_event");
    let _ = app.emit_to(WINDOW_LABEL, "hook", card);

    // The relay hung up (Antigravity stopped waiting, or the turn was cancelled): the
    // card goes, nothing is written.
    let first = tokio::select! {
        r = tokio::time::timeout(ACK_TIMEOUT, rx.recv()) => r.ok().flatten(),
        () = client_gone(&mut pipe) => {
            agy_settled(&app, &id, "answered elsewhere");
            return;
        }
    };
    let decision = match first {
        Some(Reply::Ack) => {
            reply_line(&mut pipe, "pending").await;
            tokio::select! {
                r = tokio::time::timeout(gate.wait, rx.recv()) => match r {
                    Ok(Some(Reply::Decision(d))) => Some(d),
                    Ok(Some(_)) | Ok(None) => None,
                    Err(_) => {
                        // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
                        tracing::info!(target: "overlay", kind = "agy_timed_out", approval_id = %id, "hook_event");
                        None
                    }
                },
                () = client_gone(&mut pipe) => {
                    agy_settled(&app, &id, "answered elsewhere");
                    return;
                }
            }
        }
        // A click that beats the ack is still a click.
        Some(Reply::Decision(d)) => Some(d),
        Some(Reply::Decline) | None => None,
    };
    app.state::<Pending>().0.lock().unwrap().remove(&id);
    // Only a click on Allow or Deny is a decision; `ask` hands the command back to
    // Antigravity's own prompt, where its Always allow lives.
    let word = match decision.as_deref() {
        Some("allow") => "allow",
        Some("deny") => "deny",
        _ => "ask",
    };
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "answered", decision = word, approval_id = %id, "hook_event");
    if word == "ask" {
        crate::recap::forget_request(&app, &id);
        let _ = app.emit_to(WINDOW_LABEL, "approval-expired", &id);
    }
    reply_line(&mut pipe, word).await;
    pipe.finish();
}

/// The relay left before an answer: forget the request, take the card down.
fn agy_settled(app: &AppHandle, id: &str, why: &str) {
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "settled", reason = why, approval_id = %id, "hook_event");
    app.state::<Pending>().0.lock().unwrap().remove(id);
    crate::recap::forget_request(app, id);
    let _ = app.emit_to(WINDOW_LABEL, "approval-expired", id);
}

async fn reply_line(pipe: &mut impl Relay, word: &str) {
    let _ = pipe.write_all(format!("{word}\n").as_bytes()).await;
    let _ = pipe.flush().await;
}

/// Resolves once the relay has gone (end of stream or a broken pipe). It sends
/// nothing after its one line, so any read result means it is done.
async fn client_gone(pipe: &mut impl Relay) {
    let mut probe = [0u8; 64];
    loop {
        match pipe.read(&mut probe).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// Finds, once per session, the window it runs in — see session_window.rs.
///
/// Only while the session is unknown, so the process snapshot is not taken on
/// every event. The relay must still be running for its parents to be found:
/// a permission request always is (it waits for us), a quick event may already
/// have exited, and then a later event of the session tries again.
fn note_session_window(pipe: &impl Relay, payload: &Value, event: &str) {
    let Some(session) = payload.get("session_id").and_then(Value::as_str) else { return };
    if event == "SessionEnd" {
        session_window::forget(session);
        return;
    }
    if session_window::known(session) {
        return;
    }
    let Some(relay) = pipe.client_pid() else { return };
    let ancestors = crate::platform::process_ancestors(relay);
    // No ancestors: the relay was already gone, so try again next time. Some,
    // but none with a window (a classic console): settled, VS Code it is.
    if !ancestors.is_empty() {
        session_window::remember(session, crate::platform::window_owners(&ancestors));
    }
}

/// Two waits: a short one for "the card is up", then the long one for a human.
async fn wait_for_decision(id: &str, rx: &mut mpsc::Receiver<Reply>) -> Option<String> {
    match tokio::time::timeout(ACK_TIMEOUT, rx.recv()).await {
        Ok(Some(Reply::Ack)) => {}
        // A click that beats the ack is still a click.
        Ok(Some(Reply::Decision(d))) => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "answered", decision = loggable(&d), approval_id = %id, "hook_event");
            return Some(d);
        }
        Ok(Some(Reply::Decline)) => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "not_shown", approval_id = %id, "hook_event");
            return None;
        }
        Ok(None) => return None,
        Err(_) => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "not_acknowledged", approval_id = %id, "hook_event");
            return None;
        }
    }

    match tokio::time::timeout(DECISION_TIMEOUT, rx.recv()).await {
        Ok(Some(Reply::Decision(d))) => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "answered", decision = loggable(&d), approval_id = %id, "hook_event");
            Some(d)
        }
        Ok(Some(Reply::Decline)) => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "released", approval_id = %id, "hook_event");
            None
        }
        _ => {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::info!(target: "overlay", kind = "timed_out", approval_id = %id, "hook_event");
            None
        }
    }
}

/// The log says a question was answered, never with what.
fn loggable(decision: &str) -> &str {
    if decision.starts_with('{') { "question" } else { decision }
}

fn send(app: &AppHandle, request_id: &str, reply: Reply, keep: bool) {
    let sender = {
        let pending = app.state::<Pending>();
        let mut map = pending.0.lock().unwrap();
        if keep { map.get(request_id).cloned() } else { map.remove(request_id) }
    };
    match sender {
        Some(tx) => {
            let _ = tx.try_send(reply);
        }
        // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
        None => tracing::warn!(target: "overlay", kind = "no_pending_request", approval_id = %request_id, "hook_event"),
    }
}

/// One relay line, at most MAX_PAYLOAD bytes (upstream's loop, moved here so
/// `handle` can put a time limit on it). None: the connection failed.
/// Next Agent: relay limits (security review): a payload over the limit is refused, not cut.
async fn read_line(pipe: &mut impl Relay) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if chunk[..n].contains(&b'\n') {
                    break;
                }
                if buf.len() > MAX_PAYLOAD {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    Some(buf)
}

/// The island has the card on screen; the long wait may begin.
pub fn acknowledge(app: &AppHandle, request_id: &str) {
    send(app, request_id, Reply::Ack, true);
}

/// Nobody can act on this one — paused, or another card already holds the view.
pub fn decline(app: &AppHandle, request_id: &str) {
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "decline", approval_id = %request_id, "hook_event");
    send(app, request_id, Reply::Decline, false);
}

/// Called by the island's Allow / Deny buttons. Only ever a bare word: turning
/// it into Claude Code's JSON is next-agent-hook's job.
pub fn answer(app: &AppHandle, request_id: &str, decision: &str) {
    let word = match decision {
        "allow" | "always" => "allow",
        _ => "deny",
    };
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "decision", decision = word, approval_id = %request_id, "hook_event");
    send(app, request_id, Reply::Decision(word.to_string()), false);
}

// ── Next Agent: its own tasks on the same input path ─────────────────────────

/// Next Agent's own task events come in here, on the same path as a
/// next-agent-hook connection: an in-memory stream that `handle` reads exactly
/// like a relay's. Next Agent writes one hook-shaped line on the stream it
/// gets back and, for a `PermissionRequest`, reads the decision line (`allow`
/// or `deny`) from it. Dropping it is the relay going away: a card still up
/// goes ("answered elsewhere").
pub fn connect_in_process(app: &AppHandle) -> tokio::io::DuplexStream {
    let (ours, theirs) = tokio::io::duplex(MAX_PAYLOAD + 4096);
    let app = app.clone();
    tauri::async_runtime::spawn(async move { handle(app, theirs).await });
    ours
}

/// No process on the other end, so no session window to remember. (The
/// tests below declare the same one for their own streams.)
#[cfg(not(test))]
impl Relay for tokio::io::DuplexStream {}

/// Called when an option is picked for a question Claude Code asked. `answers`
/// maps each question's text to the chosen label, which is the shape
/// AskUserQuestion takes them in.
pub fn answer_question(app: &AppHandle, request_id: &str, answers: &HashMap<String, serde_json::Value>) {
    // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
    tracing::info!(target: "overlay", kind = "question_answered", approval_id = %request_id, "hook_event");
    // One line: the relay reads up to the first newline.
    let line = json!({ "answers": answers }).to_string();
    send(app, request_id, Reply::Decision(line), false);
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_known_other_user_is_refused() {
        assert!(!super::is_other_user::<()>(Ok(501), 501));
        assert!(super::is_other_user::<()>(Ok(0), 501));
        // The relay already hung up: the 0600 socket in a 0700 folder vouches for it.
        assert!(!super::is_other_user(Err("ENOTCONN"), 501));
    }

    use super::*;

    impl Relay for tokio::io::DuplexStream {}

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    /// The card's fast exit: the relay hanging up (Claude Code ended it because
    /// the prompt was answered in the terminal) is seen at once, and a relay
    /// that is still waiting is not mistaken for one that left.
    #[test]
    fn a_relay_that_hangs_up_is_seen_at_once() {
        block_on(async {
            let (mut ours, theirs) = tokio::io::duplex(64);
            let waiting = tokio::time::timeout(Duration::from_millis(50), client_gone(&mut ours)).await;
            assert!(waiting.is_err(), "a live relay must not count as gone");
            drop(theirs);
            let gone = tokio::time::timeout(Duration::from_millis(50), client_gone(&mut ours)).await;
            assert!(gone.is_ok(), "a closed relay must be seen within the poll");
        });
    }

    /// Next Agent: relay limits (security review): one line, bounded in size
    /// and in time.
    #[test]
    fn a_relay_line_is_bounded_in_size_and_time() {
        block_on(async {
            // A line arrives whole, in pieces.
            let (mut ours, mut theirs) = tokio::io::duplex(64);
            tokio::spawn(async move {
                theirs.write_all(br#"{"hook_event_name":"#).await.unwrap();
                theirs.write_all(b"\"Stop\"}\n").await.unwrap();
            });
            let line = read_line(&mut ours).await.unwrap();
            assert!(line.starts_with(br#"{"hook_event_name":"Stop"}"#));

            // Over the limit with no newline: refused, not cut to a parseable prefix.
            let (mut ours, mut theirs) = tokio::io::duplex(MAX_PAYLOAD * 2);
            theirs.write_all(&vec![b' '; MAX_PAYLOAD + 10]).await.unwrap();
            assert!(read_line(&mut ours).await.is_none());

            // Silence: the time limit ends it (READ_TIMEOUT in `handle`).
            let (mut ours, _theirs) = tokio::io::duplex(64);
            let silent = tokio::time::timeout(Duration::from_millis(50), read_line(&mut ours)).await;
            assert!(silent.is_err());
        });
    }

    #[test]
    fn relay_connections_are_capped() {
        let held: Vec<_> = (0..MAX_CONNECTIONS).filter_map(|_| SLOTS.try_acquire().ok()).collect();
        assert_eq!(held.len(), MAX_CONNECTIONS);
        assert!(SLOTS.try_acquire().is_err());
        drop(held);
        assert!(SLOTS.try_acquire().is_ok());
    }
}
