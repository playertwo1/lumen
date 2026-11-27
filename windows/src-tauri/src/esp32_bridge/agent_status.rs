//! Adaptado de VibePulse (https://github.com/niclasvestlund-YT/vibepulse), MIT, (c) 2026 Niclas Vestlund

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Limite máximo de leitura da cauda de cada arquivo JSONL (~64 KiB).
pub const TAIL_MAX_BYTES: usize = 64 * 1024;

/// Limite de tarefas rastreadas internamente por provedor (memória).
pub const TRACKED_JOB_LIMIT: usize = 16;

/// Limite de jobs públicos incluídos no snapshot por provedor.
pub const PUBLIC_JOB_LIMIT: usize = 4;

/// Prazo de expiração de atividade em execução (120 s).
pub const WORKING_LEASE_MS: i64 = 120_000;

/// Prazo de expiração de espera ou erro (2 h).
pub const WAITING_LEASE_MS: i64 = 7_200_000;

/// Prioridade de exibição dos estados no snapshot.
pub fn state_priority(state: &str) -> i32 {
    match state {
        "waiting" => 5,
        "error" => 4,
        "working" => 3,
        "done" => 2,
        "idle" => 1,
        _ => 0, // "unknown" ou desconhecido
    }
}

/// Job público de agente no snapshot v2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentJob {
    pub task_id: String,
    pub state: String,
    pub project: Option<String>,
    pub activity: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub updated_ms: u64,
}

/// Status de um provedor (claude ou codex) no snapshot v2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderStatus {
    pub active_count: usize,
    pub jobs: Vec<AgentJob>,
}

/// Evento classificado a partir de um registro JSONL.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub state: String,
    pub activity: Option<String>,
    pub task_id: String,
    pub project: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub order_at_ms: i64,
}

/// Registro mantido no armazenamento em memória.
#[derive(Debug, Clone)]
struct StoredJob {
    task_id: String,
    state: String,
    project: Option<String>,
    activity: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    order_at_ms: i64,
}

/// Armazenamento interno de um provedor.
#[derive(Debug, Clone, Default)]
struct ProviderStore {
    jobs: HashMap<String, StoredJob>,
}

impl ProviderStore {
    fn apply(&mut self, event: Event) {
        if let Some(existing) = self.jobs.get_mut(&event.task_id) {
            if event.order_at_ms < existing.order_at_ms {
                return;
            }
            existing.state = event.state;
            existing.activity = event.activity;
            existing.order_at_ms = event.order_at_ms;
            if event.model.is_some() {
                existing.model = event.model;
            }
            if event.effort.is_some() {
                existing.effort = event.effort;
            }
            if event.project.is_some() {
                existing.project = event.project;
            }
        } else {
            self.jobs.insert(
                event.task_id.clone(),
                StoredJob {
                    task_id: event.task_id,
                    state: event.state,
                    project: event.project,
                    activity: event.activity,
                    model: event.model,
                    effort: event.effort,
                    order_at_ms: event.order_at_ms,
                },
            );
        }

        if self.jobs.len() > TRACKED_JOB_LIMIT {
            let evict_id = self
                .jobs
                .values()
                .min_by(|a, b| {
                    let pa = state_priority(
                        &effective_state(
                            &a.state,
                            a.activity.as_deref(),
                            a.order_at_ms,
                            event.order_at_ms,
                        )
                        .0,
                    );
                    let pb = state_priority(
                        &effective_state(
                            &b.state,
                            b.activity.as_deref(),
                            b.order_at_ms,
                            event.order_at_ms,
                        )
                        .0,
                    );
                    pa.cmp(&pb)
                        .then_with(|| a.order_at_ms.cmp(&b.order_at_ms))
                        .then_with(|| a.task_id.cmp(&b.task_id))
                })
                .map(|j| j.task_id.clone());

            if let Some(id) = evict_id {
                self.jobs.remove(&id);
            }
        }
    }

    fn snapshot(&self, now_ms: i64) -> ProviderStatus {
        let mut public_jobs: Vec<AgentJob> = Vec::new();

        for job in self.jobs.values() {
            let (eff_state, eff_activity) = effective_state(
                &job.state,
                job.activity.as_deref(),
                job.order_at_ms,
                now_ms,
            );

            if eff_state == "idle" || eff_state == "unknown" {
                continue;
            }

            let age_ms = (now_ms - job.order_at_ms).max(0);
            let updated_ms = (age_ms as u64).min(u32::MAX as u64);

            public_jobs.push(AgentJob {
                task_id: job.task_id.clone(),
                state: eff_state,
                project: job.project.clone(),
                activity: eff_activity,
                model: job.model.clone(),
                effort: job.effort.clone(),
                updated_ms,
            });
        }

        let active_count = public_jobs
            .iter()
            .filter(|j| {
                j.state == "working"
                    || ((j.state == "waiting" || j.state == "error") && j.updated_ms <= 120_000)
            })
            .count();

        public_jobs.sort_by(|a, b| {
            let pa = state_priority(&a.state);
            let pb = state_priority(&b.state);
            pb.cmp(&pa)
                .then_with(|| a.updated_ms.cmp(&b.updated_ms))
                .then_with(|| a.task_id.cmp(&b.task_id))
        });

        if public_jobs.len() > PUBLIC_JOB_LIMIT {
            public_jobs.truncate(PUBLIC_JOB_LIMIT);
        }

        ProviderStatus {
            active_count,
            jobs: public_jobs,
        }
    }
}

/// Monitor de status de agentes para a ponte ESP32 / MQTT.
pub struct AgentStatusMonitor {
    seq: AtomicU64,
    claude_store: Mutex<ProviderStore>,
    codex_store: Mutex<ProviderStore>,
}

