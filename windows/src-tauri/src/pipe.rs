// Relay server for coucou-hook.
//
// Windows: the named pipe `\\.\pipe\coucou-<sid>`, one instance per connection.
// Linux: the Unix socket `$XDG_RUNTIME_DIR/coucou.sock`. Every hook event is
// forwarded to the island as a `hook` event. `PermissionRequest` keeps its
// connection open: it waits for the island's decision and writes it back on the
// same connection, which is how approving from the island works. Antigravity's
// `PreToolUse` (tagged `agy_pre_tool`) is the other one: it is answered `ask` at
// once, or `pending` and then the click when its command is put on the card
// (agy_pre_tool below, agy_approvals.rs).
//
// Claude Code is never blocked by us. Three things guarantee it:
//   * coucou-hook gives the connection 300 ms and exits cleanly if we are closed;
//   * we only wait for a human once the island has *confirmed* the card is on
//     screen, so a paused island or a webview that is not listening costs a few
//     hundred milliseconds, not two minutes;
//   * whatever happens we drop the connection after the decision timeout, and
//     the terminal takes over.
//
// What we write back is the bare word `allow` or `deny` — or, when the request
// was Claude Code asking a question, `{"answers":{…}}` with what was picked on
// the island. Turning either into the documented hookSpecificOutput JSON is
// coucou-hook's job, so the wire format Claude Code expects lives in exactly
// one place.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;

use crate::agy_approvals;
use crate::island::WINDOW_LABEL;
use crate::log;
use crate::session_window;

/// Slightly under coucou-hook's own 110 s wait, so we always answer first.
pub(crate) const DECISION_TIMEOUT: Duration = Duration::from_secs(108);
/// How long the island gets to say "the card is up". This is the whole of B4:
/// without it, an island that is paused, hidden behind a crashed webview or
/// simply not listening would leave Claude Code staring at a prompt nobody can
/// see for nearly two minutes.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_millis(800);
const MAX_PAYLOAD: usize = 1 << 20;

/// What the island can say about a permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The card is on screen and a human can act on it.
    Ack,
    /// A human clicked: `allow`, `deny`, or `always` (Mochi medium tools only).
    Decision(String),
    /// Nobody can act on it — paused, or another request already holds the card.
    Decline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingKind {
    ExternalHook,
    MochiTool { allow_always: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnswerOutcome {
    pub is_mochi: bool,
}

/// Permission requests the island has been told about.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<String, PendingReply>>);

struct PendingReply {
    sender: mpsc::Sender<Reply>,
    deadline: Instant,
    kind: PendingKind,
}

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// `\\.\pipe\coucou-<sid>` — must match coucou-hook's `pipe_path()` exactly.
#[cfg(windows)]
pub fn pipe_name() -> String {
    let key = crate::platform::current_user_sid()
        .unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_else(|_| "user".into()));
    format!(r"\\.\pipe\coucou-{key}")
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
                log::line(format!("cannot open the relay pipe: {err}"));
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
                    log::line(format!("cannot reopen the relay pipe: {err}"));
                    return;
                }
            };
            let connected = std::mem::replace(&mut server, next);
            let app = app.clone();
            tauri::async_runtime::spawn(async move { handle(app, connected).await });
        }
    });
}

#[cfg(target_os = "linux")]
pub fn start(app: AppHandle) {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    tauri::async_runtime::spawn(async move {
        let Some(path) = crate::platform::relay_socket_path() else {
            log::line("no private runtime directory ($XDG_RUNTIME_DIR) — Claude Code hooks are inactive");
            return;
        };
        // A socket file left behind by a crash answers nothing and can go. One
        // that answers belongs to a Coucou that is still running: like
        // first_pipe_instance on Windows, we refuse to serve on top of it.
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                log::line("another Coucou already serves the relay socket");
                return;
            }
            let _ = std::fs::remove_file(&path);
        }
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(err) => {
                log::line(format!("cannot open the relay socket: {err}"));
                return;
            }
        };
        // The runtime directory is already 0700; this is belt and braces.
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        let uid = unsafe { libc::getuid() };
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            // Only the relay run by our own user may drive the island.
            if !matches!(stream.peer_cred(), Ok(c) if c.uid() == uid) {
                log::line("refused a relay connection from another user");
                continue;
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move { handle(app, stream).await });
        }
    });
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
#[cfg(target_os = "linux")]
impl Relay for tokio::net::UnixStream {
    /// SO_PEERCRED: the process that connected, as the kernel saw it.
    fn client_pid(&self) -> Option<u32> {
        let pid = self.peer_cred().ok()?.pid()?;
        u32::try_from(pid).ok().filter(|pid| *pid > 1)
    }
}

