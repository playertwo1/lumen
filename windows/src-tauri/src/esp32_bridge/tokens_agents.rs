use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const APPROVAL_TTL_SECS: u64 = 115;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokensSnapshot {
    pub claude_session_pct: Option<f64>,
    pub claude_session_reset_min: Option<i64>,
    pub claude_week_pct: Option<f64>,
    pub claude_week_reset_min: Option<i64>,
    pub codex_session_pct: Option<f64>,
    pub codex_session_reset_min: Option<i64>,
    pub codex_week_pct: Option<f64>,
    pub codex_week_reset_min: Option<i64>,
    pub antigravity_session_pct: Option<f64>,
    pub antigravity_session_reset_min: Option<i64>,
    pub antigravity_week_pct: Option<f64>,
    pub antigravity_week_reset_min: Option<i64>,
    pub claude_stale: bool,
    pub codex_stale: bool,
    pub antigravity_stale: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ClaudeQuotaCache {
    pub session_pct: Option<f64>,
    pub session_resets_at: Option<u64>,
    pub week_pct: Option<f64>,
    pub week_resets_at: Option<u64>,
    pub updated_at: u64,
}

impl ClaudeQuotaCache {
    pub fn update_from_statusline(&mut self, rate_limits: &serde_json::Value, now_sec: u64) {
        if let Some(five_hour) = rate_limits.get("five_hour") {
            if let Some(used) = five_hour.get("used_percentage").and_then(|v| v.as_f64()) {
                self.session_pct = Some(used);
            }
            if let Some(resets) = five_hour.get("resets_at") {
                if let Some(sec) = parse_resets_at(resets) {
                    self.session_resets_at = Some(sec);
                }
            }
        }
        if let Some(seven_day) = rate_limits.get("seven_day") {
            if let Some(used) = seven_day.get("used_percentage").and_then(|v| v.as_f64()) {
                self.week_pct = Some(used);
            }
            if let Some(resets) = seven_day.get("resets_at") {
                if let Some(sec) = parse_resets_at(resets) {
                    self.week_resets_at = Some(sec);
                }
            }
        }
        self.updated_at = now_sec;
    }

    pub fn update_from_oauth(&mut self, payload: &serde_json::Value, now_sec: u64) {
        if let Some(five) = payload.get("five_hour") {
            if let Some(u) = five.get("utilization").and_then(|v| v.as_f64()) {
                self.session_pct = Some(u.clamp(0.0, 100.0).round());
            }
            if let Some(sec) = five.get("resets_at").and_then(parse_resets_at) {
                self.session_resets_at = Some(sec);
            }
        }
        if let Some(seven) = payload.get("seven_day") {
            if let Some(u) = seven.get("utilization").and_then(|v| v.as_f64()) {
                self.week_pct = Some(u.clamp(0.0, 100.0).round());
            }
            if let Some(sec) = seven.get("resets_at").and_then(parse_resets_at) {
                self.week_resets_at = Some(sec);
            }
        }
        if self.session_pct.is_some() || self.week_pct.is_some() {
            self.updated_at = now_sec;
        }
    }

    pub fn update_from_agy_limits(&mut self, limits: &serde_json::Value, now_sec: u64) {
        let Some(groups) = limits.get("groups").and_then(|v| v.as_array()) else {
            return;
        };
        let mut best_5h: Option<(f64, Option<u64>)> = None;
        let mut best_wk: Option<(f64, Option<u64>)> = None;
        for g in groups {
            let Some(buckets) = g.get("buckets").and_then(|v| v.as_array()) else {
                continue;
            };
            for b in buckets {
                if b.get("disabled").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                let win = b.get("window").and_then(|v| v.as_str()).unwrap_or("").to_ascii_lowercase();
                let Some(pct) = b
                    .get("remaining_fraction")
                    .and_then(|v| v.as_f64())
                    .map(|rem| ((1.0 - rem.clamp(0.0, 1.0)) * 100.0).round())
                else {
                    continue;
                };
                let reset = b
                    .get("reset_time")
                    .and_then(parse_resets_at)
                    .filter(|&s| s > 1_000_000_000);
                if win.contains("5h") || win.contains("session") {
                    let take = match best_5h {
                        None => true,
                        Some((p, r)) => pct > p || (pct == p && r.is_none() && reset.is_some()),
                    };
                    if take {
                        best_5h = Some((pct, reset));
                    }
                } else if win.contains("week") {
                    let take = match best_wk {
                        None => true,
                        Some((p, r)) => pct > p || (pct == p && r.is_none() && reset.is_some()),
                    };
                    if take {
                        best_wk = Some((pct, reset));
                    }
                }
            }
        }
        if let Some((pct, reset)) = best_5h {
            self.session_pct = Some(pct);
            self.session_resets_at = reset;
        }
        if let Some((pct, reset)) = best_wk {
            self.week_pct = Some(pct);
            self.week_resets_at = reset;
        }
        if best_5h.is_some() || best_wk.is_some() {
            self.updated_at = now_sec;
        }
    }

    pub fn to_fields(&self, now_sec: u64) -> (Option<f64>, Option<i64>, Option<f64>, Option<i64>, bool) {
        let stale = self.updated_at == 0 || (now_sec.saturating_sub(self.updated_at) > 3600);
        let sess_min = self.session_resets_at.map(|r| {
            if r > now_sec {
                ((r - now_sec) as i64 + 59) / 60
            } else {
                0
            }
        });
        let week_min = self.week_resets_at.map(|r| {
            if r > now_sec {
                ((r - now_sec) as i64 + 59) / 60
            } else {
                0
            }
        });
        (self.session_pct, sess_min, self.week_pct, week_min, stale)
    }
}

pub struct CodexQuotaPart {
    pub session_pct: Option<f64>,
    pub session_reset_min: Option<i64>,
    pub week_pct: Option<f64>,
    pub week_reset_min: Option<i64>,
    pub stale: bool,
}

pub fn parse_resets_at(val: &serde_json::Value) -> Option<u64> {
    if let Some(n) = val.as_u64() {
        if n > 1_000_000_000_000 {
            Some(n / 1000)
        } else if n > 0 {
            Some(n)
        } else {
            None
        }
    } else if let Some(i) = val.as_i64() {
        if i > 0 {
            let u = i as u64;
            if u > 1_000_000_000_000 {
                Some(u / 1000)
            } else {
                Some(u)
            }
        } else {
            None
        }
    } else if let Some(s) = val.as_str() {
        if let Some(ms) = crate::whatsapp::gateway::rfc3339_ms(Some(s)) {
            if ms > 0 {
                return Some((ms / 1000) as u64);
            }
        }
        // Fallback parse as u64 string
        if let Ok(n) = s.parse::<u64>() {
            if n > 1_000_000_000_000 {
                return Some(n / 1000);
            } else if n > 0 {
                return Some(n);
            }
        }
        None
    } else {
        None
    }
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<(std::time::SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                files.push((mtime, path));
            }
        }
    }
}

