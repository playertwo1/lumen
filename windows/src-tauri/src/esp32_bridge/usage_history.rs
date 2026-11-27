//! Adaptado de VibePulse (https://github.com/niclasvestlund-YT/vibepulse), MIT, (c) 2026 Niclas Vestlund

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SAMPLE_INTERVAL_SECS: u64 = 15 * 60; // 900 s
pub const RETENTION_SECS: u64 = 8 * 24 * 60 * 60; // 691200 s (8 dias)
pub const FORECAST_WINDOW_SECS: u64 = 24 * 60 * 60; // 86400 s (24 h)
pub const MIN_FORECAST_SPAN_SECS: u64 = 90 * 60; // 5400 s (90 min)
pub const MIN_FORECAST_DELTA: f64 = 1.0;
pub const RESET_QUANTUM_SECS: u64 = 5 * 60; // 300 s

#[inline]
pub fn reset_cycle(reset_at: u64) -> u64 {
    let q = RESET_QUANTUM_SECS;
    ((reset_at + q / 2) / q) * q
}

#[inline]
pub fn is_valid_provider(provider: &str) -> bool {
    matches!(provider, "claude" | "codex" | "antigravity")
}

#[inline]
pub fn is_valid_window(window: &str) -> bool {
    matches!(window, "session" | "week" | "model_week")
}

