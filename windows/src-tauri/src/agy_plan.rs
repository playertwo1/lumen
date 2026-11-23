// Antigravity plan usage: the 5-hour and weekly limits, asked of the Antigravity
// CLI itself (`agy -p /usage --output-format json`, the way Orca does).
//
// Like codex_plan.rs: Coucou makes no network call and reads no credential, `agy`
// answers with its own sign-in, and nothing is installed or written. It runs only
// when the pill shows or is clicked, one at a time, for 30 s at most, and is skipped
// while Coucou is paused. An `agy` older than 1.1.11 answers `/usage` as a prompt
// and spends quota, so the version is checked first.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{self, AtomicBool};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::platform;

const TIMEOUT: Duration = Duration::from_secs(30);
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// What we are willing to read from `agy`; the answer is a few KB.
const MAX_OUTPUT: u64 = 512 * 1024;
const MIN_VERSION: (u64, u64, u64) = (1, 1, 11);

static BUSY: AtomicBool = AtomicBool::new(false);

struct NotBusy;
impl Drop for NotBusy {
    fn drop(&mut self) {
        BUSY.store(false, atomic::Ordering::Release);
    }
}

/// Runs `agy` with `args`, returns its stdout, or `None` on failure or timeout.
fn run(exe: &std::path::Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    platform::no_console(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.take(MAX_OUTPUT).read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    let out = rx.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    out
}

/// "1.5.15" (or a line that ends with it) as numbers.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let word = text.split_whitespace().rev().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut it = word.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// The windows of `agy /usage`'s answer, or `None` when it is not a successful
/// `usage` command answer. Only what the pill shows goes on to the webview.
pub fn limits_only(stdout: &str) -> Option<Value> {
    let answer = stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).find(|v| {
        v.get("status").and_then(Value::as_str) == Some("SUCCESS")
            && v.pointer("/command/name").and_then(Value::as_str) == Some("usage")
    })?;
    let groups = answer.pointer("/command/data/groups")?.as_array()?;
    let groups: Vec<Value> = groups
        .iter()
        .map(|g| {
            let buckets: Vec<Value> = g
                .get("buckets")
                .and_then(Value::as_array)
                .map(|bs| {
                    bs.iter()
                        .map(|b| {
                            json!({
                                "window": b.get("window"),
                                "remaining_fraction": b.get("remaining_fraction"),
                                "reset_time": b.get("reset_time"),
                                "disabled": b.get("disabled"),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({ "buckets": buckets })
        })
        .collect();
    Some(json!({ "groups": groups }))
}

/// Antigravity's limits, or `None` when `agy` is missing, too old, signed out,
/// slow, already being asked, or Coucou is paused. Blocking: call it off the main thread.
pub fn read() -> Option<Value> {
    if crate::integrations::PAUSED.load(atomic::Ordering::Relaxed) {
        return None;
    }
    if BUSY.swap(true, atomic::Ordering::AcqRel) {
        return None;
    }
    let _busy = NotBusy;

    let exe = platform::cli_candidates("agy").into_iter().next()?;
    if parse_version(&run(&exe, &["--version"], VERSION_TIMEOUT)?)? < MIN_VERSION {
        return None;
    }
    limits_only(&run(&exe, &["-p", "/usage", "--output-format", "json", "--print-timeout", "20s"], TIMEOUT)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"status":"SUCCESS","response":"secret text","command":{"name":"usage","data":{"description":"x","groups":[{"name":"Gemini","buckets":[{"id":"a","window":"weekly","remaining_fraction":0,"reset_time":"2026-11-28T09:20:13Z"},{"window":"5h","disabled":true,"remaining_fraction":1}]}]}}}"#;

    #[test]
    fn version_is_read_and_compared() {
        assert_eq!(parse_version("1.5.15\n"), Some((1, 5, 15)));
        assert_eq!(parse_version("agy version 1.1.9"), Some((1, 1, 9)));
        assert!(parse_version("1.1.9").unwrap() < MIN_VERSION);
        assert!(parse_version("1.10.0").unwrap() >= MIN_VERSION);
        assert_eq!(parse_version("no number"), None);
    }

    #[test]
    fn keeps_only_the_windows() {
        let v = limits_only(ANSWER).unwrap();
        let b = &v["groups"][0]["buckets"];
        assert_eq!(b[0]["window"], "weekly");
        assert_eq!(b[1]["disabled"], true);
        assert!(!v.to_string().contains("secret"));
    }

    #[test]
    fn a_prompt_answer_is_not_usage() {
        assert!(limits_only(r#"{"status":"SUCCESS","response":"hi","num_turns":1}"#).is_none());
        assert!(limits_only("not json").is_none());
    }
}
