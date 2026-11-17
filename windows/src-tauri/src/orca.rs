// Orca as an execution environment: its terminals show in Settings → Active pills
// and its install state in Settings → Agents. Read-only, except "Open terminal",
// which only moves focus in Orca. Orca missing or closed is not an error.

use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Per CLI call, so a hung Orca cannot hang Settings.
const TIMEOUT: Duration = Duration::from_secs(4);
const FOCUS_FAILED: &str = "Não foi possível abrir o terminal no Orca. Confira se o Orca está aberto.";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrcaTerminal {
    pub handle: String,
    pub title: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrcaState {
    pub installed: bool,
    pub running: bool,
    pub terminals: Vec<OrcaTerminal>,
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "orca.exe" } else { "orca" }
}

/// PATH first, then the Windows app's install folder. Never `orca.cmd`.
fn locate_in(path_var: Option<&std::ffi::OsStr>, local_app_data: Option<&Path>) -> Option<PathBuf> {
    let on_path = path_var.into_iter().flat_map(std::env::split_paths).map(|dir| dir.join(exe_name()));
    let install = local_app_data
        .filter(|_| cfg!(windows))
        .map(|dir| dir.join("Programs").join("orca").join("resources").join("bin").join(exe_name()));
    on_path.chain(install).find(|p| p.is_file())
}

fn locate() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    locate_in(std::env::var_os("PATH").as_deref(), local.as_deref())
}

/// The `result` of a reply with `ok: true`; anything else is no answer.
fn reply_result(text: &str) -> Option<Value> {
    let reply: Value = serde_json::from_str(text).ok()?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    reply.get("result").cloned()
}