async fn handle(app: AppHandle, mut pipe: impl Relay) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') || buf.len() > MAX_PAYLOAD {
                    break;
                }
            }
            Err(_) => return,
        }
    }
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
            log::line(format!("hook {event}"));
        }
        let _ = app.emit_to(WINDOW_LABEL, "hook", payload);
        pipe.finish();
        return;
    }

    let id = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let (tx, mut rx) = mpsc::channel::<Reply>(4);
    {
        let pending = app.state::<Pending>();
        pending.0.lock().unwrap().insert(id.clone(), PendingReply {
            sender: tx,
            deadline: Instant::now() + ACK_TIMEOUT + DECISION_TIMEOUT,
            kind: PendingKind::ExternalHook,
        });
    }
    crate::recap::note_request(&app, &id, &payload);
    payload["request_id"] = json!(id);
    log::line(format!("hook PermissionRequest id={id}"));
    let _ = app.emit_to(WINDOW_LABEL, "hook", payload);

    // Answered in the terminal (or the app) instead: Claude Code ends the relay,
    // its end of the connection closes, and the island must drop the card at
    // once rather than keep offering a question that is already settled.
    let decision = tokio::select! {
        d = wait_for_decision(&id, &mut rx) => d,
        () = client_gone(&mut pipe) => {
            log::line(format!("hook id={id} answered elsewhere"));
            crate::recap::forget_request(&app, &id);
            None
        }
    };
    app.state::<Pending>().0.lock().unwrap().remove(&id);
    #[cfg(windows)]
    if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
        crate::permission_notifications::clear(&toasts, &id);
    }

    // No decision: say nothing at all. coucou-hook then writes nothing to stdout
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
    log::line("hook PreToolUse");
    let _ = app.emit_to(WINDOW_LABEL, "hook", payload.clone());
    let Some(gate) = gate else {
        reply_line(&mut pipe, "ask").await;
        pipe.finish();
        return;
    };

    let id = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let (tx, mut rx) = mpsc::channel::<Reply>(4);
    app.state::<Pending>().0.lock().unwrap().insert(id.clone(), PendingReply {
        sender: tx,
        deadline: Instant::now() + ACK_TIMEOUT + gate.wait,
        kind: PendingKind::ExternalHook,
    });
    crate::recap::note_request(&app, &id, &payload);
    let mut card = payload;
    card["hook_event_name"] = json!("PermissionRequest");
    card["request_id"] = json!(id);
    card["tool_input"]["command"] = json!(gate.command);
    log::line(format!("hook Antigravity command waits for the island id={id}"));
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
                        log::line(format!("hook id={id} timed out — Antigravity asks"));
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
    #[cfg(windows)]
    if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
        crate::permission_notifications::clear(&toasts, &id);
    }
    // Only a click on Allow or Deny is a decision; `ask` hands the command back to
    // Antigravity's own prompt, where its Always allow lives.
    let word = match decision.as_deref() {
        Some("allow") => "allow",
        Some("deny") => "deny",
        _ => "ask",
    };
    log::line(format!("hook id={id} answered {word}"));
    if word == "ask" {
        crate::recap::forget_request(&app, &id);
        let _ = app.emit_to(WINDOW_LABEL, "approval-expired", &id);
    }
    reply_line(&mut pipe, word).await;
    pipe.finish();
}

