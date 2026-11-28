use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

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
        // Janela que já zerou depois da última leitura: uso 0% e próximo reset desconhecido.
        let window = |pct: Option<f64>, reset: Option<u64>| match reset {
            Some(r) if r <= now_sec => (pct.map(|_| 0.0), None),
            Some(r) => (pct, Some(((r - now_sec) as i64 + 59) / 60)),
            None => (pct, None),
        };
        let (sess_pct, sess_min) = window(self.session_pct, self.session_resets_at);
        let (week_pct, week_min) = window(self.week_pct, self.week_resets_at);
        (sess_pct, sess_min, week_pct, week_min, stale)
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

/// Uma janela de cota do Codex (`primary` = 5 h, `secondary` = semana) do último registro.
/// Se o `resets_at` já passou, a janela zerou depois desse registro: o uso real é 0% e o próximo
/// reset só existe quando o Codex for usado de novo (por isso `None`, a placa mostra "—").
fn codex_window(w: &serde_json::Value, now_sec: u64) -> Option<(Option<f64>, Option<i64>)> {
    let pct = w.get("used_percent").and_then(|x| x.as_f64());
    let reset = w.get("resets_at").and_then(parse_resets_at);
    match reset {
        Some(res) if res <= now_sec => Some((Some(0.0), None)),
        Some(res) => Some((pct, Some(((res - now_sec) as i64 + 59) / 60))),
        None if pct.is_some() => Some((pct, None)),
        None => None,
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
                            if let Some(w) = rl.get("primary").or_else(|| rl.get("5h")) {
                                if let Some((p, r)) = codex_window(w, now_sec) {
                                    session_pct = p;
                                    session_reset_min = r;
                                    found_primary = true;
                                }
                            }
                        }

                        if !found_secondary {
                            if let Some(w) = rl.get("secondary").or_else(|| rl.get("7d")) {
                                if let Some((p, r)) = codex_window(w, now_sec) {
                                    week_pct = p;
                                    week_reset_min = r;
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
    fn test_codex_rollout_quota_janela_ja_zerada() {
        // Registro antigo: a janela de 5 h já zerou, a semanal ainda não.
        let dir = ScratchDir::new("codex-quota-expired");
        std::fs::create_dir_all(&dir.0).unwrap();
        let line = json!({
            "payload": {
                "rate_limits": {
                    "primary": { "used_percent": 52.0, "resets_at": 1_000_u64 },
                    "secondary": { "used_percent": 28.0, "resets_at": 1_000_000_u64 }
                }
            }
        });
        std::fs::write(dir.0.join("s.jsonl"), line.to_string() + "\n").unwrap();

        let quota = read_codex_rollout_quota(&dir.0, 5_000);
        assert_eq!(quota.session_pct, Some(0.0));
        assert_eq!(quota.session_reset_min, None);
        assert_eq!(quota.week_pct, Some(28.0));
        assert_eq!(quota.week_reset_min, Some((1_000_000 - 5_000 + 59) / 60));
        assert!(!quota.stale);
    }
}