pub fn default_history_path() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local).join("Lumen").join("usage-history.json")
    } else {
        std::env::temp_dir().join("lumen-usage-history.json")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageRecord {
    pub at: u64,
    pub provider: String,
    pub window: String,
    pub pct: f64,
    pub reset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryFilePayload {
    v: u32,
    samples: Vec<UsageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderForecast {
    pub state: String,
    pub pct_at_reset: Option<f64>,
    pub pace_factor: Option<f64>,
    pub at: Option<u64>,
    pub offset_min: Option<i64>,
}

pub type Forecast = ProviderForecast;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CombinedForecasts {
    pub claude_forecast_state: String,
    pub claude_forecast_pct_at_reset: Option<f64>,
    pub claude_forecast_pace_factor: Option<f64>,
    pub claude_forecast_at: Option<u64>,
    pub claude_forecast_offset_min: Option<i64>,

    pub codex_forecast_state: String,
    pub codex_forecast_pct_at_reset: Option<f64>,
    pub codex_forecast_pace_factor: Option<f64>,
    pub codex_forecast_at: Option<u64>,
    pub codex_forecast_offset_min: Option<i64>,
}

impl CombinedForecasts {
    pub fn new(claude: &ProviderForecast, codex: &ProviderForecast) -> Self {
        Self {
            claude_forecast_state: claude.state.clone(),
            claude_forecast_pct_at_reset: claude.pct_at_reset,
            claude_forecast_pace_factor: claude.pace_factor,
            claude_forecast_at: claude.at,
            claude_forecast_offset_min: claude.offset_min,

            codex_forecast_state: codex.state.clone(),
            codex_forecast_pct_at_reset: codex.pct_at_reset,
            codex_forecast_pace_factor: codex.pace_factor,
            codex_forecast_at: codex.at,
            codex_forecast_offset_min: codex.offset_min,
        }
    }
}

pub fn merge_forecasts_into_usage(
    payload: &mut Value,
    claude: &ProviderForecast,
    codex: &ProviderForecast,
) {
    if let Value::Object(map) = payload {
        map.insert("claudeForecastState".to_string(), json!(claude.state));
        map.insert(
            "claudeForecastPctAtReset".to_string(),
            claude.pct_at_reset.map(|v| json!(v)).unwrap_or(Value::Null),
        );
        map.insert(
            "claudeForecastPaceFactor".to_string(),
            claude.pace_factor.map(|v| json!(v)).unwrap_or(Value::Null),
        );
        map.insert(
            "claudeForecastAt".to_string(),
            claude.at.map(|v| json!(v)).unwrap_or(Value::Null),
        );
        map.insert(
            "claudeForecastOffsetMin".to_string(),
            claude.offset_min.map(|v| json!(v)).unwrap_or(Value::Null),
        );

        map.insert("codexForecastState".to_string(), json!(codex.state));
        map.insert(
            "codexForecastPctAtReset".to_string(),
            codex.pct_at_reset.map(|v| json!(v)).unwrap_or(Value::Null),
        );
        map.insert(
            "codexForecastPaceFactor".to_string(),
            codex.pace_factor.map(|v| json!(v)).unwrap_or(Value::Null),
        );
        map.insert(
            "codexForecastAt".to_string(),
            codex.at.map(|v| json!(v)).unwrap_or(Value::Null),
        );
        map.insert(
            "codexForecastOffsetMin".to_string(),
            codex.offset_min.map(|v| json!(v)).unwrap_or(Value::Null),
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelWeekQuota {
    pub claude_model_week_pct: Option<f64>,
    pub claude_model_week_reset_min: Option<i64>,
    pub claude_model_week_label: Option<String>,
}

impl ClaudeModelWeekQuota {
    pub fn from_oauth_payload(payload: &Value, now_sec: u64) -> Self {
        extract_claude_model_week(payload, now_sec)
    }
}

pub fn merge_model_week_into_usage(
    payload: &mut Value,
    model_week: &ClaudeModelWeekQuota,
) {
    if let Value::Object(map) = payload {
        map.insert(
            "claudeModelWeekPct".to_string(),
            model_week
                .claude_model_week_pct
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "claudeModelWeekResetMin".to_string(),
            model_week
                .claude_model_week_reset_min
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "claudeModelWeekLabel".to_string(),
            model_week
                .claude_model_week_label
                .as_ref()
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
        );
    }
}

pub struct UsageHistoryStore {
    path: PathBuf,
    records: Mutex<Vec<UsageRecord>>,
    unreadable_at_start: bool,
}

impl UsageHistoryStore {
    pub fn new(path: PathBuf) -> Self {
        let (records, unreadable) = Self::load_file(&path);
        Self {
            path,
            records: Mutex::new(records),
            unreadable_at_start: unreadable,
        }
    }

    fn load_file(path: &Path) -> (Vec<UsageRecord>, bool) {
        if !path.exists() {
            return (Vec::new(), false);
        }
        let data = match fs::read_to_string(path) {
            Ok(d) => d,
            Err(_) => return (Vec::new(), true),
        };
        let payload: HistoryFilePayload = match serde_json::from_str(&data) {
            Ok(p) => p,
            Err(_) => return (Vec::new(), false),
        };
        if payload.v != 1 {
            return (Vec::new(), false);
        }
        let now_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now_sec.saturating_sub(RETENTION_SECS);

        let mut valid_records = Vec::new();
        for r in payload.samples {
            if is_valid_provider(&r.provider)
                && is_valid_window(&r.window)
                && r.pct.is_finite()
                && (0.0..=100.0).contains(&r.pct)
                && r.reset > 0
                && (now_sec == 0 || r.at >= cutoff)
            {
                valid_records.push(r);
            }
        }
        valid_records.sort_by_key(|r| r.at);
        (valid_records, false)
    }

    pub fn records(&self) -> Vec<UsageRecord> {
        let records = self.records.lock().unwrap();
        records.clone()
    }

    pub fn record(
        &self,
        provider: &str,
        window: &str,
        pct: f64,
        reset_at: u64,
        at: Option<u64>,
    ) -> bool {
        self.record_many(&[(provider, window, pct, reset_at)], at) > 0
    }

    pub fn record_many(&self, samples: &[(&str, &str, f64, u64)], at: Option<u64>) -> usize {
        let timestamp = at.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });

        let mut records = self.records.lock().unwrap();
        let cutoff = timestamp.saturating_sub(RETENTION_SECS);
        records.retain(|r| r.at >= cutoff);

        let mut added = 0;
        for &(provider, window, pct, reset_at) in samples {
            if !is_valid_provider(provider)
                || !is_valid_window(window)
                || !pct.is_finite()
                || !(0.0..=100.0).contains(&pct)
                || reset_at == 0
            {
                continue;
            }

            let cycle = reset_cycle(reset_at);
            let previous = records
                .iter()
                .rev()
                .find(|r| r.provider == provider && r.window == window && r.reset == cycle);

            if let Some(prev) = previous {
                if timestamp.saturating_sub(prev.at) < SAMPLE_INTERVAL_SECS {
                    continue;
                }
            }

            records.push(UsageRecord {
                at: timestamp,
                provider: provider.to_string(),
                window: window.to_string(),
                pct,
                reset: cycle,
            });
            added += 1;
        }

        if added > 0 {
            records.sort_by_key(|r| r.at);
            if !self.unreadable_at_start {
                let _ = self.persist_locked(&records);
            }
        }

        added
    }

    fn persist_locked(&self, records: &[UsageRecord]) -> io::Result<()> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let unique_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base_name = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("usage-history.json");
        let temp_path = parent.join(format!(".{}.{}.tmp", base_name, unique_ts));

        let payload = HistoryFilePayload {
            v: 1,
            samples: records.to_vec(),
        };
        let json_bytes = serde_json::to_vec(&payload)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        fs::write(&temp_path, json_bytes)?;

        #[cfg(windows)]
        {
            if self.path.exists() {
                let _ = fs::remove_file(&self.path);
            }
        }

        if let Err(e) = fs::rename(&temp_path, &self.path) {
            let _ = fs::remove_file(&temp_path);
            return Err(e);
        }

        Ok(())
    }

    pub fn forecast(
        &self,
        provider: &str,
        window: &str,
        reset_at: u64,
        now_sec: u64,
    ) -> Forecast {
        let records = self.records();
        compute_forecast(&records, provider, window, reset_at, now_sec)
    }

    pub fn delta_since(
        &self,
        provider: &str,
        window: &str,
        since: u64,
        reset_at: u64,
        now_sec: u64,
    ) -> Option<f64> {
        let window_len_secs: u64 = match window {
            "session" => 5 * 3600,
            "week" | "model_week" => 7 * 86400,
            _ => return None,
        };
        let cycle = reset_cycle(reset_at);
        let records = self.records.lock().unwrap();
        let mut samples: Vec<&UsageRecord> = records
            .iter()
            .filter(|r| {
                r.provider == provider
                    && r.window == window
                    && r.reset == cycle
                    && r.at <= now_sec
            })
            .collect();
        if samples.is_empty() {
            return None;
        }
        samples.sort_by_key(|r| r.at);
        let latest = samples[samples.len() - 1];

        let cycle_start = reset_at.saturating_sub(window_len_secs);
        if cycle_start >= since {
            return Some((latest.pct.max(0.0) * 10.0).round() / 10.0);
        }

        if samples.len() < 2 {
            return None;
        }

        let earlier: Vec<&&UsageRecord> = samples.iter().filter(|r| r.at <= since).collect();
        let baseline = match earlier.last() {
            Some(b) => **b,
            None => samples[0],
        };
        if std::ptr::eq(baseline, latest) {
            return None;
        }

        let delta = latest.pct - baseline.pct;
        if delta < 0.0 {
            None
        } else {
            Some((delta * 10.0).round() / 10.0)
        }
    }
}

pub fn compute_forecast(
    records: &[UsageRecord],
    provider: &str,
    window: &str,
    reset_at: u64,
    current_time: u64,
) -> Forecast {
    if !is_valid_provider(provider)
        || !is_valid_window(window)
        || reset_at == 0
        || current_time == 0
    {
        return Forecast {
            state: "unavailable".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: None,
            offset_min: None,
        };
    }

    let cycle = reset_cycle(reset_at);
    let cutoff = current_time.saturating_sub(FORECAST_WINDOW_SECS);

    let mut samples: Vec<&UsageRecord> = records
        .iter()
        .filter(|r| {
            r.provider == provider
                && r.window == window
                && r.reset == cycle
                && r.at >= cutoff
                && r.at <= current_time
        })
        .collect();

    if samples.is_empty() {
        return Forecast {
            state: "unavailable".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: None,
            offset_min: None,
        };
    }

    samples.sort_by_key(|r| r.at);
    let latest = samples[samples.len() - 1];

    if reset_at <= latest.at {
        return Forecast {
            state: "unavailable".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: None,
            offset_min: None,
        };
    }

    let first = samples[0];
    let span = latest.at - first.at;

    let min_pct = samples.iter().map(|r| r.pct).fold(f64::INFINITY, f64::min);
    let max_pct = samples
        .iter()
        .map(|r| r.pct)
        .fold(f64::NEG_INFINITY, f64::max);
    let movement = max_pct - min_pct;

    if samples.len() < 3 || span < MIN_FORECAST_SPAN_SECS || movement < MIN_FORECAST_DELTA {
        return Forecast {
            state: "collecting".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: None,
            offset_min: None,
        };
    }

    let xs: Vec<f64> = samples.iter().map(|r| (r.at - first.at) as f64).collect();
    let ys: Vec<f64> = samples.iter().map(|r| r.pct).collect();
    let n = xs.len() as f64;
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;

    let denominator = xs.iter().map(|&x| (x - mean_x).powi(2)).sum::<f64>();
    if denominator <= 0.0 {
        return Forecast {
            state: "collecting".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: None,
            offset_min: None,
        };
    }

    let numerator: f64 = xs
        .iter()
        .zip(ys.iter())
        .map(|(&x, &y)| (x - mean_x) * (y - mean_y))
        .sum();
    let slope = numerator / denominator;

    if !slope.is_finite() || slope <= 0.0 {
        return Forecast {
            state: "unavailable".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: None,
            offset_min: None,
        };
    }

    let seconds_left = (reset_at - latest.at) as f64;
    let projected_gain = slope * seconds_left;
    let projected_pct = latest.pct + projected_gain;

    if projected_pct >= 100.0 {
        let exhausts_at = (latest.at as f64 + (100.0 - latest.pct) / slope).round() as u64;
        let offset_minutes = (((exhausts_at as f64 - reset_at as f64) / 60.0).round()) as i64;
        return Forecast {
            state: "exhausts".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: Some(exhausts_at),
            offset_min: Some(offset_minutes),
        };
    }

    let pace_factor = if projected_gain > 0.0 {
        Some(((100.0 - latest.pct) / projected_gain * 10.0).round() / 10.0)
    } else {
        None
    };

    let pct_at_reset = projected_pct.round().clamp(0.0, 100.0);

    Forecast {
        state: "at_reset".to_string(),
        pct_at_reset: Some(pct_at_reset),
        pace_factor,
        at: None,
        offset_min: None,
    }
}

pub fn parse_timestamp_value(val: &Value) -> Option<u64> {
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
    } else if let Some(f) = val.as_f64() {
        if f > 1_000_000_000_000.0 {
            Some((f / 1000.0) as u64)
        } else if f > 0.0 {
            Some(f as u64)
        } else {
            None
        }
    } else if let Some(s) = val.as_str() {
        if let Some(ms) = crate::whatsapp::gateway::rfc3339_ms(Some(s)) {
            if ms > 0 {
                return Some((ms / 1000) as u64);
            }
        }
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

fn model_name_to_label(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    if lower.contains("fable") {
        "FABLE · WEEK".to_string()
    } else if lower.contains("opus") {
        "OPUS · WEEK".to_string()
    } else if lower.contains("sonnet") {
        "SONNET · WEEK".to_string()
    } else {
        format!("{} · WEEK", lower.to_ascii_uppercase())
    }
}

struct CandidateModelQuota {
    pct: f64,
    reset_at: Option<u64>,
    label: String,
}

pub fn extract_claude_model_week(payload: &Value, now_sec: u64) -> ClaudeModelWeekQuota {
    let mut candidates: Vec<CandidateModelQuota> = Vec::new();

    // 1. Array de `limits` (estrutura de OAuth usage de Anthropic)
    if let Some(limits) = payload.get("limits").and_then(|v| v.as_array()) {
        for limit in limits {
            let kind = limit.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "weekly_scoped" {
                let pct_raw = limit
                    .get("percent")
                    .or_else(|| limit.get("utilization"))
                    .and_then(|v| v.as_f64());
                let is_active = limit
                    .get("is_active")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if let Some(mut pct) = pct_raw {
                    if pct <= 1.0 && pct > 0.0 {
                        pct *= 100.0;
                    }
                    if pct > 0.0 || is_active {
                        let reset_at = limit.get("resets_at").and_then(parse_timestamp_value);
                        let display_name = limit
                            .get("scope")
                            .and_then(|s| s.get("model"))
                            .and_then(|m| m.get("display_name"))
                            .and_then(|d| d.as_str())
                            .or_else(|| limit.get("model").and_then(|m| m.as_str()))
                            .unwrap_or("model");

                        candidates.push(CandidateModelQuota {
                            pct,
                            reset_at,
                            label: model_name_to_label(display_name),
                        });
                    }
                }
            }
        }
    }

    // 2. Map / Chaves diretas no payload (ex.: seven_day_opus, seven_day_sonnet, 7d_opus, etc.)
    if let Some(obj) = payload.as_object() {
        for (k, v) in obj {
            let lower = k.to_ascii_lowercase();
            // Ignorar chaves que representam a semana global ou sessão
            if lower == "seven_day"
                || lower == "7d"
                || lower == "weekly"
                || lower == "five_hour"
                || lower == "session"
                || lower == "5h"
            {
                continue;
            }

            let is_model_bucket = lower.starts_with("seven_day_")
                || lower.starts_with("7d_")
                || lower == "model_week"
                || lower.contains("opus")
                || lower.contains("sonnet")
                || lower.contains("fable");

            if !is_model_bucket {
                continue;
            }

            let label = if lower.contains("opus") {
                "OPUS · WEEK".to_string()
            } else if lower.contains("sonnet") {
                "SONNET · WEEK".to_string()
            } else if lower.contains("fable") {
                "FABLE · WEEK".to_string()
            } else if let Some(suffix) = lower
                .strip_prefix("seven_day_")
                .or_else(|| lower.strip_prefix("7d_"))
            {
                if suffix == "all" {
                    continue;
                }
                model_name_to_label(suffix)
            } else {
                model_name_to_label(&lower)
            };

            let mut pct_opt = None;
            let mut reset_opt = None;

            if let Some(obj_val) = v.as_object() {
                pct_opt = obj_val
                    .get("utilization")
                    .or_else(|| obj_val.get("percent"))
                    .or_else(|| obj_val.get("used_percentage"))
                    .or_else(|| obj_val.get("pct"))
                    .and_then(|val| val.as_f64());
                reset_opt = obj_val
                    .get("resets_at")
                    .or_else(|| obj_val.get("reset_time"))
                    .or_else(|| obj_val.get("reset"))
                    .and_then(parse_timestamp_value);
            } else if let Some(p) = v.as_f64() {
                pct_opt = Some(p);
            }

            if let Some(mut pct) = pct_opt {
                if pct <= 1.0 && pct > 0.0 {
                    pct *= 100.0;
                }
                candidates.push(CandidateModelQuota {
                    pct,
                    reset_at: reset_opt,
                    label,
                });
            }
        }
    }

    if candidates.is_empty() {
        return ClaudeModelWeekQuota {
            claude_model_week_pct: None,
            claude_model_week_reset_min: None,
            claude_model_week_label: None,
        };
    }

    // Seleciona o modelo com maior utilização semanal (mais pesado)
    let best = candidates
        .into_iter()
        .max_by(|a, b| a.pct.partial_cmp(&b.pct).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();

    let reset_min = best.reset_at.map(|reset_sec| {
        if reset_sec >= now_sec {
            ((reset_sec - now_sec) as i64 + 59) / 60
        } else {
            0
        }
    });

    ClaudeModelWeekQuota {
        claude_model_week_pct: Some((best.pct * 10.0).round() / 10.0),
        claude_model_week_reset_min: reset_min,
        claude_model_week_label: Some(best.label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * HOUR;

    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new(suffix: &str) -> Self {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "lumen-test-{}-{}-{}",
                suffix,
                std::process::id(),
                ts
            ));
            let _ = fs::create_dir_all(&path);
            TestTempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn make_test_store() -> (UsageHistoryStore, PathBuf, TestTempDir) {
        let temp_dir = TestTempDir::new("usage-history");
        let path = temp_dir.path().join("usage-history.json");
        let store = UsageHistoryStore::new(path.clone());
        (store, path, temp_dir)
    }

    #[test]
    fn test_records_at_most_one_sample_per_fifteen_minutes() {
        let (store, _path, _dir) = make_test_store();

        assert!(store.record("claude", "week", 10.0, 7 * DAY, Some(0)));
        assert!(!store.record("claude", "week", 11.0, 7 * DAY, Some(899)));
        assert!(store.record("claude", "week", 12.0, 7 * DAY, Some(900)));

        let recs = store.records();
        let pcts: Vec<f64> = recs.iter().map(|r| r.pct).collect();
        assert_eq!(pcts, vec![10.0, 12.0]);
    }

    #[test]
    fn test_prunes_samples_older_than_eight_days() {
        let (store, _path, _dir) = make_test_store();

        assert!(store.record("claude", "week", 5.0, 7 * DAY, Some(0)));
        assert!(store.record("codex", "week", 20.0, 10 * DAY, Some(8 * DAY + 1)));

        let recs = store.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].provider, "codex");
    }

    #[test]
    fn test_persists_with_atomic_replace_and_fixed_privacy_schema() {
        let temp_dir = TestTempDir::new("nested-history");
        let path = temp_dir.path().join("nested").join("usage-history.json");
        let store = UsageHistoryStore::new(path.clone());

        assert!(store.record("claude", "model_week", 73.0, 7 * DAY, Some(HOUR)));
        assert!(path.exists());

        let raw = fs::read_to_string(&path).unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload.get("v").and_then(|v| v.as_u64()), Some(1));

        let samples = payload.get("samples").and_then(|v| v.as_array()).unwrap();
        assert_eq!(samples.len(), 1);
        let s0 = &samples[0];
        assert_eq!(s0.get("provider").and_then(|v| v.as_str()), Some("claude"));
        assert_eq!(s0.get("window").and_then(|v| v.as_str()), Some("model_week"));
        assert_eq!(s0.get("pct").and_then(|v| v.as_f64()), Some(73.0));
        assert!(s0.get("at").is_some());
        assert!(s0.get("reset").is_some());
    }

    #[test]
    fn test_forecast_is_unavailable_without_samples() {
        let (store, _path, _dir) = make_test_store();
        let forecast = store.forecast("claude", "week", 4 * HOUR, 0);
        assert_eq!(forecast.state, "unavailable");
    }

    #[test]
    fn test_forecast_collects_until_three_points_span_ninety_minutes() {
        let (store, _path, _dir) = make_test_store();
        store.record("claude", "week", 20.0, 4 * HOUR, Some(0));
        store.record("claude", "week", 25.0, 4 * HOUR, Some(HOUR));
        store.record("claude", "week", 28.0, 4 * HOUR, Some(HOUR + 29 * 60));

        let now = HOUR + 29 * 60;
        let forecast = store.forecast("claude", "week", 4 * HOUR, now);
        assert_eq!(forecast.state, "collecting");
    }

    #[test]
    fn test_forecast_collects_until_usage_moves_one_percentage_point() {
        let (store, _path, _dir) = make_test_store();
        store.record("claude", "week", 20.0, 4 * HOUR, Some(0));
        store.record("claude", "week", 20.3, 4 * HOUR, Some(HOUR));
        store.record("claude", "week", 20.8, 4 * HOUR, Some(2 * HOUR));

        let now = 2 * HOUR;
        let forecast = store.forecast("claude", "week", 4 * HOUR, now);
        assert_eq!(forecast.state, "collecting");
    }

    #[test]
    fn test_low_pace_projects_reset_percentage_and_required_multiplier() {
        let (store, _path, _dir) = make_test_store();
        store.record("claude", "week", 20.0, 4 * HOUR, Some(0));
        store.record("claude", "week", 25.0, 4 * HOUR, Some(HOUR));
        store.record("claude", "week", 30.0, 4 * HOUR, Some(2 * HOUR));

        let now = 2 * HOUR;
        let forecast = store.forecast("claude", "week", 4 * HOUR, now);

        assert_eq!(forecast.state, "at_reset");
        assert_eq!(forecast.pct_at_reset, Some(40.0));
        assert_eq!(forecast.pace_factor, Some(7.0));
        assert!(forecast.at.is_none());
        assert!(forecast.offset_min.is_none());
    }

    #[test]
    fn test_fast_pace_projects_exhaustion_before_reset() {
        let (store, _path, _dir) = make_test_store();
        store.record("claude", "week", 70.0, 4 * HOUR, Some(0));
        store.record("claude", "week", 80.0, 4 * HOUR, Some(HOUR));
        store.record("claude", "week", 90.0, 4 * HOUR, Some(2 * HOUR));

        let now = 2 * HOUR;
        let forecast = store.forecast("claude", "week", 4 * HOUR, now);

        assert_eq!(forecast.state, "exhausts");
        assert_eq!(forecast.at, Some(3 * HOUR));
        assert_eq!(forecast.offset_min, Some(-60));
        assert!(forecast.pct_at_reset.is_none());
    }

    #[test]
    fn test_falling_usage_has_no_misleading_forecast() {
        let (store, _path, _dir) = make_test_store();
        store.record("claude", "week", 50.0, 4 * HOUR, Some(0));
        store.record("claude", "week", 48.0, 4 * HOUR, Some(HOUR));
        store.record("claude", "week", 46.0, 4 * HOUR, Some(2 * HOUR));

        let now = 2 * HOUR;
        let forecast = store.forecast("claude", "week", 4 * HOUR, now);
        assert_eq!(forecast.state, "unavailable");
    }

    #[test]
    fn test_forecast_uses_only_current_reset_cycle() {
        let (store, _path, _dir) = make_test_store();
        store.record("claude", "week", 80.0, 4 * HOUR, Some(0));
        store.record("claude", "week", 90.0, 4 * HOUR, Some(HOUR));
        store.record("claude", "week", 10.0, 11 * HOUR, Some(2 * HOUR));
        store.record("claude", "week", 15.0, 11 * HOUR, Some(3 * HOUR));
        store.record("claude", "week", 20.0, 11 * HOUR, Some(4 * HOUR));

        let forecast = store.forecast("claude", "week", 11 * HOUR, 4 * HOUR);
        assert_eq!(forecast.state, "at_reset");
        assert_eq!(forecast.pct_at_reset, Some(55.0));
    }

    #[test]
    fn test_forecast_ignores_points_older_than_twenty_four_hours() {
        let (store, _path, _dir) = make_test_store();
        let reset_at = 32 * HOUR;
        store.record("claude", "week", 70.0, reset_at, Some(0));
        store.record("claude", "week", 10.0, reset_at, Some(25 * HOUR));
        store.record("claude", "week", 15.0, reset_at, Some(26 * HOUR));
        store.record("claude", "week", 20.0, reset_at, Some(27 * HOUR));

        let forecast = store.forecast("claude", "week", reset_at, 27 * HOUR);
        assert_eq!(forecast.state, "at_reset");
        assert_eq!(forecast.pct_at_reset, Some(45.0));
    }

    #[test]
    fn test_delta_since_basic_flow() {
        let (store, _path, _dir) = make_test_store();
        let reset_at = 10 * HOUR;
        store.record("claude", "week", 40.0, reset_at, Some(HOUR));
        store.record("claude", "week", 43.0, reset_at, Some(2 * HOUR));
        store.record("claude", "week", 47.0, reset_at, Some(3 * HOUR));

        let delta = store.delta_since("claude", "week", HOUR + 30 * 60, reset_at, 3 * HOUR);
        assert_eq!(delta, Some(7.0));
    }

    #[test]
    fn test_extract_claude_model_week_from_oauth_limits() {
        let payload = json!({
            "limits": [
                {
                    "kind": "session",
                    "percent": 15.0,
                    "resets_at": 1800000000
                },
                {
                    "kind": "weekly_scoped",
                    "percent": 82.5,
                    "resets_at": 1800003600,
                    "scope": {
                        "model": {
                            "display_name": "Opus"
                        }
                    }
                },
                {
                    "kind": "weekly_scoped",
                    "percent": 45.0,
                    "resets_at": 1800007200,
                    "scope": {
                        "model": {
                            "display_name": "Sonnet"
                        }
                    }
                }
            ]
        });

        let now_sec = 1800000000;
        let quota = extract_claude_model_week(&payload, now_sec);

        assert_eq!(quota.claude_model_week_pct, Some(82.5));
        assert_eq!(quota.claude_model_week_label, Some("OPUS · WEEK".to_string()));
        assert_eq!(quota.claude_model_week_reset_min, Some(60));
    }

    #[test]
    fn test_extract_claude_model_week_from_direct_keys() {
        let payload = json!({
            "seven_day_opus": {
                "utilization": 0.88,
                "resets_at": 1800007200
            },
            "seven_day_sonnet": {
                "utilization": 0.35,
                "resets_at": 1800003600
            }
        });

        let now_sec = 1800000000;
        let quota = extract_claude_model_week(&payload, now_sec);

        assert_eq!(quota.claude_model_week_pct, Some(88.0));
        assert_eq!(quota.claude_model_week_label, Some("OPUS · WEEK".to_string()));
        assert_eq!(quota.claude_model_week_reset_min, Some(120));
    }

    #[test]
    fn test_merge_helpers_into_usage_payload() {
        let mut usage = json!({
            "claudeSessionPct": 10.0,
            "codexWeekPct": 50.0
        });

        let claude_fc = ProviderForecast {
            state: "at_reset".to_string(),
            pct_at_reset: Some(45.0),
            pace_factor: Some(5.2),
            at: None,
            offset_min: None,
        };

        let codex_fc = ProviderForecast {
            state: "exhausts".to_string(),
            pct_at_reset: None,
            pace_factor: None,
            at: Some(1800010000),
            offset_min: Some(-45),
        };

        merge_forecasts_into_usage(&mut usage, &claude_fc, &codex_fc);

        assert_eq!(usage["claudeForecastState"], "at_reset");
        assert_eq!(usage["claudeForecastPctAtReset"], 45.0);
        assert_eq!(usage["claudeForecastPaceFactor"], 5.2);
        assert_eq!(usage["claudeForecastAt"], Value::Null);
        assert_eq!(usage["claudeForecastOffsetMin"], Value::Null);

        assert_eq!(usage["codexForecastState"], "exhausts");
        assert_eq!(usage["codexForecastAt"], 1800010000);
        assert_eq!(usage["codexForecastOffsetMin"], -45);

        let model_week = ClaudeModelWeekQuota {
            claude_model_week_pct: Some(73.5),
            claude_model_week_reset_min: Some(90),
            claude_model_week_label: Some("SONNET · WEEK".to_string()),
        };

        merge_model_week_into_usage(&mut usage, &model_week);

        assert_eq!(usage["claudeModelWeekPct"], 73.5);
        assert_eq!(usage["claudeModelWeekResetMin"], 90);
        assert_eq!(usage["claudeModelWeekLabel"], "SONNET · WEEK");
    }
}