pub fn read_codex_rollout_quota(sessions_dir: &Path, now_sec: u64) -> CodexQuotaPart {
    let mut files = Vec::new();
    collect_jsonl_files(sessions_dir, &mut files);
    // Sort descending by mtime
    files.sort_by(|a, b| b.0.cmp(&a.0));
    if files.len() > 256 {
        files.truncate(256);
    }

    let mut session_pct = None;
    let mut session_reset_min = None;
    let mut week_pct = None;
    let mut week_reset_min = None;
    let mut found_primary = false;
    let mut found_secondary = false;

    for (_, path) in files {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = content.lines().collect();
            for line in lines.iter().rev() {
                if line.contains("rate_limits") {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        // Look for rate_limits object in payload, message, or root
                        let rl = if let Some(r) = v.pointer("/payload/rate_limits") {
                            r
                        } else if let Some(r) = v.get("rate_limits") {
                            r
                        } else if let Some(r) = v.pointer("/message/rate_limits") {
                            r
                        } else {
                            &v
                        };

                        if !found_primary {
                            if let Some(prim) = rl.get("primary").or_else(|| rl.get("5h")) {
                                if let Some(p) = prim.get("used_percent").and_then(|x| x.as_f64()) {
                                    session_pct = Some(p);
                                    found_primary = true;
                                }
                                if let Some(res) = prim.get("resets_at").and_then(parse_resets_at) {
                                    if res > now_sec {
                                        session_reset_min = Some(((res - now_sec) as i64 + 59) / 60);
                                    } else {
                                        session_reset_min = Some(0);
                                    }
                                    found_primary = true;
                                }
                            }
                        }

                        if !found_secondary {
                            if let Some(sec) = rl.get("secondary").or_else(|| rl.get("7d")) {
                                if let Some(p) = sec.get("used_percent").and_then(|x| x.as_f64()) {
                                    week_pct = Some(p);
                                    found_secondary = true;
                                }
                                if let Some(res) = sec.get("resets_at").and_then(parse_resets_at) {
                                    if res > now_sec {
                                        week_reset_min = Some(((res - now_sec) as i64 + 59) / 60);
                                    } else {
                                        week_reset_min = Some(0);
                                    }
                                    found_secondary = true;
                                }
                            }
                        }

                        if found_primary && found_secondary {
                            break;
                        }
                    }
                }
            }
        }
        if found_primary && found_secondary {
            break;
        }
    }

    let stale = !found_primary && !found_secondary;
    CodexQuotaPart {
        session_pct,
        session_reset_min,
        week_pct,
        week_reset_min,
        stale,
    }
}