/// The relay left before an answer: forget the request, take the card down.
fn agy_settled(app: &AppHandle, id: &str, why: &str) {
    log::line(format!("hook id={id} {why}"));
    app.state::<Pending>().0.lock().unwrap().remove(id);
    #[cfg(windows)]
    if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
        crate::permission_notifications::clear(&toasts, id);
    }
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
            log::line(format!("hook id={id} answered {}", loggable(&d)));
            return Some(d);
        }
        Ok(Some(Reply::Decline)) => {
            log::line(format!("hook id={id} not shown — terminal takes over"));
            return None;
        }
        Ok(None) => return None,
        Err(_) => {
            log::line(format!("hook id={id} island never acknowledged — terminal takes over"));
            return None;
        }
    }

    match tokio::time::timeout(DECISION_TIMEOUT, rx.recv()).await {
        Ok(Some(Reply::Decision(d))) => {
            log::line(format!("hook id={id} answered {}", loggable(&d)));
            Some(d)
        }
        Ok(Some(Reply::Decline)) => {
            log::line(format!("hook id={id} released without a decision"));
            None
        }
        _ => {
            log::line(format!("hook id={id} timed out — terminal takes over"));
            None
        }
    }
}

/// The log says a question was answered, never with what.
fn loggable(decision: &str) -> &str {
    if decision.starts_with('{') { "a question" } else { decision }
}

fn clear_toast(app: &AppHandle, request_id: &str) {
    #[cfg(windows)]
    if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
        crate::permission_notifications::clear(&toasts, request_id);
    }
    #[cfg(not(windows))]
    let _ = (app, request_id);
}

pub(crate) fn normalize_decision(kind: PendingKind, decision: &str) -> &'static str {
    match (kind, decision) {
        (PendingKind::MochiTool { allow_always: true }, "always") => "always",
        (_, "allow" | "always") => "allow",
        _ => "deny",
    }
}