/// Runs the CLI with --json, without a console window and with a time limit.
fn run(args: &[&str]) -> Option<Value> {
    let mut cmd = Command::new(locate()?);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    crate::platform::no_console(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on another thread, so a full pipe never stalls the child.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let text = reader.join().ok()?;
    if !status.success() {
        return None;
    }
    reply_result(&text)
}

/// `term_` and a UUID: the only shape a handle may take before it becomes an argument.
pub fn valid_handle(handle: &str) -> bool {
    let Some(uuid) = handle.strip_prefix("term_") else { return false };
    let parts: Vec<&str> = uuid.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

fn parse_running(result: &Value) -> bool {
    result.pointer("/app/running").and_then(Value::as_bool).unwrap_or(false)
}

fn terminal_row(row: &Value) -> Option<OrcaTerminal> {
    let handle = row.get("handle").and_then(Value::as_str)?;
    if !valid_handle(handle) {
        return None;
    }
    let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let title = text("title").or_else(|| text("paneTitle")).unwrap_or("Terminal");
    Some(OrcaTerminal {
        handle: handle.to_string(),
        title: title.chars().take(80).collect(),
        connected: row.get("connected").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Rows that are not objects or lack a valid handle are skipped, not fatal.
fn parse_terminals(result: &Value) -> Vec<OrcaTerminal> {
    let rows = result.get("terminals").and_then(Value::as_array).cloned().unwrap_or_default();
    rows.iter().filter_map(terminal_row).collect()
}

/// Install, whether the app runs, and its terminals. Never fails: a missing or
/// closed Orca just reports fewer things.
pub fn state() -> OrcaState {
    let mut state = OrcaState { installed: locate().is_some(), ..OrcaState::default() };
    if !state.installed {
        return state;
    }
    state.running = run(&["status", "--json"]).is_some_and(|r| parse_running(&r));
    if state.running {
        state.terminals = run(&["terminal", "list", "--limit", "200", "--json"])
            .map(|r| parse_terminals(&r))
            .unwrap_or_default();
    }
    state
}

/// "Open terminal": Orca brings this terminal's tab to the front. Nothing is sent to it.
pub fn focus(handle: &str) -> Result<(), String> {
    if !valid_handle(handle) {
        return Err(FOCUS_FAILED.to_string());
    }
    run(&["terminal", "switch", "--terminal", handle, "--json"])
        .map(|_| ())
        .ok_or_else(|| FOCUS_FAILED.to_string())
}

#[tauri::command]
pub async fn orca_state() -> OrcaState {
    tauri::async_runtime::spawn_blocking(state).await.unwrap_or_default()
}

#[tauri::command]
pub async fn orca_focus_terminal(handle: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || focus(&handle))
        .await
        .unwrap_or_else(|_| Err(FOCUS_FAILED.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sanitized from a real reply with Orca closed (home path removed).
    const STATUS_CLOSED: &str = r#"{"id":"local-status","ok":true,"result":{"target":{"kind":"local"},"app":{"running":false,"pid":null},"runtime":{"state":"not_running","reachable":false,"runtimeId":null},"graph":{"state":"not_running"}},"_meta":{"runtimeId":"none"}}"#;
    const LIST_CLOSED: &str = r#"{"id":"local","ok":false,"error":{"code":"runtime_unavailable","message":"Could not read Orca runtime metadata at <home>\\AppData\\Roaming\\orca\\orca-runtime.json. Start the Orca app first."},"_meta":{"runtimeId":null}}"#;
    // Synthetic reply with the app open: handles and titles are invented.
    const STATUS_OPEN: &str = r#"{"id":"local-status","ok":true,"result":{"app":{"running":true,"pid":1},"runtime":{"state":"ready","reachable":true,"runtimeId":"rt-1"},"graph":{"state":"ready"}}}"#;
    const LIST_OPEN: &str = r#"{"id":"local","ok":true,"result":{"terminals":[
        {"handle":"term_6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5e6f","worktreeId":"id:w1","connected":true,"title":"Claude Code"},
        {"handle":"term_0b7e4d2c-1a3f-4e5d-9c8b-7a6f5e4d3c2b","worktreeId":"id:w2","connected":false,"paneTitle":"pwsh"},
        {"handle":"term_1d2c3b4a-0000-4000-8000-00000000abcd","worktreeId":"id:w3","connected":false},
        {"handle":"term_..\\bad","worktreeId":"id:w4","connected":true,"title":"invalido"},
        {"worktreeId":"id:w5","connected":true,"title":"sem handle"},
        "linha quebrada"
    ]},"_meta":{"runtimeId":"rt-1"}}"#;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("coucou-orca-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_closed_orca_reports_not_running_and_no_terminals() {
        assert!(!parse_running(&reply_result(STATUS_CLOSED).unwrap()));
        assert!(reply_result(LIST_CLOSED).is_none());
        assert!(!parse_running(&Value::Null));
    }

    #[test]
    fn an_open_orca_lists_only_valid_terminals() {
        assert!(parse_running(&reply_result(STATUS_OPEN).unwrap()));
        let terminals = parse_terminals(&reply_result(LIST_OPEN).unwrap());
        let names: Vec<(&str, bool)> = terminals.iter().map(|t| (t.title.as_str(), t.connected)).collect();
        assert_eq!(names, [("Claude Code", true), ("pwsh", false), ("Terminal", false)]);
        assert_eq!(terminals[0].handle, "term_6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5e6f");
    }

    #[test]
    fn a_handle_must_be_term_and_a_uuid() {
        assert!(valid_handle("term_6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5e6f"));
        for bad in [
            "",
            "term_",
            "6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5e6f",
            "term_6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5e6",
            "term_6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5e6f --json",
            "term_..\\..\\x",
            "term_6f1c2a3e-9b8d-4c7a-8e2f-1a2b3c4d5eZZ",
            "term_-x",
        ] {
            assert!(!valid_handle(bad), "{bad}");
        }
    }

    #[test]
    fn a_bad_handle_is_refused_before_orca_runs() {
        assert_eq!(focus("term_x; del").unwrap_err(), FOCUS_FAILED);
    }

    #[test]
    fn the_cli_is_found_on_path_and_never_as_orca_cmd() {
        let dir = scratch("path");
        let path_var = std::env::join_paths([&dir]).unwrap();
        assert!(locate_in(Some(path_var.as_os_str()), None).is_none());
        std::fs::write(dir.join("orca.cmd"), "").unwrap();
        assert!(locate_in(Some(path_var.as_os_str()), None).is_none(), "orca.cmd must never be run");
        std::fs::write(dir.join(exe_name()), "").unwrap();
        assert_eq!(locate_in(Some(path_var.as_os_str()), None), Some(dir.join(exe_name())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_windows_install_folder_is_the_fallback() {
        let dir = scratch("install");
        let bin = dir.join("Programs").join("orca").join("resources").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(exe_name()), "").unwrap();
        let found = locate_in(None, Some(dir.as_path()));
        if cfg!(windows) {
            assert_eq!(found, Some(bin.join(exe_name())));
        } else {
            assert!(found.is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reply_that_is_not_ok_json_or_failed_is_no_answer() {
        assert!(reply_result("não é json").is_none());
        assert!(reply_result(r#"{"ok":false,"error":{}}"#).is_none());
        assert!(reply_result(r#"{"result":{}}"#).is_none());
        assert!(reply_result(r#"{"ok":true,"result":{"terminals":[]}}"#).is_some());
        assert!(parse_terminals(&Value::Null).is_empty());
    }
}