impl Default for AgentStatusMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentStatusMonitor {
    pub fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            claude_store: Mutex::new(ProviderStore::default()),
            codex_store: Mutex::new(ProviderStore::default()),
        }
    }

    /// Retorna o snapshot JSON v2 dos agentes lendo as caudas dos arquivos em disco.
    pub fn snapshot(&self, claude_dir: &Path, codex_dir: &Path, now_ms: i64) -> Value {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;

        let claude_events = scan_claude_dir(claude_dir, now_ms);
        let codex_events = scan_codex_dir(codex_dir, now_ms);

        let claude_status = {
            let mut store = self.claude_store.lock().unwrap();
            for ev in claude_events {
                store.apply(ev);
            }
            store.snapshot(now_ms)
        };

        let codex_status = {
            let mut store = self.codex_store.lock().unwrap();
            for ev in codex_events {
                store.apply(ev);
            }
            store.snapshot(now_ms)
        };

        json!({
            "v": 2,
            "seq": seq,
            "agents": {
                "claude": claude_status,
                "codex": codex_status
            }
        })
    }

    /// Permite alimentar diretamente um evento externo (ex: vindo do pipe.rs / hooks).
    pub fn apply_external_event(&self, provider: &str, event: Event) {
        match provider {
            "claude" => {
                let mut store = self.claude_store.lock().unwrap();
                store.apply(event);
            }
            "codex" => {
                let mut store = self.codex_store.lock().unwrap();
                store.apply(event);
            }
            _ => {}
        }
    }
}

/// Diretório padrão de projetos do Claude Code (`~/.claude/projects`).
pub fn default_claude_projects_dir() -> PathBuf {
    crate::platform::home_dir().join(".claude").join("projects")
}

/// Diretório padrão de sessões do Codex (`~/.codex/sessions`).
pub fn default_codex_sessions_dir() -> PathBuf {
    crate::platform::home_dir().join(".codex").join("sessions")
}

/// Calcula o estado efetivo considerando os prazos de expiração.
pub fn effective_state(
    state: &str,
    activity: Option<&str>,
    order_at_ms: i64,
    now_ms: i64,
) -> (String, Option<String>) {
    let age_ms = (now_ms - order_at_ms).max(0);
    if state == "working" && age_ms > WORKING_LEASE_MS {
        ("unknown".to_string(), None)
    } else if (state == "waiting" || state == "error") && age_ms > WAITING_LEASE_MS {
        ("unknown".to_string(), None)
    } else {
        (state.to_string(), activity.map(|s| s.to_string()))
    }
}

/// Lê no máximo ~64 KiB do final do arquivo via SeekFrom::End.
pub fn read_file_tail(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len <= TAIL_MAX_BYTES as u64 {
        let mut buf = Vec::with_capacity(len as usize);
        file.read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        file.seek(SeekFrom::End(-(TAIL_MAX_BYTES as i64)))?;
        let mut buf = vec![0u8; TAIL_MAX_BYTES];
        file.read_exact(&mut buf)?;
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            Ok(buf[pos + 1..].to_vec())
        } else {
            Ok(buf)
        }
    }
}

/// Varre o diretório de projetos do Claude Code (`**/*.jsonl`), até 16 arquivos mais recentes por mtime.
pub fn scan_claude_dir(claude_dir: &Path, now_ms: i64) -> Vec<Event> {
    let mut files = Vec::new();
    collect_files_matching(
        claude_dir,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"),
        &mut files,
    );
    files.sort_by(|a, b| b.0.cmp(&a.0));
    if files.len() > TRACKED_JOB_LIMIT {
        files.truncate(TRACKED_JOB_LIMIT);
    }
    // Processa do mais antigo para o mais recente para preservar ordem cronológica
    files.reverse();

    let mut events = Vec::new();
    for (mtime, path) in files {
        let file_mtime_ms = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as i64)
            .unwrap_or(now_ms);

        if let Ok(bytes) = read_file_tail(&path) {
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(ev) = classify_claude(&val, file_mtime_ms, now_ms) {
                        events.push(ev);
                    }
                }
            }
        }
    }
    events
}

/// Varre o diretório de sessões do Codex (`**/rollout-*.jsonl`), até 16 arquivos mais recentes por mtime.
pub fn scan_codex_dir(codex_dir: &Path, now_ms: i64) -> Vec<Event> {
    let mut files = Vec::new();
    collect_files_matching(
        codex_dir,
        &|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            name.starts_with("rollout-") && name.ends_with(".jsonl")
        },
        &mut files,
    );
    files.sort_by(|a, b| b.0.cmp(&a.0));
    if files.len() > TRACKED_JOB_LIMIT {
        files.truncate(TRACKED_JOB_LIMIT);
    }
    files.reverse();

    let mut events = Vec::new();
    for (mtime, path) in files {
        let file_mtime_ms = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as i64)
            .unwrap_or(now_ms);

        if let Ok(bytes) = read_file_tail(&path) {
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(ev) = classify_codex(&val, file_mtime_ms, now_ms) {
                        events.push(ev);
                    }
                }
            }
        }
    }
    events
}

fn collect_files_matching<F>(dir: &Path, filter: &F, files: &mut Vec<(std::time::SystemTime, PathBuf)>)
where
    F: Fn(&Path) -> bool,
{
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_matching(&path, filter, files);
        } else if filter(&path) {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                files.push((mtime, path));
            }
        }
    }
}