pub fn read_claude_access_token(cred_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(cred_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    val.pointer("/claudeAiOauth/accessToken")
        .or_else(|| val.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn build_clean_usage_payload(snapshot: &TokensSnapshot) -> serde_json::Value {
    json!({
        "claudeSessionPct": snapshot.claude_session_pct,
        "claudeSessionResetMin": snapshot.claude_session_reset_min,
        "claudeWeekPct": snapshot.claude_week_pct,
        "claudeWeekResetMin": snapshot.claude_week_reset_min,
        "codexSessionPct": snapshot.codex_session_pct,
        "codexSessionResetMin": snapshot.codex_session_reset_min,
        "codexWeekPct": snapshot.codex_week_pct,
        "codexWeekResetMin": snapshot.codex_week_reset_min,
        "antigravitySessionPct": snapshot.antigravity_session_pct,
        "antigravitySessionResetMin": snapshot.antigravity_session_reset_min,
        "antigravityWeekPct": snapshot.antigravity_week_pct,
        "antigravityWeekResetMin": snapshot.antigravity_week_reset_min,
        "claudeStale": snapshot.claude_stale,
        "codexStale": snapshot.codex_stale,
        "antigravityStale": snapshot.antigravity_stale,
        "updatedAt": snapshot.updated_at
    })
}

pub fn sanitize_project(raw: &str) -> String {
    let normalized = raw.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..").collect();
    parts.last().map(|s| s.to_string()).unwrap_or_else(|| "unknown".to_string())
}

pub fn is_dangerous_command(tool: &str, command: &str) -> bool {
    let t = tool.to_lowercase();
    let c = command.to_lowercase();
    let combined = format!("{} {}", t, c);

    let patterns = [
        "rm ",
        "del ",
        "remove-item",
        "git push",
        "wrangler deploy",
        "publish",
        "format",
        "--force",
        "reset --hard",
    ];

    for p in &patterns {
        if combined.contains(p) {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalView {
    pub id: String,
    pub agent: String,
    pub tool: String,
    pub command: String,
    pub dangerous: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct Esp32AgentPanelState {
    pub your_turn: bool,
    pub turn_seq: u64,
    pub agent: Option<String>,
    pub project: Option<String>,
    pub pending_approval: Option<PendingApprovalView>,
    pub pipe_request_id: Option<String>,
    pub decisions: HashMap<String, String>, // id -> decision ("allow" | "deny")
    pub approval_times: HashMap<String, u64>, // id -> created_at
}

impl Default for Esp32AgentPanelState {
    fn default() -> Self {
        Self {
            your_turn: false,
            turn_seq: 0,
            agent: None,
            project: None,
            pending_approval: None,
            pipe_request_id: None,
            decisions: HashMap::new(),
            approval_times: HashMap::new(),
        }
    }
}

impl Esp32AgentPanelState {
    pub fn on_stop_or_notify(&mut self, agent: &str, cwd_or_project: &str, now_sec: u64) {
        self.turn_seq += 1;
        self.your_turn = true;
        self.agent = Some(agent.to_string());
        self.project = Some(sanitize_project(cwd_or_project));
        let _ = now_sec;
    }

    pub fn on_permission(
        &mut self,
        pipe_request_id: Option<String>,
        agent: &str,
        cwd_or_project: &str,
        tool: &str,
        command: &str,
        now_sec: u64,
    ) -> String {
        let mut rand_bytes = [0u8; 16];
        let _ = ring::rand::SystemRandom::new().fill(&mut rand_bytes);
        let mut hex_rand = String::with_capacity(32);
        for b in rand_bytes {
            use std::fmt::Write as _;
            let _ = write!(&mut hex_rand, "{b:02x}");
        }
        let id = format!("esp-app-{hex_rand}");

        let dangerous = is_dangerous_command(tool, command);
        let approval = PendingApprovalView {
            id: id.clone(),
            agent: agent.to_string(),
            tool: tool.to_string(),
            command: command.to_string(),
            dangerous,
            created_at: now_sec,
        };

        self.pending_approval = Some(approval);
        self.pipe_request_id = pipe_request_id;
        self.approval_times.insert(id.clone(), now_sec);
        self.agent = Some(agent.to_string());
        self.project = Some(sanitize_project(cwd_or_project));

        id
    }

    pub fn on_clear(&mut self) {
        self.your_turn = false;
        self.pending_approval = None;
        self.pipe_request_id = None;
    }

    pub fn dismiss_turn(&mut self) -> serde_json::Value {
        self.your_turn = false;
        json!({
            "ok": true,
            "yourTurn": false,
            "turnSeq": self.turn_seq
        })
    }

    fn prune_ttl(&mut self, now_sec: u64) {
        let mut expired = Vec::new();
        for (id, &created) in &self.approval_times {
            if now_sec.saturating_sub(created) > APPROVAL_TTL_SECS {
                expired.push(id.clone());
            }
        }
        for id in expired {
            self.approval_times.remove(&id);
            if let Some(ref p) = self.pending_approval {
                if p.id == id {
                    self.pending_approval = None;
                    self.pipe_request_id = None;
                }
            }
        }
    }

    pub fn decide(
        &mut self,
        id: &str,
        decision: &str,
        now_sec: u64,
    ) -> Result<(serde_json::Value, Option<String>), (u16, String)> {
        if decision != "allow" && decision != "deny" {
            return Err((400, "Invalid decision".to_string()));
        }

        self.prune_ttl(now_sec);

        // Check if already decided
        if let Some(existing) = self.decisions.get(id) {
            if existing == "expired" {
                return Err((409, "Approval expired".to_string()));
            }
            return Err((409, format!("Already decided: {}", existing)));
        }

        // Check if pending matches id
        let is_valid = match &self.pending_approval {
            Some(p) => p.id == id,
            None => false,
        };

        if !is_valid {
            // Check if it was in approval_times (meaning it existed once, but consumed/expired)
            if self.approval_times.contains_key(id) {
                return Err((409, "Approval already consumed or expired".to_string()));
            }
            return Err((409, "Approval not found".to_string()));
        }

        // Consume single use
        self.decisions.insert(id.to_string(), decision.to_string());
        self.pending_approval = None;
        let pipe_id = self.pipe_request_id.take();

        Ok((
            json!({
                "ok": true,
                "id": id,
                "decision": decision
            }),
            pipe_id,
        ))
    }

    pub fn get_decision(&mut self, id: &str, now_sec: u64) -> serde_json::Value {
        self.prune_ttl(now_sec);
        let status = if let Some(d) = self.decisions.get(id) {
            d.clone()
        } else if self.approval_times.contains_key(id) {
            // If it's in approval_times but no decision, it's pending
            "pending".to_string()
        } else {
            // If completely missing or expired
            "expired".to_string()
        };

        json!({
            "id": id,
            "decision": status,
            "status": status
        })
    }

    pub fn clear_by_pipe_id(&mut self, pipe_request_id: &str) {
        if self.pipe_request_id.as_deref() == Some(pipe_request_id) {
            self.pending_approval = None;
            self.pipe_request_id = None;
        }
    }

    pub fn snapshot(&mut self, now_sec: u64) -> serde_json::Value {
        self.prune_ttl(now_sec);
        json!({
            "yourTurn": self.your_turn,
            "turnSeq": self.turn_seq,
            "agent": self.agent,
            "project": self.project,
            "pendingApproval": self.pending_approval,
            "updatedAt": now_sec
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ScratchDir(PathBuf);
    impl ScratchDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("lumen-esp32-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_sanitize_project() {
        assert_eq!(sanitize_project("C:\\Users\\foo\\project"), "project");
        assert_eq!(sanitize_project("/home/user/my-app/"), "my-app");
        assert_eq!(sanitize_project("foo/bar/./../baz"), "baz");
        assert_eq!(sanitize_project(""), "unknown");
    }

    #[test]
    fn test_is_dangerous_command() {
        assert!(is_dangerous_command("bash", "rm -rf /"));
        assert!(is_dangerous_command("fs", "del file.txt"));
        assert!(is_dangerous_command("git", "git push origin main"));
        assert!(is_dangerous_command("npm", "wrangler deploy"));
        assert!(is_dangerous_command("cmd", "format c:"));
        assert!(!is_dangerous_command("cargo", "cargo build"));
    }

    #[test]
    fn test_build_clean_usage_payload() {
        let snapshot = TokensSnapshot {
            claude_session_pct: Some(15.5),
            claude_session_reset_min: Some(120),
            claude_week_pct: Some(50.0),
            claude_week_reset_min: Some(1500),
            codex_session_pct: Some(10.0),
            codex_session_reset_min: Some(60),
            codex_week_pct: Some(25.0),
            codex_week_reset_min: Some(800),
            antigravity_session_pct: Some(18.0),
            antigravity_session_reset_min: Some(280),
            antigravity_week_pct: Some(32.0),
            antigravity_week_reset_min: Some(8760),
            claude_stale: false,
            codex_stale: false,
            antigravity_stale: false,
            updated_at: 1000,
        };
        let payload = build_clean_usage_payload(&snapshot);
        let s = payload.to_string();
        assert!(s.len() <= 512);
        assert!(payload.get("claudeSessionPct").is_some());
        assert!(payload.get("antigravitySessionPct").is_some());
        assert!(payload.get("updatedAt").is_some());
    }

    #[test]
    fn test_claude_oauth_and_agy_limits() {
        let mut c = ClaudeQuotaCache::default();
        c.update_from_oauth(
            &json!({
                "five_hour": { "utilization": 12.4, "resets_at": "2026-11-27T16:00:00Z" },
                "seven_day": { "utilization": 81.2, "resets_at": "2026-11-29T16:00:00Z" }
            }),
            1795788000,
        );
        assert_eq!(c.session_pct, Some(12.0));
        assert_eq!(c.week_pct, Some(81.0));

        let mut a = ClaudeQuotaCache::default();
        a.update_from_agy_limits(
            &json!({
                "groups": [{
                    "buckets": [
                        { "window": "5h", "remaining_fraction": 0.82, "reset_time": "2026-11-27T16:00:00Z" },
                        { "window": "weekly", "remaining_fraction": 0.68, "reset_time": "2026-12-03T16:00:00Z" }
                    ]
                }]
            }),
            1795788000,
        );
        assert_eq!(a.session_pct, Some(18.0));
        assert_eq!(a.week_pct, Some(32.0));
    }

    #[test]
    fn test_codex_rollout_quota() {
        let dir = ScratchDir::new("codex-quota");
        let sub = dir.0.join("2026").join("11");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("session1.jsonl");
        let line = json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "rate_limits": {
                    "primary": { "used_percent": 12.3, "resets_at": 9999999999_u64 },
                    "secondary": { "used_percent": 45.6, "resets_at": 9999999999_u64 }
                }
            }
        });
        std::fs::write(&file_path, line.to_string() + "\n").unwrap();

        let quota = read_codex_rollout_quota(&dir.0, 1000);
        assert_eq!(quota.session_pct, Some(12.3));
        assert_eq!(quota.week_pct, Some(45.6));
        assert!(!quota.stale);
    }

    #[test]
    fn test_approval_lifecycle_and_replay() {
        let mut state = Esp32AgentPanelState::default();
        let id = state.on_permission(
            Some("pipe-1".to_string()),
            "claude",
            "/projects/foo",
            "bash",
            "rm test.txt",
            100,
        );

        assert!(state.pending_approval.is_some());

        // Decide allow
        let res = state.decide(&id, "allow", 100);
        assert!(res.is_ok());

        // Replay should fail with 409
        let replay = state.decide(&id, "allow", 100);
        assert!(replay.is_err());
        assert_eq!(replay.unwrap_err().0, 409);
    }

    #[test]
    fn test_approval_expiration() {
        let mut state = Esp32AgentPanelState::default();
        let id = state.on_permission(
            None,
            "claude",
            "/projects/foo",
            "bash",
            "ls",
            100,
        );

        // Decide after TTL (115s + 1 = 216s)
        let res = state.decide(&id, "allow", 216);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().0, 409);
    }
}