#[allow(dead_code)]
pub(crate) fn next_mochi_request_id() -> String {
    format!("mochi-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

impl Pending {
    pub(crate) fn insert_mochi(
        &self,
        request_id: String,
        allow_always: bool,
        ack_timeout: Duration,
        decision_timeout: Duration,
    ) -> mpsc::Receiver<Reply> {
        let (tx, rx) = mpsc::channel::<Reply>(4);
        self.0.lock().unwrap().insert(request_id, PendingReply {
            sender: tx,
            deadline: Instant::now() + ack_timeout + decision_timeout,
            kind: PendingKind::MochiTool { allow_always },
        });
        rx
    }

    pub(crate) fn insert_external_for_test(
        &self,
        request_id: String,
        timeout: Duration,
    ) -> mpsc::Receiver<Reply> {
        let (tx, rx) = mpsc::channel::<Reply>(4);
        self.0.lock().unwrap().insert(request_id, PendingReply {
            sender: tx,
            deadline: Instant::now() + timeout,
            kind: PendingKind::ExternalHook,
        });
        rx
    }

    fn send_reply(&self, request_id: &str, reply: Reply, keep: bool) -> bool {
        let mut map = self.0.lock().unwrap();
        let Some(entry) = map.get(request_id) else {
            log::line(format!("reply for id={request_id} — no pending request"));
            return false;
        };
        if entry.sender.try_send(reply).is_err() {
            log::line(format!("reply for id={request_id} — channel is full"));
            return false;
        }
        if !keep {
            map.remove(request_id);
        }
        true
    }

    pub(crate) fn acknowledge_request(&self, request_id: &str) -> bool {
        self.send_reply(request_id, Reply::Ack, true)
    }

    pub(crate) fn decline_request(&self, request_id: &str) -> bool {
        self.send_reply(request_id, Reply::Decline, false)
    }

    pub(crate) fn answer_request(&self, request_id: &str, decision: &str) -> Option<AnswerOutcome> {
        let mut map = self.0.lock().unwrap();
        let entry = map.get(request_id)?;
        let is_mochi = matches!(entry.kind, PendingKind::MochiTool { .. });
        let word = normalize_decision(entry.kind, decision);
        log::line(format!("decision id={request_id} {word}"));
        if entry.sender.try_send(Reply::Decision(word.to_string())).is_err() {
            log::line(format!("reply for id={request_id} — channel is full"));
            return None;
        }
        map.remove(request_id);
        Some(AnswerOutcome { is_mochi })
    }

    pub(crate) fn is_mochi_allow_always(&self, request_id: &str) -> bool {
        matches!(
            self.0.lock().unwrap().get(request_id).map(|entry| entry.kind),
            Some(PendingKind::MochiTool { allow_always: true })
        )
    }

    pub(crate) fn remove_mochi(&self, request_id: &str) -> bool {
        let mut map = self.0.lock().unwrap();
        if matches!(map.get(request_id).map(|entry| entry.kind), Some(PendingKind::MochiTool { .. })) {
            map.remove(request_id);
            true
        } else {
            false
        }
    }

    pub(crate) fn remaining_duration(&self, request_id: &str) -> Option<Duration> {
        self.0.lock().unwrap().get(request_id)
            .map(|entry| entry.deadline.saturating_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero())
    }
}

fn send(app: &AppHandle, request_id: &str, reply: Reply, keep: bool) -> bool {
    app.state::<Pending>().send_reply(request_id, reply, keep)
}

/// Registers an internal Mochi tool approval request and emits it to the island.
#[allow(dead_code)]
pub(crate) fn start_mochi_request(
    app: &AppHandle,
    request_id: String,
    payload: Value,
    allow_always: bool,
) -> Option<mpsc::Receiver<Reply>> {
    let pending = app.try_state::<Pending>()?;
    let rx = pending.insert_mochi(request_id.clone(), allow_always, ACK_TIMEOUT, DECISION_TIMEOUT);
    log::line(format!("mochi PermissionRequest id={request_id}"));
    let _ = app.emit_to(WINDOW_LABEL, "hook", payload);
    Some(rx)
}

/// Cleans up a Mochi request after waiting completes; emits `approval-expired` on timeout or decline.
#[allow(dead_code)]
pub(crate) fn finish_mochi_request(app: &AppHandle, request_id: &str, declined: bool) {
    let removed = app
        .try_state::<Pending>()
        .is_some_and(|pending| pending.remove_mochi(request_id));
    clear_toast(app, request_id);
    if declined || removed {
        let _ = app.emit_to(WINDOW_LABEL, "approval-expired", request_id);
    }
}

/// Cancels/expires a pending Mochi request from outside (e.g., cancellation or timeout).
#[allow(dead_code)]
pub(crate) fn expire_mochi_request(app: &AppHandle, request_id: &str) -> bool {
    let removed = app
        .try_state::<Pending>()
        .is_some_and(|pending| pending.remove_mochi(request_id));
    if removed {
        log::line(format!("mochi id={request_id} expired or cancelled"));
        clear_toast(app, request_id);
        let _ = app.emit_to(WINDOW_LABEL, "approval-expired", request_id);
    }
    removed
}

pub(crate) fn is_mochi_allow_always(app: &AppHandle, request_id: &str) -> bool {
    app.try_state::<Pending>()
        .is_some_and(|pending| pending.is_mochi_allow_always(request_id))
}

/// Remaining relay time. Toast actions expire no later than the request itself.
pub fn remaining(app: &AppHandle, request_id: &str) -> Option<Duration> {
    app.state::<Pending>().remaining_duration(request_id)
}

/// The island has the card on screen; the long wait may begin.
pub fn acknowledge(app: &AppHandle, request_id: &str) {
    let _ = app.state::<Pending>().acknowledge_request(request_id);
}

/// Nobody can act on this one — paused, or another card already holds the view.
pub fn decline(app: &AppHandle, request_id: &str) {
    log::line(format!("decline id={request_id}"));
    let _ = app.state::<Pending>().decline_request(request_id);
    clear_toast(app, request_id);
}

/// Called by the island's Allow / Deny / Always allow buttons.
pub(crate) fn answer_with_outcome(
    app: &AppHandle,
    request_id: &str,
    decision: &str,
) -> Option<AnswerOutcome> {
    let outcome = app.state::<Pending>().answer_request(request_id, decision)?;
    clear_toast(app, request_id);
    let _ = app.emit_to(WINDOW_LABEL, "approval-decided", request_id);
    Some(outcome)
}

/// Called by the island's Allow / Deny buttons. Only ever a bare word: turning
/// it into Claude Code's JSON is coucou-hook's job.
pub fn answer(app: &AppHandle, request_id: &str, decision: &str) -> bool {
    answer_with_outcome(app, request_id, decision).is_some()
}

/// Called when an option is picked for a question Claude Code asked. `answers`
/// maps each question's text to the chosen label, which is the shape
/// AskUserQuestion takes them in.
pub fn answer_question(app: &AppHandle, request_id: &str, answers: &HashMap<String, serde_json::Value>) {
    log::line(format!("decision id={request_id} answered a question"));
    // One line: the relay reads up to the first newline.
    let line = json!({ "answers": answers }).to_string();
    if send(app, request_id, Reply::Decision(line), false) {
        clear_toast(app, request_id);
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn mochi_pending_preserves_always_only_when_allowed() {
        block_on(async {
            let pending = Pending::default();

            let mut rx_med = pending.insert_mochi(
                "m-med".into(),
                true,
                Duration::from_millis(200),
                Duration::from_secs(5),
            );
            assert!(pending.is_mochi_allow_always("m-med"));
            assert!(pending.remaining_duration("m-med").is_some());
            assert!(pending.acknowledge_request("m-med"));
            assert_eq!(rx_med.recv().await, Some(Reply::Ack));
            assert_eq!(
                pending.answer_request("m-med", "always"),
                Some(AnswerOutcome { is_mochi: true })
            );
            assert_eq!(rx_med.recv().await, Some(Reply::Decision("always".into())));
            assert!(pending.remaining_duration("m-med").is_none());

            // Risco alto (allow_always = false) nunca preserva "always".
            let mut rx_high = pending.insert_mochi(
                "m-high".into(),
                false,
                Duration::from_millis(200),
                Duration::from_secs(5),
            );
            assert!(!pending.is_mochi_allow_always("m-high"));
            assert_eq!(
                pending.answer_request("m-high", "always"),
                Some(AnswerOutcome { is_mochi: true })
            );
            assert_eq!(rx_high.recv().await, Some(Reply::Decision("allow".into())));

            // Hooks externos também normalizam "always" para "allow".
            let mut rx_ext = pending.insert_external_for_test("ext-1".into(), Duration::from_secs(5));
            assert!(!pending.is_mochi_allow_always("ext-1"));
            assert_eq!(
                pending.answer_request("ext-1", "always"),
                Some(AnswerOutcome { is_mochi: false })
            );
            assert_eq!(rx_ext.recv().await, Some(Reply::Decision("allow".into())));
        });
    }

    #[test]
    fn remove_mochi_only_removes_mochi_requests_and_closes_channel() {
        block_on(async {
            let pending = Pending::default();
            let mut rx_ext = pending.insert_external_for_test("ext-2".into(), Duration::from_secs(5));
            let mut rx_mochi = pending.insert_mochi(
                "m-cancel".into(),
                false,
                Duration::from_millis(200),
                Duration::from_secs(5),
            );

            assert!(!pending.remove_mochi("ext-2"));
            assert!(pending.remaining_duration("ext-2").is_some());
            assert!(pending.remove_mochi("m-cancel"));
            assert_eq!(rx_mochi.recv().await, None);

            assert!(pending.decline_request("ext-2"));
            assert_eq!(rx_ext.recv().await, Some(Reply::Decline));
        });
    }
}