/// Classifica um registro do Claude Code seguindo as regras do VibePulse.
pub fn classify_claude(val: &Value, file_mtime_ms: i64, now_ms: i64) -> Option<Event> {
    let entry_type = val.get("type").and_then(|v| v.as_str())?;

    let session_id = val.get("sessionId").and_then(|v| v.as_str())?;
    if session_id.is_empty() {
        return None;
    }
    let uuid = val.get("uuid").and_then(|v| v.as_str())?;
    if uuid.is_empty() {
        return None;
    }

    let task_id = sha256_hex(session_id);
    let project = val.get("cwd").and_then(|v| v.as_str()).and_then(clean_project);

    let record_ts = val
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(parse_iso_timestamp_ms);
    let mut order_at_ms = match record_ts {
        Some(ts) if ts <= now_ms => ts,
        _ => file_mtime_ms,
    };
    if order_at_ms > now_ms {
        order_at_ms = now_ms;
    }

    let message = val.get("message");
    let model = message
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
        .and_then(normalize_model);

    let mut effort = message
        .and_then(|m| m.get("effort"))
        .and_then(|v| v.as_str())
        .and_then(normalize_effort);
    if effort.is_none() {
        effort = val
            .get("effort")
            .and_then(|v| v.as_str())
            .and_then(normalize_effort);
    }

    let (state, activity) = match entry_type {
        "user" => ("working".to_string(), Some("thinking".to_string())),
        "result" => {
            let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
            if has_explicit_error(val) || matches!(subtype, "error" | "failure" | "failed") {
                ("error".to_string(), None)
            } else if subtype == "success" {
                ("done".to_string(), None)
            } else {
                return None;
            }
        }
        "system" => {
            let subtype = val
                .get("subtype")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let status = val
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if subtype.contains("permission") || status.contains("permission") {
                ("waiting".to_string(), Some("waiting_approval".to_string()))
            } else {
                return None;
            }
        }
        "assistant" => {
            let mut tool_classifications = Vec::new();
            if let Some(content) = message.and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
                for item in content {
                    if item.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                        if let Some(cls) = classify_claude_tool(item) {
                            tool_classifications.push(cls);
                        }
                    }
                }
            }

            if tool_classifications
                .iter()
                .any(|(s, a)| s == "waiting" && a.as_deref() == Some("waiting_input"))
            {
                ("waiting".to_string(), Some("waiting_input".to_string()))
            } else if tool_classifications
                .iter()
                .any(|(s, a)| s == "waiting" && a.as_deref() == Some("waiting_approval"))
            {
                ("waiting".to_string(), Some("waiting_approval".to_string()))
            } else if let Some((last_s, last_a)) = tool_classifications.pop() {
                (last_s, last_a)
            } else if message
                .and_then(|m| m.get("stop_reason"))
                .and_then(|v| v.as_str())
                == Some("end_turn")
            {
                ("waiting".to_string(), None)
            } else if message
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter().any(|item| {
                        matches!(
                            item.get("type").and_then(|v| v.as_str()),
                            Some("thinking" | "text")
                        )
                    })
                })
                .unwrap_or(false)
            {
                ("working".to_string(), Some("thinking".to_string()))
            } else {
                return None;
            }
        }
        _ => return None,
    };

    Some(Event {
        state,
        activity,
        task_id,
        project,
        model,
        effort,
        order_at_ms,
    })
}

fn classify_claude_tool(tool: &Value) -> Option<(String, Option<String>)> {
    let name = tool.get("name").and_then(|v| v.as_str())?;
    let lower = name.to_ascii_lowercase();

    if lower == "askuserquestion" {
        return Some(("waiting".to_string(), Some("waiting_input".to_string())));
    }
    if lower.contains("permission") {
        return Some(("waiting".to_string(), Some("waiting_approval".to_string())));
    }
    if matches!(lower.as_str(), "edit" | "write" | "apply_patch") {
        return Some(("working".to_string(), Some("editing".to_string())));
    }
    if lower == "read" {
        return Some(("working".to_string(), Some("reading".to_string())));
    }
    if matches!(lower.as_str(), "glob" | "grep" | "websearch" | "web_search") {
        return Some(("working".to_string(), Some("searching".to_string())));
    }
    if matches!(lower.as_str(), "bash" | "shell" | "exec" | "exec_command") {
        let command = tool
            .get("input")
            .and_then(|i| i.get("command"))
            .and_then(|v| v.as_str());
        if let Some(cmd) = command {
            if is_test_command(cmd) {
                return Some(("working".to_string(), Some("testing".to_string())));
            }
            if is_build_command(cmd) {
                return Some(("working".to_string(), Some("building".to_string())));
            }
        }
        return Some(("working".to_string(), Some("running".to_string())));
    }
    None
}

/// Classifica um registro do Codex seguindo as regras do VibePulse.
pub fn classify_codex(val: &Value, file_mtime_ms: i64, now_ms: i64) -> Option<Event> {
    let payload = val.get("payload")?;
    if !payload.is_object() {
        return None;
    }

    let is_complete = payload.get("type").and_then(|v| v.as_str()) == Some("task_complete");
    let record_ts = val
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(parse_iso_timestamp_ms)
        .or_else(|| {
            let primary = if is_complete { "completed_at" } else { "started_at" };
            let secondary = if is_complete { "started_at" } else { "completed_at" };
            payload
                .get(primary)
                .and_then(|v| v.as_str())
                .and_then(parse_iso_timestamp_ms)
                .or_else(|| {
                    payload
                        .get(secondary)
                        .and_then(|v| v.as_str())
                        .and_then(parse_iso_timestamp_ms)
                })
        });

    let mut order_at_ms = match record_ts {
        Some(ts) if ts <= now_ms => ts,
        _ => file_mtime_ms,
    };
    if order_at_ms > now_ms {
        order_at_ms = now_ms;
    }

    match val.get("type").and_then(|v| v.as_str()) {
        Some("turn_context") => {
            let turn_id = payload.get("turn_id").and_then(|v| v.as_str())?;
            if turn_id.is_empty() {
                return None;
            }
            let task_id = bounded_task_id(turn_id);
            let project = payload.get("cwd").and_then(|v| v.as_str()).and_then(clean_project);
            let model = payload.get("model").and_then(|v| v.as_str()).and_then(normalize_model);
            let effort = payload
                .get("effort")
                .and_then(|v| v.as_str())
                .and_then(normalize_effort);

            Some(Event {
                state: "working".to_string(),
                activity: Some("thinking".to_string()),
                task_id,
                project,
                model,
                effort,
                order_at_ms,
            })
        }
        Some("response_item") => {
            let source_id = payload.get("id").and_then(|v| v.as_str())?;
            if source_id.is_empty() {
                return None;
            }
            let activity = codex_response_activity(payload)?;
            let turn_id = payload
                .get("internal_chat_message_metadata_passthrough")
                .and_then(|m| m.get("turn_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(source_id);
            let task_id = bounded_task_id(turn_id);

            Some(Event {
                state: "working".to_string(),
                activity: Some(activity),
                task_id,
                project: None,
                model: None,
                effort: None,
                order_at_ms,
            })
        }
        Some("event_msg") => {
            let kind = payload.get("type").and_then(|v| v.as_str())?;
            let turn_id = payload.get("turn_id").and_then(|v| v.as_str())?;
            if turn_id.is_empty() {
                return None;
            }
            let task_id = bounded_task_id(turn_id);

            if kind == "task_started" {
                Some(Event {
                    state: "working".to_string(),
                    activity: Some("thinking".to_string()),
                    task_id,
                    project: None,
                    model: None,
                    effort: None,
                    order_at_ms,
                })
            } else if kind == "task_complete" {
                let state = if has_explicit_error(payload) {
                    "error".to_string()
                } else {
                    "done".to_string()
                };
                Some(Event {
                    state,
                    activity: None,
                    task_id,
                    project: None,
                    model: None,
                    effort: None,
                    order_at_ms,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn codex_response_activity(payload: &Value) -> Option<String> {
    let kind = payload.get("type").and_then(|v| v.as_str())?;
    if matches!(
        kind,
        "reasoning" | "function_call_output" | "custom_tool_call_output"
    ) {
        return Some("thinking".to_string());
    }
    if !matches!(kind, "function_call" | "custom_tool_call") {
        return None;
    }
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let lower = name.to_ascii_lowercase();
    if lower.is_empty() {
        return Some("running".to_string());
    }
    if matches!(lower.as_str(), "apply_patch" | "edit" | "write") {
        Some("editing".to_string())
    } else if matches!(lower.as_str(), "read" | "view_image") {
        Some("reading".to_string())
    } else if lower.contains("search") || matches!(lower.as_str(), "web" | "web__run" | "find") {
        Some("searching".to_string())
    } else {
        Some("running".to_string())
    }
}

fn has_explicit_error(val: &Value) -> bool {
    if val.get("is_error").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if let Some(err) = val.get("error") {
        if !err.is_null() {
            return true;
        }
    }
    false
}

/// Sanitiza o nome do projeto usando exclusivamente o basename via tokens_agents.
fn clean_project(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let sanitized = super::tokens_agents::sanitize_project(s);
    if sanitized.is_empty() || sanitized == "unknown" {
        None
    } else {
        Some(sanitized)
    }
}

pub fn bounded_task_id(raw: &str) -> String {
    let has_controls = raw.chars().any(|c| c.is_control());
    if raw.len() <= 64 && !has_controls {
        raw.to_string()
    } else {
        sha256_hex(raw)
    }
}

pub fn sha256_hex(input: &str) -> String {
    let d = digest(&SHA256, input.as_bytes());
    let mut out = String::with_capacity(64);
    for b in d.as_ref() {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

fn bounded_display(val: &str, max_bytes: usize) -> Option<String> {
    let clean: String = val.chars().filter(|c| !c.is_control()).collect();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut byte_count = 0;
    let mut end_idx = 0;
    for (idx, ch) in trimmed.char_indices() {
        let ch_len = ch.len_utf8();
        if byte_count + ch_len > max_bytes {
            break;
        }
        byte_count += ch_len;
        end_idx = idx + ch_len;
    }
    if end_idx == 0 {
        None
    } else {
        Some(trimmed[..end_idx].to_string())
    }
}

pub fn normalize_model(val: &str) -> Option<String> {
    let raw = bounded_display(val, 64)?;
    let lower = raw.to_ascii_lowercase();
    let label = match lower.as_str() {
        "claude-fable-5" => "FABLE 5".to_string(),
        "claude-opus-5" => "OPUS 5".to_string(),
        "claude-sonnet-5" => "SONNET 5".to_string(),
        "gpt-5.6-luna" => "GPT-5.6 LUNA".to_string(),
        "gpt-5.6-sol" => "GPT-5.6 SOL".to_string(),
        "gpt-5.6-terra" => "GPT-5.6 TERRA".to_string(),
        _ => derive_model_label(&raw),
    };
    bounded_display(&label, 24)
}

pub fn normalize_effort(val: &str) -> Option<String> {
    bounded_display(val, 12).map(|s| s.to_ascii_uppercase())
}

fn is_version_token(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn strip_dated_suffix(s: &str) -> String {
    let tokens: Vec<&str> = s.split('-').filter(|t| !t.is_empty()).collect();
    let mut out: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // Detecta sufixo de 3 partes: YYYY-MM-DD
        if i + 2 < tokens.len() {
            let t0 = tokens[i];
            let t1 = tokens[i + 1];
            let t2 = tokens[i + 2];
            if t0.len() == 4
                && t0.starts_with("20")
                && t0.chars().all(|c| c.is_ascii_digit())
                && t1.len() == 2
                && t1.chars().all(|c| c.is_ascii_digit())
                && t2.len() == 2
                && t2.chars().all(|c| c.is_ascii_digit())
            {
                i += 3;
                continue;
            }
        }
        let t = tokens[i];
        // 8 dígitos começando com 20 (ex: 20250219, 20251001)
        if t.len() == 8 && t.starts_with("20") && t.chars().all(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        // 4 dígitos MMDD (ex: 0613, 0125, 1106)
        if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        out.push(t);
        i += 1;
    }
    out.join("-")
}

pub fn derive_model_label(model_id: &str) -> String {
    let trimmed = model_id.trim();
    let mut base = trimmed.to_ascii_lowercase();
    if base.starts_with("ft:") {
        base = base[3..].split(':').next().unwrap_or("").to_string();
    }
    base = strip_dated_suffix(&base);
    let tokens: Vec<&str> = base.split('-').filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return trimmed.to_ascii_uppercase();
    }
    if tokens[0] == "claude" && tokens.len() > 1 {
        let names: Vec<String> = tokens[1..]
            .iter()
            .filter(|&&t| !is_version_token(t))
            .map(|t| t.to_ascii_uppercase())
            .collect();
        let version_parts: Vec<&str> = tokens[1..]
            .iter()
            .copied()
            .filter(|&t| is_version_token(t))
            .collect();
        let version = version_parts.join(".");
        let label = names.join(" ");
        if !version.is_empty() {
            if label.is_empty() {
                version
            } else {
                format!("{label} {version}").trim().to_string()
            }
        } else {
            label
        }
    } else if tokens[0] == "gpt" && tokens.len() > 1 {
        let head = format!("GPT-{}", tokens[1].to_ascii_uppercase());
        let tail: Vec<String> = tokens[2..].iter().map(|t| t.to_ascii_uppercase()).collect();
        if tail.is_empty() {
            head
        } else {
            format!("{head} {}", tail.join(" "))
        }
    } else {
        tokens
            .iter()
            .map(|t| t.to_ascii_uppercase())
            .collect::<Vec<String>>()
            .join(" ")
    }
}

pub fn is_test_command(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    for segment in split_command_segments(&lower) {
        let trimmed = segment.trim();
        for pattern in &[
            "./test/run.sh",
            "pytest",
            "python -m unittest",
            "python3 -m unittest",
            "npm test",
            "npm run test",
            "cargo test",
            "go test",
            "ctest",
        ] {
            if match_command_word(trimmed, pattern) {
                return true;
            }
        }
    }
    false
}

pub fn is_build_command(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    for segment in split_command_segments(&lower) {
        let trimmed = segment.trim();
        for pattern in &[
            "cmake --build",
            "ninja",
            "make",
            "cargo build",
            "npm run build",
            "idf.py build",
        ] {
            if match_command_word(trimmed, pattern) {
                return true;
            }
        }
    }
    false
}

fn split_command_segments(cmd: &str) -> Vec<&str> {
    cmd.split(|c| c == ';' || c == '&' || c == '|' || c == '\n' || c == '\r')
        .collect()
}

fn match_command_word(segment: &str, pattern: &str) -> bool {
    if segment == pattern {
        return true;
    }
    if segment.starts_with(pattern) {
        let rest = &segment[pattern.len()..];
        if rest.starts_with(char::is_whitespace) {
            return true;
        }
    }
    let needle = format!(" {pattern}");
    if let Some(pos) = segment.find(&needle) {
        let rest = &segment[pos + needle.len()..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return true;
        }
    }
    false
}

/// Converte timestamp ISO-8601 (`YYYY-MM-DDTHH:MM:SS...`) em ms Unix UTC.
pub fn parse_iso_timestamp_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    let hh: i64 = s.get(11..13)?.parse().ok()?;
    let mm: i64 = s.get(14..16)?.parse().ok()?;
    let ss: i64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let mut secs = days * 86_400 + hh * 3600 + mm * 60 + ss;

    let mut rest = &s[19..];
    let mut frac_ms: i64 = 0;
    if rest.starts_with('.') {
        rest = &rest[1..];
        let num_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let frac_str = &rest[..num_end];
        rest = &rest[num_end..];
        if !frac_str.is_empty() {
            let padded = format!("{:0<3}", frac_str);
            frac_ms = padded[..3].parse().unwrap_or(0);
        }
    }

    rest = rest.trim();
    if !rest.is_empty() && rest != "Z" && rest != "z" {
        let sign = if rest.starts_with('+') {
            1
        } else if rest.starts_with('-') {
            -1
        } else {
            0
        };
        if sign != 0 {
            let offset_str = &rest[1..];
            let (off_h, off_m): (i64, i64) = if let Some(colon) = offset_str.find(':') {
                (
                    offset_str[..colon].parse().unwrap_or(0),
                    offset_str[colon + 1..].parse().unwrap_or(0),
                )
            } else if offset_str.len() >= 4 {
                (
                    offset_str[..2].parse().unwrap_or(0),
                    offset_str[2..4].parse().unwrap_or(0),
                )
            } else if offset_str.len() == 2 {
                (offset_str.parse().unwrap_or(0), 0)
            } else {
                (0, 0)
            };
            let offset_secs = sign * (off_h * 3600 + off_m * 60);
            secs -= offset_secs;
        }
    }

    Some(secs * 1000 + frac_ms)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m_i = m as i64;
    let doy = (153 * (m_i + if m_i > 2 { -3 } else { 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTestDir(PathBuf);
    impl TempTestDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "lumen-test-{}-{}",
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::create_dir_all(&p);
            Self(p)
        }
    }
    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_normalize_model_and_derive_label() {
        assert_eq!(normalize_model("claude-fable-5").as_deref(), Some("FABLE 5"));
        assert_eq!(normalize_model("claude-opus-5").as_deref(), Some("OPUS 5"));
        assert_eq!(normalize_model("claude-sonnet-5").as_deref(), Some("SONNET 5"));
        assert_eq!(normalize_model("gpt-5.6-luna").as_deref(), Some("GPT-5.6 LUNA"));
        assert_eq!(normalize_model("gpt-5.6-sol").as_deref(), Some("GPT-5.6 SOL"));
        assert_eq!(normalize_model("gpt-5.6-terra").as_deref(), Some("GPT-5.6 TERRA"));

        assert_eq!(normalize_model("claude-opus-4-8").as_deref(), Some("OPUS 4.8"));
        assert_eq!(
            normalize_model("claude-haiku-4-5-20251001").as_deref(),
            Some("HAIKU 4.5")
        );
        assert_eq!(
            normalize_model("claude-3-7-sonnet-20250219").as_deref(),
            Some("SONNET 3.7")
        );
        assert_eq!(
            normalize_model("claude-sonnet-4-20250514").as_deref(),
            Some("SONNET 4")
        );
        assert_eq!(
            normalize_model("claude-mythos-preview").as_deref(),
            Some("MYTHOS PREVIEW")
        );
        assert_eq!(normalize_model("gpt-5.4-mini").as_deref(), Some("GPT-5.4 MINI"));
        assert_eq!(normalize_model("gpt-4o").as_deref(), Some("GPT-4O"));
        assert_eq!(normalize_model("o4-mini").as_deref(), Some("O4 MINI"));
        assert_eq!(
            normalize_model("codex-mini-latest").as_deref(),
            Some("CODEX MINI LATEST")
        );
        assert_eq!(
            normalize_model("ft:gpt-4o-2024-08-06:acme:support-bot:abc123").as_deref(),
            Some("GPT-4O")
        );
        assert_eq!(
            normalize_model("gpt-4-0125-preview").as_deref(),
            Some("GPT-4 PREVIEW")
        );
    }

    #[test]
    fn test_normalize_effort() {
        assert_eq!(normalize_effort("xhigh").as_deref(), Some("XHIGH"));
        assert_eq!(normalize_effort("high").as_deref(), Some("HIGH"));
        assert_eq!(normalize_effort("low").as_deref(), Some("LOW"));
        assert_eq!(normalize_effort("").as_deref(), None);
        assert_eq!(normalize_effort("  \n\t ").as_deref(), None);
    }

    #[test]
    fn test_claude_jsonl_all_states() {
        let claude_dir = TempTestDir::new("claude-all-states");
        let codex_dir = TempTestDir::new("codex-empty");
        let file_path = claude_dir.0.join("session.jsonl");

        let lines = vec![
            // 1. user -> working (thinking)
            json!({
                "type": "user",
                "sessionId": "sess-claude-1",
                "uuid": "u1",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:00Z"
            }),
            // 2. tool_use read -> working (reading)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u2",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:01Z",
                "message": {
                    "model": "claude-fable-5",
                    "content": [{"type": "tool_use", "name": "read", "input": {"path": "src/main.rs"}}]
                },
                "effort": "xhigh"
            }),
            // 3. tool_use edit -> working (editing)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u3",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:02Z",
                "message": {
                    "content": [{"type": "tool_use", "name": "edit", "input": {"path": "src/main.rs"}}]
                }
            }),
            // 4. tool_use search -> working (searching)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u4",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:03Z",
                "message": {
                    "content": [{"type": "tool_use", "name": "grep", "input": {"pattern": "foo"}}]
                }
            }),
            // 5. tool_use bash cargo test -> working (testing)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u5",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:04Z",
                "message": {
                    "content": [{"type": "tool_use", "name": "bash", "input": {"command": "cargo test --lib"}}]
                }
            }),
            // 6. tool_use bash cargo build -> working (building)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u6",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:05Z",
                "message": {
                    "content": [{"type": "tool_use", "name": "bash", "input": {"command": "cargo build"}}]
                }
            }),
            // 7. tool_use bash running -> working (running)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u7",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:06Z",
                "message": {
                    "content": [{"type": "tool_use", "name": "bash", "input": {"command": "echo hello"}}]
                }
            }),
            // 8. askuserquestion -> waiting (waiting_input)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u8",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:07Z",
                "message": {
                    "content": [{"type": "tool_use", "name": "AskUserQuestion", "input": {}}]
                }
            }),
            // 9. system permission -> waiting (waiting_approval)
            json!({
                "type": "system",
                "sessionId": "sess-claude-1",
                "uuid": "u9",
                "cwd": "C:\\Users\\test\\coucou",
                "status": "permission_prompt",
                "timestamp": "2026-11-27T10:00:08Z"
            }),
            // 10. end_turn -> waiting (None)
            json!({
                "type": "assistant",
                "sessionId": "sess-claude-1",
                "uuid": "u10",
                "cwd": "C:\\Users\\test\\coucou",
                "timestamp": "2026-11-27T10:00:09Z",
                "message": {
                    "stop_reason": "end_turn",
                    "content": []
                }
            }),
            // 11. result success -> done (None)
            json!({
                "type": "result",
                "sessionId": "sess-claude-1",
                "uuid": "u11",
                "cwd": "C:\\Users\\test\\coucou",
                "subtype": "success",
                "timestamp": "2026-11-27T10:00:10Z"
            }),
        ];

        let content = lines
            .into_iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&file_path, content).unwrap();

        let monitor = AgentStatusMonitor::new();
        let base_now = parse_iso_timestamp_ms("2026-11-27T10:00:10Z").unwrap();
        let snap = monitor.snapshot(&claude_dir.0, &codex_dir.0, base_now);

        let claude = &snap["agents"]["claude"];
        assert_eq!(claude["active_count"], 0); // "done" não conta como ativo
        let jobs = claude["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["state"], "done");
        assert_eq!(jobs[0]["project"], "coucou");
        assert_eq!(jobs[0]["model"], "FABLE 5");
        assert_eq!(jobs[0]["effort"], "XHIGH");
        assert_eq!(jobs[0]["activity"], Value::Null);

        // Agora testamos resultado com erro
        let error_line = json!({
            "type": "result",
            "sessionId": "sess-claude-1",
            "uuid": "u12",
            "cwd": "C:\\Users\\test\\coucou",
            "subtype": "error",
            "timestamp": "2026-11-27T10:00:12Z"
        });
        std::fs::write(&file_path, error_line.to_string() + "\n").unwrap();
        let error_now = parse_iso_timestamp_ms("2026-11-27T10:00:12Z").unwrap();
        let snap_err = monitor.snapshot(&claude_dir.0, &codex_dir.0, error_now);
        let jobs_err = snap_err["agents"]["claude"]["jobs"].as_array().unwrap();
        assert_eq!(jobs_err[0]["state"], "error");
        assert_eq!(snap_err["agents"]["claude"]["active_count"], 1); // error recente (<=120s) conta como ativo
    }

    #[test]
    fn test_codex_jsonl_all_states() {
        let claude_dir = TempTestDir::new("claude-empty");
        let codex_dir = TempTestDir::new("codex-all-states");
        let file_path = codex_dir.0.join("rollout-2026-11-27.jsonl");

        let lines = vec![
            // 1. turn_context -> working (thinking)
            json!({
                "type": "turn_context",
                "payload": {
                    "turn_id": "turn-100",
                    "cwd": "/home/user/workspace/lumen",
                    "model": "gpt-5.6-sol",
                    "effort": "xhigh"
                },
                "timestamp": "2026-11-27T12:00:00Z"
            }),
            // 2. response_item reading
            json!({
                "type": "response_item",
                "payload": {
                    "id": "resp-1",
                    "type": "function_call",
                    "name": "read",
                    "internal_chat_message_metadata_passthrough": { "turn_id": "turn-100" }
                },
                "timestamp": "2026-11-27T12:00:01Z"
            }),
            // 3. response_item editing
            json!({
                "type": "response_item",
                "payload": {
                    "id": "resp-2",
                    "type": "function_call",
                    "name": "apply_patch",
                    "internal_chat_message_metadata_passthrough": { "turn_id": "turn-100" }
                },
                "timestamp": "2026-11-27T12:00:02Z"
            }),
            // 4. response_item searching
            json!({
                "type": "response_item",
                "payload": {
                    "id": "resp-3",
                    "type": "function_call",
                    "name": "websearch",
                    "internal_chat_message_metadata_passthrough": { "turn_id": "turn-100" }
                },
                "timestamp": "2026-11-27T12:00:03Z"
            }),
            // 5. response_item running
            json!({
                "type": "response_item",
                "payload": {
                    "id": "resp-4",
                    "type": "function_call",
                    "name": "exec_cmd",
                    "internal_chat_message_metadata_passthrough": { "turn_id": "turn-100" }
                },
                "timestamp": "2026-11-27T12:00:04Z"
            }),
            // 6. event_msg task_started
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_started",
                    "turn_id": "turn-100"
                },
                "timestamp": "2026-11-27T12:00:05Z"
            }),
            // 7. event_msg task_complete
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_complete",
                    "turn_id": "turn-100"
                },
                "timestamp": "2026-11-27T12:00:06Z"
            }),
        ];

        let content = lines
            .into_iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&file_path, content).unwrap();

        let monitor = AgentStatusMonitor::new();
        let base_now = parse_iso_timestamp_ms("2026-11-27T12:00:06Z").unwrap();
        let snap = monitor.snapshot(&claude_dir.0, &codex_dir.0, base_now);

        let codex = &snap["agents"]["codex"];
        assert_eq!(codex["active_count"], 0);
        let jobs = codex["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["task_id"], "turn-100");
        assert_eq!(jobs[0]["state"], "done");
        assert_eq!(jobs[0]["project"], "lumen");
        assert_eq!(jobs[0]["model"], "GPT-5.6 SOL");
        assert_eq!(jobs[0]["effort"], "XHIGH");

        // Testar task_complete com erro explícito
        let error_line = json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "turn_id": "turn-100",
                "is_error": true
            },
            "timestamp": "2026-11-27T12:00:07Z"
        });
        std::fs::write(&file_path, error_line.to_string() + "\n").unwrap();
        let snap_err = monitor.snapshot(
            &claude_dir.0,
            &codex_dir.0,
            parse_iso_timestamp_ms("2026-11-27T12:00:07Z").unwrap(),
        );
        assert_eq!(snap_err["agents"]["codex"]["active_count"], 1);
        assert_eq!(snap_err["agents"]["codex"]["jobs"][0]["state"], "error");
    }

    #[test]
    fn test_timeouts_and_active_count() {
        let claude_dir = TempTestDir::new("claude-timeouts");
        let codex_dir = TempTestDir::new("codex-timeouts");

        // 1. Job "working"
        let working_file = claude_dir.0.join("working.jsonl");
        let working_json = json!({
            "type": "user",
            "sessionId": "working-task",
            "uuid": "u1",
            "timestamp": "2026-11-27T00:00:00Z"
        });
        std::fs::write(&working_file, working_json.to_string() + "\n").unwrap();

        let monitor = AgentStatusMonitor::new();
        let t0 = parse_iso_timestamp_ms("2026-11-27T00:00:00Z").unwrap();

        // No tempo t0 + 120s (exatamente 120s): ainda é working, ativo
        let snap1 = monitor.snapshot(&claude_dir.0, &codex_dir.0, t0 + 120_000);
        assert_eq!(snap1["agents"]["claude"]["active_count"], 1);
        assert_eq!(snap1["agents"]["claude"]["jobs"][0]["state"], "working");
        assert_eq!(snap1["agents"]["claude"]["jobs"][0]["updated_ms"], 120_000);

        // No tempo t0 + 120s + 1ms: vira unknown e sai de jobs
        let snap2 = monitor.snapshot(&claude_dir.0, &codex_dir.0, t0 + 120_001);
        assert_eq!(snap2["agents"]["claude"]["active_count"], 0);
        assert_eq!(snap2["agents"]["claude"]["jobs"].as_array().unwrap().len(), 0);

        // 2. Job "waiting" (Claude end_turn) e "error" (Codex task_complete is_error)
        let waiting_file = claude_dir.0.join("waiting.jsonl");
        let waiting_json = json!({
            "type": "assistant",
            "sessionId": "wait-task",
            "uuid": "u-wait",
            "message": { "stop_reason": "end_turn", "content": [] },
            "timestamp": "2026-11-27T00:00:00Z"
        });
        let error_file = codex_dir.0.join("rollout-error.jsonl");
        let error_json = json!({
            "type": "event_msg",
            "payload": { "type": "task_complete", "turn_id": "err-task", "is_error": true },
            "timestamp": "2026-11-27T00:00:00Z"
        });
        std::fs::write(&waiting_file, waiting_json.to_string() + "\n").unwrap();
        std::fs::write(&error_file, error_json.to_string() + "\n").unwrap();

        // Em t0 + 100s: ambos contam como ativos (<= 120s)
        let snap_100 = monitor.snapshot(&claude_dir.0, &codex_dir.0, t0 + 100_000);
        assert_eq!(snap_100["agents"]["claude"]["jobs"][0]["state"], "waiting");
        assert_eq!(snap_100["agents"]["codex"]["active_count"], 1);

        // Em t0 + 200s: ativo = 0 (pois > 120s), mas permanecem em jobs (pois <= 2h)
        let snap_200 = monitor.snapshot(&claude_dir.0, &codex_dir.0, t0 + 200_000);
        assert_eq!(snap_200["agents"]["claude"]["active_count"], 0);
        assert_eq!(snap_200["agents"]["claude"]["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(snap_200["agents"]["codex"]["active_count"], 0);
        assert_eq!(snap_200["agents"]["codex"]["jobs"].as_array().unwrap().len(), 1);

        // Em t0 + 7200s (2h): ainda presentes
        let snap_2h = monitor.snapshot(&claude_dir.0, &codex_dir.0, t0 + 7_200_000);
        assert_eq!(snap_2h["agents"]["claude"]["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(snap_2h["agents"]["codex"]["jobs"].as_array().unwrap().len(), 1);

        // Em t0 + 7200s + 1ms (> 2h): expiram para unknown e saem de jobs
        let snap_exp = monitor.snapshot(&claude_dir.0, &codex_dir.0, t0 + 7_200_001);
        assert_eq!(snap_exp["agents"]["claude"]["jobs"].as_array().unwrap().len(), 0);
        assert_eq!(snap_exp["agents"]["codex"]["jobs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_project_sanitization() {
        assert_eq!(clean_project("C:\\Users\\foo\\coucou").as_deref(), Some("coucou"));
        assert_eq!(clean_project("/home/user/my-app/").as_deref(), Some("my-app"));
        assert_eq!(clean_project("").as_deref(), None);
        assert_eq!(clean_project("   ").as_deref(), None);
    }

    #[test]
    fn test_public_job_limit_and_seq() {
        let claude_dir = TempTestDir::new("claude-limit");
        let codex_dir = TempTestDir::new("codex-limit");

        // Cria 6 arquivos para Claude dentro da janela de 120s
        for i in 1..=6 {
            let f = claude_dir.0.join(format!("session-{i}.jsonl"));
            let l = json!({
                "type": "user",
                "sessionId": format!("sess-{i}"),
                "uuid": format!("u-{i}"),
                "timestamp": format!("2026-11-27T10:06:0{i}Z")
            });
            std::fs::write(f, l.to_string() + "\n").unwrap();
        }

        let monitor = AgentStatusMonitor::new();
        let now = parse_iso_timestamp_ms("2026-11-27T10:06:30Z").unwrap();

        let s1 = monitor.snapshot(&claude_dir.0, &codex_dir.0, now);
        assert_eq!(s1["seq"], 1);
        assert_eq!(s1["agents"]["claude"]["active_count"], 6);
        assert_eq!(s1["agents"]["claude"]["jobs"].as_array().unwrap().len(), 4); // limitado a 4 jobs públicos

        let s2 = monitor.snapshot(&claude_dir.0, &codex_dir.0, now + 1000);
        assert_eq!(s2["seq"], 2);

        let s3 = monitor.snapshot(&claude_dir.0, &codex_dir.0, now + 2000);
        assert_eq!(s3["seq"], 3);
    }

    #[test]
    fn test_seek_tail_64k() {
        let dir = TempTestDir::new("tail-test");
        let path = dir.0.join("large.jsonl");

        // Escreve mais de 80 KiB de dados
        let mut file = File::create(&path).unwrap();
        use std::io::Write;
        for i in 0..2500 {
            writeln!(file, "{{\"dummy\": \"padding-data-line-{i}\"}}").unwrap();
        }
        let target_line = json!({
            "type": "user",
            "sessionId": "tail-session-id",
            "uuid": "u-tail",
            "cwd": "/projects/test-tail",
            "timestamp": "2026-11-27T15:00:00Z"
        });
        writeln!(file, "{}", target_line.to_string()).unwrap();
        drop(file);

        assert!(path.metadata().unwrap().len() > 64 * 1024);

        let claude_dir = dir;
        let codex_dir = TempTestDir::new("codex-empty");
        let monitor = AgentStatusMonitor::new();
        let now = parse_iso_timestamp_ms("2026-11-27T15:00:00Z").unwrap();

        let snap = monitor.snapshot(&claude_dir.0, &codex_dir.0, now);
        let jobs = snap["agents"]["claude"]["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["state"], "working");
        assert_eq!(jobs[0]["project"], "test-tail");
    }

    #[test]
    fn test_effective_state_all_combinations() {
        // working
        assert_eq!(
            effective_state("working", Some("thinking"), 1000, 1000 + 120_000),
            ("working".to_string(), Some("thinking".to_string()))
        );
        assert_eq!(
            effective_state("working", Some("thinking"), 1000, 1000 + 120_001),
            ("unknown".to_string(), None)
        );

        // waiting
        assert_eq!(
            effective_state("waiting", Some("waiting_input"), 1000, 1000 + 7_200_000),
            ("waiting".to_string(), Some("waiting_input".to_string()))
        );
        assert_eq!(
            effective_state("waiting", Some("waiting_input"), 1000, 1000 + 7_200_001),
            ("unknown".to_string(), None)
        );

        // error
        assert_eq!(
            effective_state("error", None, 1000, 1000 + 7_200_000),
            ("error".to_string(), None)
        );
        assert_eq!(
            effective_state("error", None, 1000, 1000 + 7_200_001),
            ("unknown".to_string(), None)
        );

        // done
        assert_eq!(
            effective_state("done", None, 1000, 1000 + 10_000_000),
            ("done".to_string(), None)
        );
    }
}
