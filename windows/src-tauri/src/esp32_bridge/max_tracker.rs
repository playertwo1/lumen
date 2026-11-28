//! Adaptado de VibePulse (https://github.com/niclasvestlund-YT/vibepulse), MIT, (c) 2026 Niclas Vestlund

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_DAYS: usize = 400;
pub const MAX_WEEKS: usize = 60;
pub const MAX_BACKFILL_ENTRIES: usize = 2000;
pub const WINDOW_WEEKS: usize = 20;
pub const WINDOW_DAYS: usize = WINDOW_WEEKS * 7; // 140
pub const AGGREGATE_MAX: u32 = 999;
pub const MAX_BUDGET_BYTES: usize = 1_048_576; // 1 MiB
pub const MAX_RECORDS_PER_STEP: usize = 256;
pub const BLOCK_BYTES: usize = 64 * 1024;
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024; // 8 MiB
pub const SCHEMA_VERSION: u32 = 1;
pub const GENERAL_WINDOW_MINUTES: f64 = 600.0;

pub fn default_max_tracker_path() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local).join("Lumen").join("max-tracker.json")
    } else {
        std::env::temp_dir().join("lumen-max-tracker.json")
    }
}

// ---------------------------------------------------------------------------
// Pure calendar arithmetic (Howard Hinnant algorithm, no IO, no timezones)
// ---------------------------------------------------------------------------

#[inline]
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[inline]
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

pub fn parse_date_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let (vy, vm, vd) = civil_from_days(days);
    if vy == y && vm == m && vd == d {
        Some((y, m, d))
    } else {
        None
    }
}

pub fn date_to_days(s: &str) -> Option<i64> {
    let (y, m, d) = parse_date_ymd(s)?;
    Some(days_from_civil(y, m, d))
}

pub fn days_to_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

#[inline]
pub fn isoweekday(days: i64) -> u32 {
    // 1970-01-01 was Thursday (iso weekday 4).
    ((days + 3).rem_euclid(7) + 1) as u32
}

pub fn week_key(date_str: &str) -> Option<String> {
    let days = date_to_days(date_str)?;
    let w = isoweekday(days);
    let thurs_days = days + 4 - w as i64;
    let (thurs_y, _, _) = civil_from_days(thurs_days);
    let jan4_days = days_from_civil(thurs_y, 1, 4);
    let jan4_w = isoweekday(jan4_days);
    let mon1_days = jan4_days - (jan4_w as i64 - 1);
    let week_num = (days - mon1_days) / 7 + 1;
    Some(format!("{thurs_y:04}-W{week_num:02}"))
}

pub fn week_key_to_monday_days(key: &str) -> Option<i64> {
    let parts: Vec<&str> = key.split("-W").collect();
    if parts.len() != 2 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let week: i64 = parts[1].parse().ok()?;
    if !(1..=53).contains(&week) {
        return None;
    }
    let jan4_days = days_from_civil(year, 1, 4);
    let jan4_w = isoweekday(jan4_days);
    let mon1_days = jan4_days - (jan4_w as i64 - 1);
    Some(mon1_days + (week - 1) * 7)
}

fn local_timezone_offset_secs() -> i64 {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let lt = crate::platform::local_time();
    let local_days = days_from_civil(lt.year as i32, lt.month, lt.day);
    let local_secs = (local_days * 86_400)
        + (lt.hour as i64 * 3600)
        + (lt.minute as i64 * 60)
        + lt.second as i64;
    let diff = local_secs - (now_unix as i64);
    ((diff + 450) / 900) * 900
}

pub fn local_date_from_epoch_secs(secs: u64) -> String {
    let offset_secs = local_timezone_offset_secs();
    let local_secs = (secs as i64) + offset_secs;
    let days = local_secs.div_euclid(86_400);
    days_to_date(days)
}

pub fn current_month_start_secs() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let offset_secs = local_timezone_offset_secs();
    let local_secs = (now as i64) + offset_secs;
    let days = local_secs.div_euclid(86_400);
    let (y, m, _) = civil_from_days(days);
    let month_start_days = days_from_civil(y, m, 1);
    let month_start_local_secs = month_start_days * 86_400;
    let month_start_utc = month_start_local_secs - offset_secs;
    month_start_utc.max(0) as u64
}

// ---------------------------------------------------------------------------
// Pure aggregation and rendering functions
// ---------------------------------------------------------------------------

#[inline]
pub fn round_day_pct(pct: f64) -> i32 {
    if !pct.is_finite() || pct <= 0.0 {
        return 0;
    }
    if pct >= 100.0 {
        return 100;
    }
    let rounded = (pct + 0.5).floor() as i32;
    rounded.min(99).max(0)
}

pub fn volume_levels(day_volumes: &HashMap<String, u64>) -> HashMap<String, u8> {
    let mut distinct: Vec<u64> = day_volumes
        .values()
        .copied()
        .filter(|&v| v > 0)
        .collect();
    distinct.sort_unstable();
    distinct.dedup();

    let count = distinct.len();
    let mut rank = HashMap::with_capacity(count);
    if count > 0 {
        for (index, &volume) in distinct.iter().enumerate() {
            let level = ((index * 3) / count) as u8;
            rank.insert(volume, level.min(2));
        }
    }

    let mut result = HashMap::with_capacity(day_volumes.len());
    for (day, &volume) in day_volumes {
        let level = if volume > 0 {
            *rank.get(&volume).unwrap_or(&0)
        } else {
            0
        };
        result.insert(day.clone(), level);
    }
    result
}

pub fn coding_streak(active_dates: &HashSet<String>, today: &str) -> u32 {
    if active_dates.is_empty() {
        return 0;
    }
    let today_days = match date_to_days(today) {
        Some(d) => d,
        None => return 0,
    };
    let mut cursor = if active_dates.contains(today) {
        today_days
    } else {
        today_days - 1
    };
    let mut streak: u32 = 0;
    while active_dates.contains(&days_to_date(cursor)) {
        streak += 1;
        cursor -= 1;
    }
    streak.min(AGGREGATE_MAX)
}

pub fn max_weeks_streak(week_maxed: &HashMap<String, bool>, this_week: &str) -> u32 {
    let mut cursor_monday = match week_key_to_monday_days(this_week) {
        Some(d) => d - 7,
        None => return 0,
    };
    let mut streak: u32 = 0;
    loop {
        let cursor_date = days_to_date(cursor_monday);
        let Some(cursor_key) = week_key(&cursor_date) else {
            break;
        };
        if week_maxed.get(&cursor_key).copied().unwrap_or(false) {
            streak += 1;
            cursor_monday -= 7;
        } else {
            break;
        }
    }
    streak.min(AGGREGATE_MAX)
}

#[derive(Debug, Clone, Default)]
pub struct DayDisplayRecord {
    pub pct: Option<i32>,
    pub lvl: Option<i32>,
}

pub fn dense_window(
    today: &str,
    weeks: usize,
    per_day: &HashMap<String, DayDisplayRecord>,
) -> Vec<[i32; 2]> {
    let today_days = date_to_days(today).unwrap_or(0);
    let today_w = isoweekday(today_days);
    let current_monday = today_days - (today_w as i64 - 1);
    let window_start = current_monday - 7 * (weeks as i64 - 1);

    let mut out = Vec::with_capacity(weeks * 7);
    for offset in 0..(weeks * 7) {
        let day_days = window_start + offset as i64;
        if day_days > today_days {
            out.push([-1, -1]);
            continue;
        }
        let date_str = days_to_date(day_days);
        match per_day.get(&date_str) {
            Some(record) => {
                let pct = record.pct.unwrap_or(-1);
                let lvl = record.lvl.unwrap_or(-1);
                out.push([pct, lvl]);
            }
            None => {
                out.push([-1, -1]);
            }
        }
    }
    out
}

pub fn window_week_keys(today: &str, weeks: usize) -> Vec<String> {
    let today_days = date_to_days(today).unwrap_or(0);
    let today_w = isoweekday(today_days);
    let current_monday = today_days - (today_w as i64 - 1);
    let window_start = current_monday - 7 * (weeks as i64 - 1);

    (0..weeks)
        .map(|index| {
            let week_monday = window_start + (index as i64) * 7;
            week_key(&days_to_date(week_monday)).unwrap_or_default()
        })
        .collect()
}

pub fn map_plan_label(plan: &str) -> Option<&'static str> {
    match plan.trim().to_ascii_lowercase().as_str() {
        "pro" => Some("PRO"),
        "max5x" => Some("MAX 5X"),
        "max20x" => Some("MAX 20X"),
        "plus" => Some("PLUS"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Store data structures & persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct DayRecord {
    pub pct: Option<i32>,
    pub act: bool,
    pub vol: u64,
    pub loaded_lvl: Option<u8>,
    pub maxed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackfillEntry {
    pub offset: u64,
    pub size: u64,
    pub done: bool,
    #[serde(default)]
    pub discarding: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderState {
    pub days: HashMap<String, DayRecord>,
    pub weeks: HashMap<String, bool>,
    pub backfill: HashMap<u64, BackfillEntry>,
}

#[derive(Debug, Clone)]
pub struct TrackerInternalState {
    pub providers: HashMap<String, ProviderState>,
    pub stale: bool,
}

impl TrackerInternalState {
    pub fn new() -> Self {
        let mut providers = HashMap::new();
        providers.insert("claude".to_string(), ProviderState::default());
        providers.insert("codex".to_string(), ProviderState::default());
        Self {
            providers,
            stale: false,
        }
    }
}

impl Default for TrackerInternalState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn file_path_key(path: &Path) -> u64 {
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = p.to_string_lossy().to_ascii_lowercase();
    let digest = ring::digest::digest(&ring::digest::SHA256, s.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest.as_ref()[..8]);
    u64::from_be_bytes(bytes)
}

fn quarantine_file(path: &Path, reason: &str) {
    if !path.exists() {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("max-tracker.json");
    let quarantine_name = format!("{}.corrupt-{}", file_name, ts);
    let quarantine_path = parent.join(quarantine_name);
    let _ = fs::rename(path, &quarantine_path);
    eprintln!(
        "WARNING: {} quarantined as {}: {}",
        path.display(),
        quarantine_path.display(),
        reason
    );
}

fn load_file(path: &Path) -> (TrackerInternalState, bool) {
    if !path.exists() {
        return (TrackerInternalState::new(), false);
    }

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                return (TrackerInternalState::new(), false);
            }
            eprintln!(
                "WARNING: {} exists but could not be read ({}): starting empty and refusing to save",
                path.display(),
                e
            );
            return (TrackerInternalState::new(), true);
        }
    };

    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            quarantine_file(path, "not UTF-8");
            return (TrackerInternalState::new(), false);
        }
    };

    let payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            quarantine_file(path, &format!("invalid JSON: {}", e));
            return (TrackerInternalState::new(), false);
        }
    };

    let Some(obj) = payload.as_object() else {
        quarantine_file(path, "top level is not an object");
        return (TrackerInternalState::new(), false);
    };

    for provider in &["claude", "codex"] {
        let Some(section) = obj.get(*provider).and_then(Value::as_object) else {
            quarantine_file(path, "provider section (claude/codex) is missing");
            return (TrackerInternalState::new(), false);
        };
        let v = section.get("v").and_then(Value::as_u64);
        if v != Some(SCHEMA_VERSION as u64) {
            quarantine_file(path, "schema version mismatch or missing");
            return (TrackerInternalState::new(), false);
        }
        if !section.get("days").map_or(false, Value::is_object)
            || !section.get("weeks").map_or(false, Value::is_object)
            || !section.get("backfill").map_or(false, Value::is_object)
        {
            quarantine_file(
                path,
                "a provider section is not the {v, days, weeks, backfill} shape",
            );
            return (TrackerInternalState::new(), false);
        }
    }

    let mut state = TrackerInternalState::new();
    for provider in &["claude", "codex"] {
        let section = obj.get(*provider).unwrap().as_object().unwrap();
        let p_state = state.providers.get_mut(*provider).unwrap();

        if let Some(days_obj) = section.get("days").and_then(Value::as_object) {
            for (day_str, day_val) in days_obj {
                if date_to_days(day_str).is_none() {
                    continue;
                }
                let Some(record_obj) = day_val.as_object() else {
                    continue;
                };
                let act = record_obj
                    .get("act")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let pct = record_obj
                    .get("pct")
                    .and_then(Value::as_f64)
                    .filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
                    .map(round_day_pct);
                let lvl = if act {
                    record_obj
                        .get("lvl")
                        .and_then(Value::as_i64)
                        .map(|l| l.clamp(0, 2) as u8)
                } else {
                    None
                };
                let maxed = pct == Some(100);

                p_state.days.insert(
                    day_str.to_string(),
                    DayRecord {
                        pct,
                        act,
                        vol: 0,
                        loaded_lvl: lvl,
                        maxed,
                    },
                );
            }
        }

        if let Some(weeks_obj) = section.get("weeks").and_then(Value::as_object) {
            for (week_str, val) in weeks_obj {
                if week_key_to_monday_days(week_str).is_none() {
                    continue;
                }
                if val.as_bool().unwrap_or(false) {
                    p_state.weeks.insert(week_str.to_string(), true);
                }
            }
        }

        if let Some(backfill_obj) = section.get("backfill").and_then(Value::as_object) {
            for (key_str, entry_val) in backfill_obj {
                if !key_str.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let Ok(inode) = key_str.parse::<u64>() else {
                    continue;
                };
                let Ok(entry) = serde_json::from_value::<BackfillEntry>(entry_val.clone()) else {
                    continue;
                };
                p_state.backfill.insert(inode, entry);
            }
        }
    }

    (state, false)
}

fn prune_retention(state: &mut TrackerInternalState, today_opt: Option<&str>) {
    let today_days = match today_opt {
        Some(t) => date_to_days(t).unwrap_or_else(|| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            date_to_days(&local_date_from_epoch_secs(now)).unwrap_or(0)
        }),
        None => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            date_to_days(&local_date_from_epoch_secs(now)).unwrap_or(0)
        }
    };

    let cutoff_days = today_days - MAX_DAYS as i64;
    let week_cutoff_days = cutoff_days - 7;

    for provider_state in state.providers.values_mut() {
        provider_state.days.retain(|day_str, _| {
            date_to_days(day_str).map_or(false, |days| days >= cutoff_days)
        });

        provider_state.weeks.retain(|week_str, _| {
            week_key_to_monday_days(week_str).map_or(false, |monday_days| monday_days >= week_cutoff_days)
        });

        if provider_state.backfill.len() > MAX_BACKFILL_ENTRIES {
            let excess = provider_state.backfill.len() - MAX_BACKFILL_ENTRIES;
            let keys_to_remove: Vec<u64> = provider_state
                .backfill
                .keys()
                .take(excess)
                .copied()
                .collect();
            for k in keys_to_remove {
                provider_state.backfill.remove(&k);
            }
        }
    }
}

fn build_persisted_payload(state: &TrackerInternalState) -> Value {
    let mut root = serde_json::Map::new();

    for provider in &["claude", "codex"] {
        let p_state = state.providers.get(*provider);
        let days = p_state.map(|p| &p.days);

        let mut rankable = HashMap::new();
        if let Some(d) = days {
            for (day, record) in d {
                if record.act && record.loaded_lvl.is_none() {
                    rankable.insert(day.clone(), record.vol);
                }
            }
        }
        let levels = volume_levels(&rankable);

        let mut days_out = serde_json::Map::new();
        if let Some(d) = days {
            let mut sorted_days: Vec<&String> = d.keys().collect();
            sorted_days.sort();
            for day in sorted_days {
                let record = &d[day];
                let lvl_out = if !record.act {
                    Value::Null
                } else if let Some(l) = record.loaded_lvl {
                    json!(l)
                } else {
                    json!(*levels.get(day).unwrap_or(&0))
                };

                let pct_out = match record.pct {
                    Some(p) => json!(p),
                    None => Value::Null,
                };

                let mut day_obj = serde_json::Map::new();
                day_obj.insert("pct".to_string(), pct_out);
                day_obj.insert("act".to_string(), json!(record.act));
                day_obj.insert("lvl".to_string(), lvl_out);

                days_out.insert(day.clone(), Value::Object(day_obj));
            }
        }

        let mut weeks_out = serde_json::Map::new();
        if let Some(p) = p_state {
            let mut sorted_weeks: Vec<&String> = p.weeks.keys().collect();
            sorted_weeks.sort();
            for week in sorted_weeks {
                if p.weeks[week] {
                    weeks_out.insert(week.clone(), json!(true));
                }
            }
        }

        let mut backfill_out = serde_json::Map::new();
        if let Some(p) = p_state {
            let mut sorted_inodes: Vec<u64> = p.backfill.keys().copied().collect();
            sorted_inodes.sort();
            for inode in sorted_inodes {
                let entry = &p.backfill[&inode];
                let mut b_obj = serde_json::Map::new();
                b_obj.insert("offset".to_string(), json!(entry.offset));
                b_obj.insert("size".to_string(), json!(entry.size));
                b_obj.insert("done".to_string(), json!(entry.done));
                b_obj.insert("discarding".to_string(), json!(entry.discarding));
                backfill_out.insert(inode.to_string(), Value::Object(b_obj));
            }
        }

        let mut provider_obj = serde_json::Map::new();
        provider_obj.insert("v".to_string(), json!(SCHEMA_VERSION));
        provider_obj.insert("days".to_string(), Value::Object(days_out));
        provider_obj.insert("weeks".to_string(), Value::Object(weeks_out));
        provider_obj.insert("backfill".to_string(), Value::Object(backfill_out));

        root.insert(provider.to_string(), Value::Object(provider_obj));
    }

    Value::Object(root)
}

fn atomic_write_json(path: &Path, payload: &Value) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let unique_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("max-tracker.json");
    let temp_path = parent.join(format!(".{}.{}.tmp", base_name, unique_ts));

    let json_bytes = serde_json::to_vec(payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&json_bytes)?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
    }

    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    if let Err(e) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    Ok(())
}

fn compute_provider_merged_days(
    days_opt: Option<&HashMap<String, DayRecord>>,
) -> HashMap<String, DayDisplayRecord> {
    let Some(days) = days_opt else {
        return HashMap::new();
    };

    let mut rankable = HashMap::new();
    for (day, record) in days {
        if record.act && record.loaded_lvl.is_none() {
            rankable.insert(day.clone(), record.vol);
        }
    }
    let levels = volume_levels(&rankable);

    let mut merged = HashMap::with_capacity(days.len());
    for (day, record) in days {
        let lvl = if !record.act {
            None
        } else if let Some(l) = record.loaded_lvl {
            Some(l as i32)
        } else {
            Some(*levels.get(day).unwrap_or(&0) as i32)
        };

        merged.insert(
            day.clone(),
            DayDisplayRecord {
                pct: record.pct,
                lvl,
            },
        );
    }
    merged
}

// ---------------------------------------------------------------------------
// MaxTrackerStore
// ---------------------------------------------------------------------------

pub struct MaxTrackerStore {
    path: PathBuf,
    state: Mutex<TrackerInternalState>,
    load_error: bool,
    pending: Mutex<HashMap<u64, Vec<u8>>>,
    last_codex_tokens: Mutex<HashMap<u64, u64>>,
}

impl MaxTrackerStore {
    pub fn new(path: PathBuf) -> Self {
        let (state, load_error) = load_file(&path);
        Self {
            path,
            state: Mutex::new(state),
            load_error,
            pending: Mutex::new(HashMap::new()),
            last_codex_tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_load_error(&self) -> bool {
        self.load_error
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn observe_quota(
        &self,
        provider: &str,
        session_pct: Option<f64>,
        week_pct: Option<f64>,
        now_sec: u64,
    ) -> bool {
        let date_str = local_date_from_epoch_secs(now_sec);
        self.observe_quota_on_date(provider, session_pct, week_pct, &date_str)
    }

    pub fn observe_quota_on_date(
        &self,
        provider: &str,
        session_pct: Option<f64>,
        week_pct: Option<f64>,
        date_str: &str,
    ) -> bool {
        if provider != "claude" && provider != "codex" {
            return false;
        }
        if date_to_days(date_str).is_none() {
            return false;
        }

        let mut state = self.state.lock().unwrap();
        let p_state = match state.providers.get_mut(provider) {
            Some(p) => p,
            None => return false,
        };

        let day = p_state.days.entry(date_str.to_string()).or_default();
        let mut changed = false;

        if let Some(sess) = session_pct {
            if sess.is_finite() && (0.0..=100.0).contains(&sess) {
                let rounded = round_day_pct(sess);
                match day.pct {
                    None => {
                        day.pct = Some(rounded);
                        changed = true;
                    }
                    Some(curr) => {
                        if rounded > curr {
                            day.pct = Some(rounded);
                            changed = true;
                        }
                    }
                }
                if sess >= 100.0 {
                    day.pct = Some(100);
                    day.maxed = true;
                    changed = true;
                }
            }
        }

        if let Some(week) = week_pct {
            if week.is_finite() && (0.0..=100.0).contains(&week) {
                if week >= 100.0 {
                    day.pct = Some(100);
                    day.maxed = true;
                    if let Some(wkey) = week_key(date_str) {
                        p_state.weeks.insert(wkey, true);
                    }
                    changed = true;
                }
            }
        }

        changed
    }

    pub fn observe_volume(&self, provider: &str, date_ymd: &str, tokens: u64) {
        if provider != "claude" && provider != "codex" || tokens == 0 {
            return;
        }
        if date_to_days(date_ymd).is_none() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if let Some(p_state) = state.providers.get_mut(provider) {
            let day = p_state.days.entry(date_ymd.to_string()).or_default();
            day.act = true;
            day.vol = day.vol.saturating_add(tokens);
            day.loaded_lvl = None;
        }
    }

    pub fn recompute_levels(&self, provider: &str) -> HashMap<String, u8> {
        let state = self.state.lock().unwrap();
        let mut rankable = HashMap::new();
        if let Some(p_state) = state.providers.get(provider) {
            for (day, record) in &p_state.days {
                if record.act && record.loaded_lvl.is_none() {
                    rankable.insert(day.clone(), record.vol);
                }
            }
        }
        volume_levels(&rankable)
    }

    pub fn save(&self, today: Option<&str>) -> io::Result<()> {
        if self.load_error {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} was unreadable at startup; refusing to overwrite it",
                    self.path.display()
                ),
            ));
        }

        let payload = {
            let mut state = self.state.lock().unwrap();
            prune_retention(&mut state, today);
            build_persisted_payload(&state)
        };

        atomic_write_json(&self.path, &payload)
    }

    pub fn snapshot(&self, now_sec: u64) -> Value {
        self.snapshot_with_plans(now_sec, &HashMap::new())
    }

    pub fn snapshot_with_plans(&self, now_sec: u64, plans: &HashMap<String, String>) -> Value {
        let date_str = local_date_from_epoch_secs(now_sec);
        self.snapshot_on_date(&date_str, plans)
    }

    pub fn snapshot_on_date(&self, today: &str, plans: &HashMap<String, String>) -> Value {
        let state_snapshot = {
            let state = self.state.lock().unwrap();
            state.clone()
        };
        build_payload(&state_snapshot, today, plans)
    }

    // -----------------------------------------------------------------------
    // Backfill
    // -----------------------------------------------------------------------

    pub fn backfill_step(&self, claude_dir: &Path, codex_dir: &Path) -> bool {
        let cutoff_ts = current_month_start_secs();
        self.backfill_step_with_budget(claude_dir, codex_dir, MAX_BUDGET_BYTES, Some(cutoff_ts))
    }

    pub fn backfill_step_with_budget(
        &self,
        claude_dir: &Path,
        codex_dir: &Path,
        budget_bytes: usize,
        cutoff_ts: Option<u64>,
    ) -> bool {
        let codex_more = self.advance_one_file(
            "codex",
            codex_dir,
            "rollout-",
            budget_bytes,
            None,
        );
        let claude_more = self.advance_one_file(
            "claude",
            claude_dir,
            ".jsonl",
            budget_bytes,
            cutoff_ts,
        );
        codex_more || claude_more
    }

    fn advance_one_file(
        &self,
        provider: &str,
        root: &Path,
        pattern_suffix: &str,
        budget_bytes: usize,
        cutoff_ts: Option<u64>,
    ) -> bool {
        if !root.is_dir() {
            return false;
        }

        let mut paths = Vec::new();
        collect_files_matching(root, pattern_suffix, &mut paths);
        paths.sort();

        let mut chosen = None;

        for path in &paths {
            let info = match fs::metadata(path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let f_key = file_path_key(path);

            let mtime_sec = info
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let deferred_to_live = match cutoff_ts {
                Some(cutoff) => {
                    if mtime_sec >= cutoff {
                        let state = self.state.lock().unwrap();
                        let existing = state
                            .providers
                            .get(provider)
                            .and_then(|p| p.backfill.get(&f_key));
                        existing.is_none() || existing.map_or(false, |e| e.done)
                    } else {
                        false
                    }
                }
                None => false,
            };

            if deferred_to_live {
                let mut state = self.state.lock().unwrap();
                if let Some(p_state) = state.providers.get_mut(provider) {
                    let existing = p_state.backfill.get(&f_key);
                    if existing.is_none() || existing.map_or(false, |e| e.done) {
                        p_state.backfill.insert(
                            f_key,
                            BackfillEntry {
                                offset: info.len(),
                                size: info.len(),
                                done: true,
                                discarding: false,
                            },
                        );
                        self.pending.lock().unwrap().remove(&f_key);
                    }
                }
                continue;
            }

            if chosen.is_some() {
                continue;
            }

            let state = self.state.lock().unwrap();
            let entry = state
                .providers
                .get(provider)
                .and_then(|p| p.backfill.get(&f_key))
                .cloned();

            let is_fully_drained = entry
                .as_ref()
                .map_or(false, |e| e.done && e.size == info.len());

            if is_fully_drained {
                continue;
            }

            chosen = Some((path.clone(), info, entry, f_key));
        }

        let (path, info, entry, f_key) = match chosen {
            Some(c) => c,
            None => return false,
        };

        let (start_offset, discarding) = match entry {
            None => {
                self.pending.lock().unwrap().remove(&f_key);
                (0, false)
            }
            Some(e) => {
                if e.size > info.len() {
                    self.pending.lock().unwrap().remove(&f_key);
                    (0, false)
                } else {
                    (e.offset, e.discarding)
                }
            }
        };

        let pending_buf = self
            .pending
            .lock()
            .unwrap()
            .get(&f_key)
            .cloned()
            .unwrap_or_default();

        let (new_offset, new_pending, still_discarding, dangling_eof) = self.drain_file(
            provider,
            &path,
            f_key,
            start_offset,
            budget_bytes,
            pending_buf,
            discarding,
            cutoff_ts,
        );

        let done = dangling_eof || new_offset >= info.len();

        {
            let mut state = self.state.lock().unwrap();
            if let Some(p_state) = state.providers.get_mut(provider) {
                p_state.backfill.insert(
                    f_key,
                    BackfillEntry {
                        offset: new_offset,
                        size: info.len(),
                        done,
                        discarding: still_discarding,
                    },
                );
            }
        }

        if !new_pending.is_empty() {
            self.pending.lock().unwrap().insert(f_key, new_pending);
        } else {
            self.pending.lock().unwrap().remove(&f_key);
        }

        if !done {
            return true;
        }

        let state = self.state.lock().unwrap();
        let p_state = match state.providers.get(provider) {
            Some(p) => p,
            None => return false,
        };

        for other_path in &paths {
            if *other_path == path {
                continue;
            }
            let other_info = match fs::metadata(other_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let other_key = file_path_key(other_path);
            let other_entry = p_state.backfill.get(&other_key);
            let is_drained = other_entry.map_or(false, |e| e.done && e.size == other_info.len());
            if !is_drained {
                return true;
            }
        }

        false
    }

    #[allow(clippy::too_many_arguments)]
    fn drain_file(
        &self,
        provider: &str,
        path: &Path,
        f_key: u64,
        start_offset: u64,
        budget_bytes: usize,
        pending_buf: Vec<u8>,
        mut discarding: bool,
        cutoff_ts: Option<u64>,
    ) -> (u64, Vec<u8>, bool, bool) {
        let mut offset = start_offset;
        let seek_pos = start_offset + if discarding { 0 } else { pending_buf.len() as u64 };
        let mut records_seen = 0;
        let mut bytes_read = 0;
        let mut buf = if discarding { Vec::new() } else { pending_buf };
        let mut hit_eof = false;

        let mut file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return (start_offset, buf, discarding, false),
        };

        if file.seek(SeekFrom::Start(seek_pos)).is_err() {
            return (start_offset, buf, discarding, false);
        }

        while records_seen < MAX_RECORDS_PER_STEP {
            if discarding {
                if bytes_read >= budget_bytes {
                    break;
                }
                let to_read = (budget_bytes - bytes_read).min(BLOCK_BYTES);
                let mut chunk = vec![0u8; to_read];
                let n = match file.read(&mut chunk) {
                    Ok(n) => n,
                    Err(_) => break,
                };
                if n == 0 {
                    hit_eof = true;
                    break;
                }
                chunk.truncate(n);
                bytes_read += n;

                if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
                    discarding = false;
                    records_seen += 1;
                    offset += (pos + 1) as u64;
                    buf = chunk[pos + 1..].to_vec();
                } else {
                    offset += n as u64;
                }
                continue;
            }

            if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let raw_line = buf[..pos].to_vec();
                buf = buf[pos + 1..].to_vec();
                offset += (pos + 1) as u64;
                records_seen += 1;

                if !raw_line.is_empty() {
                    self.handle_backfill_event(provider, &raw_line, f_key, cutoff_ts);
                }
                continue;
            }

            if buf.len() > MAX_LINE_BYTES {
                discarding = true;
                offset += buf.len() as u64;
                buf.clear();
                continue;
            }

            if bytes_read >= budget_bytes {
                break;
            }
            let to_read = (budget_bytes - bytes_read).min(BLOCK_BYTES);
            let mut chunk = vec![0u8; to_read];
            let n = match file.read(&mut chunk) {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                hit_eof = true;
                break;
            }
            chunk.truncate(n);
            bytes_read += n;
            buf.extend_from_slice(&chunk);
        }

        if hit_eof && !discarding && !buf.is_empty() {
            (offset, Vec::new(), false, true)
        } else {
            (offset, if discarding { Vec::new() } else { buf }, discarding, false)
        }
    }

    fn handle_backfill_event(
        &self,
        provider: &str,
        raw_line: &[u8],
        f_key: u64,
        cutoff_ts: Option<u64>,
    ) {
        if provider == "codex" {
            let Ok(val) = serde_json::from_slice::<Value>(raw_line) else {
                return;
            };
            if val.get("type").and_then(Value::as_str) != Some("event_msg") {
                return;
            }
            let Some(payload) = val.get("payload") else {
                return;
            };
            if payload.get("type").and_then(Value::as_str) != Some("token_count") {
                return;
            }

            let ts_raw = val.get("timestamp").and_then(Value::as_str);
            let local_date = match ts_raw.and_then(|s| crate::whatsapp::gateway::rfc3339_ms(Some(s))) {
                Some(ms) => local_date_from_epoch_secs((ms / 1000).max(0) as u64),
                None => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    local_date_from_epoch_secs(now)
                }
            };

            if let Some(rate_limits) = payload.get("rate_limits").and_then(Value::as_object) {
                let limit_name = rate_limits.get("limit_name").and_then(Value::as_str).unwrap_or("");
                if limit_name.is_empty() {
                    let mut found_any = false;
                    for window_key in &["primary", "secondary"] {
                        if let Some(w_obj) = rate_limits.get(*window_key).and_then(Value::as_object) {
                            let pct_opt = w_obj.get("used_percent").and_then(Value::as_f64);
                            let win_opt = w_obj.get("window_minutes").and_then(Value::as_f64);
                            if let (Some(pct), Some(win)) = (pct_opt, win_opt) {
                                if pct.is_finite() && (0.0..=100.0).contains(&pct) && win.is_finite() && win > 0.0 {
                                    found_any = true;
                                    let rounded = round_day_pct(pct);
                                    if win <= GENERAL_WINDOW_MINUTES {
                                        let mut state = self.state.lock().unwrap();
                                        if let Some(p) = state.providers.get_mut("codex") {
                                            let day = p.days.entry(local_date.clone()).or_default();
                                            match day.pct {
                                                None => day.pct = Some(rounded),
                                                Some(curr) => {
                                                    if rounded > curr {
                                                        day.pct = Some(rounded);
                                                    }
                                                }
                                            }
                                        }
                                    } else if pct >= 100.0 {
                                        if let Some(wk) = week_key(&local_date) {
                                            let mut state = self.state.lock().unwrap();
                                            if let Some(p) = state.providers.get_mut("codex") {
                                                p.weeks.insert(wk, true);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if found_any {
                        let mut state = self.state.lock().unwrap();
                        if let Some(p) = state.providers.get_mut("codex") {
                            p.days.entry(local_date.clone()).or_default().act = true;
                        }
                    }
                }
            }

            if let Some(info) = payload.get("info").and_then(Value::as_object) {
                let total_tokens_opt = info
                    .get("total_token_usage")
                    .and_then(|u| u.get("total_tokens"))
                    .and_then(Value::as_u64)
                    .or_else(|| {
                        info.get("last_token_usage")
                            .and_then(|u| u.get("total_tokens"))
                            .and_then(Value::as_u64)
                    });
                if let Some(curr_total) = total_tokens_opt {
                    let mut last_tokens = self.last_codex_tokens.lock().unwrap();
                    let prev = last_tokens.get(&f_key).copied().unwrap_or(0);
                    if curr_total > prev {
                        let delta = curr_total - prev;
                        drop(last_tokens);
                        self.observe_volume("codex", &local_date, delta);
                        self.last_codex_tokens.lock().unwrap().insert(f_key, curr_total);
                    } else {
                        last_tokens.insert(f_key, curr_total);
                    }
                }
            }
        } else if provider == "claude" {
            let Ok(val) = serde_json::from_slice::<Value>(raw_line) else {
                return;
            };
            let (Some(ts_raw), Some(msg)) = (val.get("timestamp").and_then(Value::as_str), val.get("message")) else {
                return;
            };
            let Some(usage) = msg.get("usage") else {
                return;
            };
            let Some(ms) = crate::whatsapp::gateway::rfc3339_ms(Some(ts_raw)) else {
                return;
            };
            let event_sec = (ms / 1000).max(0) as u64;

            if let Some(cutoff) = cutoff_ts {
                if event_sec >= cutoff {
                    return;
                }
            }

            let local_date = local_date_from_epoch_secs(event_sec);
            let in_t = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
            let out_t = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
            let c_create = usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let c_read = usage
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let tokens = in_t + out_t + c_create + c_read;
            if tokens > 0 {
                self.observe_volume("claude", &local_date, tokens);
            }
        }
    }
}

fn collect_files_matching(dir: &Path, pattern_suffix: &str, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_matching(&path, pattern_suffix, out);
        } else if path.is_file() {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if file_name.ends_with(pattern_suffix)
                || (pattern_suffix.starts_with("rollout-")
                    && file_name.starts_with("rollout-")
                    && file_name.ends_with(".jsonl"))
            {
                out.push(path);
            }
        }
    }
}

pub fn build_payload(
    state: &TrackerInternalState,
    today: &str,
    plans: &HashMap<String, String>,
) -> Value {
    let mut active_dates = HashSet::new();
    for provider in &["claude", "codex"] {
        if let Some(p_state) = state.providers.get(*provider) {
            for (day, record) in &p_state.days {
                if record.act {
                    active_dates.insert(day.clone());
                }
            }
        }
    }

    let coding_streak_days = if active_dates.is_empty() {
        Value::Null
    } else {
        json!(coding_streak(&active_dates, today))
    };

    let w_keys = window_week_keys(today, WINDOW_WEEKS);
    let this_week = week_key(today).unwrap_or_default();

    let mut payload = json!({
        "v": SCHEMA_VERSION,
        "weeks": WINDOW_WEEKS,
        "stale": state.stale,
        "codingStreakDays": coding_streak_days,
    });

    for provider in &["claude", "codex"] {
        let p_state = state.providers.get(*provider);
        let days = p_state.map(|p| &p.days);
        let weeks_maxed = p_state.map(|p| &p.weeks);

        let merged_days = compute_provider_merged_days(days);
        let window_days = dense_window(today, WINDOW_WEEKS, &merged_days);

        let window_maxed: Vec<i32> = w_keys
            .iter()
            .map(|key| {
                if weeks_maxed.and_then(|w| w.get(key)).copied().unwrap_or(false) {
                    1
                } else {
                    0
                }
            })
            .collect();

        let real_pcts: Vec<i32> = window_days
            .iter()
            .map(|pair| pair[0])
            .filter(|&pct| pct != -1)
            .collect();

        let avg_peak_pct = if real_pcts.is_empty() {
            Value::Null
        } else {
            let sum: f64 = real_pcts.iter().map(|&p| p as f64).sum();
            let avg = sum / real_pcts.len() as f64;
            json!((avg * 10.0).round() / 10.0)
        };

        let max_weeks_streak_val = weeks_maxed
            .map(|w| max_weeks_streak(w, &this_week))
            .unwrap_or(0);

        let max_weeks_count = weeks_maxed
            .map(|w| w.values().filter(|&&v| v).count() as u32)
            .unwrap_or(0)
            .min(AGGREGATE_MAX);

        let max_days_count = (merged_days
            .values()
            .filter(|r| r.pct == Some(100))
            .count() as u32)
            .min(AGGREGATE_MAX);

        let mut provider_obj = serde_json::Map::new();
        if let Some(plan_raw) = plans.get(*provider) {
            if let Some(label) = map_plan_label(plan_raw) {
                provider_obj.insert("planLabel".to_string(), json!(label));
            }
        }

        provider_obj.insert("avgPeakPct".to_string(), avg_peak_pct);
        provider_obj.insert("maxWeeksStreak".to_string(), json!(max_weeks_streak_val));
        provider_obj.insert("maxWeeks".to_string(), json!(max_weeks_count));
        provider_obj.insert("maxDays".to_string(), json!(max_days_count));
        provider_obj.insert("weekMaxed".to_string(), json!(window_maxed));
        provider_obj.insert("days".to_string(), json!(window_days));

        if let Value::Object(ref mut map) = payload {
            map.insert(provider.to_string(), Value::Object(provider_obj));
        }
    }

    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::thread;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(name: &str) -> Self {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("lumen-max-test-{}-{}", name, ts));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_volume_levels_bands_and_ties() {
        let mut vols = HashMap::new();
        vols.insert("a".into(), 10);
        vols.insert("b".into(), 20);
        vols.insert("c".into(), 30);
        vols.insert("d".into(), 40);
        vols.insert("e".into(), 50);
        vols.insert("f".into(), 60);

        let lvls = volume_levels(&vols);
        assert_eq!(lvls["a"], 0);
        assert_eq!(lvls["b"], 0);
        assert_eq!(lvls["c"], 1);
        assert_eq!(lvls["d"], 1);
        assert_eq!(lvls["e"], 2);
        assert_eq!(lvls["f"], 2);

        // Ties
        let mut ties = HashMap::new();
        ties.insert("a".into(), 50);
        ties.insert("b".into(), 50);
        ties.insert("c".into(), 50);
        ties.insert("d".into(), 5);
        ties.insert("e".into(), 500);
        let lvls_ties = volume_levels(&ties);
        assert_eq!(lvls_ties["a"], lvls_ties["b"]);
        assert_eq!(lvls_ties["b"], lvls_ties["c"]);

        // Single distinct nonzero
        let mut single = HashMap::new();
        single.insert("a".into(), 7);
        single.insert("b".into(), 7);
        let lvls_single = volume_levels(&single);
        assert_eq!(lvls_single["a"], 0);
        assert_eq!(lvls_single["b"], 0);

        // Zero volume days
        let mut with_zero = HashMap::new();
        with_zero.insert("a".into(), 0);
        with_zero.insert("b".into(), 10);
        with_zero.insert("c".into(), 20);
        with_zero.insert("d".into(), 30);
        let lvls_zero = volume_levels(&with_zero);
        assert_eq!(lvls_zero["a"], 0);
        assert_eq!(lvls_zero["b"], 0);
        assert_eq!(lvls_zero["c"], 1);
        assert_eq!(lvls_zero["d"], 2);
    }

    #[test]
    fn test_coding_streak_rules() {
        let mut active = HashSet::new();
        active.insert("2026-08-10".into());
        active.insert("2026-08-11".into());
        active.insert("2026-08-12".into());
        assert_eq!(coding_streak(&active, "2026-08-12"), 3);

        // Grace day when today has no activity yet
        active.remove("2026-08-12");
        assert_eq!(coding_streak(&active, "2026-08-12"), 2);

        // Gap
        let mut gap = HashSet::new();
        gap.insert("2026-08-09".into());
        assert_eq!(coding_streak(&gap, "2026-08-12"), 0);

        // Across month boundary
        let mut month_cross = HashSet::new();
        month_cross.insert("2026-01-30".into());
        month_cross.insert("2026-01-31".into());
        month_cross.insert("2026-02-01".into());
        month_cross.insert("2026-02-02".into());
        assert_eq!(coding_streak(&month_cross, "2026-02-02"), 4);

        // Empty
        assert_eq!(coding_streak(&HashSet::new(), "2026-08-12"), 0);
    }

    #[test]
    fn test_week_key_and_iso_math() {
        assert_eq!(week_key("2026-08-12"), Some("2026-W33".into()));
        assert_eq!(week_key("2025-12-29"), Some("2026-W01".into()));
        assert_eq!(week_key("2026-12-28"), Some("2026-W53".into()));
        assert_eq!(week_key("2027-01-01"), Some("2026-W53".into()));
        assert_eq!(week_key("2027-01-04"), Some("2027-W01".into()));

        // Roundtrip monday
        let m_days = week_key_to_monday_days("2026-W33").unwrap();
        assert_eq!(days_to_date(m_days), "2026-08-10");
    }

    #[test]
    fn test_max_weeks_streak_ignores_current_week() {
        let mut weeks = HashMap::new();
        weeks.insert("2026-W33".into(), true); // Current week
        weeks.insert("2026-W32".into(), true);
        weeks.insert("2026-W31".into(), true);
        weeks.insert("2026-W30".into(), false);

        assert_eq!(max_weeks_streak(&weeks, "2026-W33"), 2);

        let mut current_only = HashMap::new();
        current_only.insert("2026-W33".into(), true);
        current_only.insert("2026-W32".into(), false);
        assert_eq!(max_weeks_streak(&current_only, "2026-W33"), 0);

        // Week 53 boundary
        let mut w53 = HashMap::new();
        w53.insert("2026-W53".into(), true);
        w53.insert("2026-W52".into(), true);
        w53.insert("2026-W51".into(), false);
        assert_eq!(max_weeks_streak(&w53, "2027-W01"), 2);
    }

    #[test]
    fn test_round_day_pct_invariants() {
        assert_eq!(round_day_pct(15.5), 16);
        assert_eq!(round_day_pct(99.6), 99);
        assert_eq!(round_day_pct(99.96), 99);
        assert_eq!(round_day_pct(100.0), 100);
        assert_eq!(round_day_pct(105.0), 100);
        assert_eq!(round_day_pct(0.5), 1);
        assert_eq!(round_day_pct(0.4), 0);
        assert_eq!(round_day_pct(0.0), 0);
        assert_eq!(round_day_pct(-5.0), 0);
    }

    #[test]
    fn test_dense_window_and_snapshot_contract() {
        let mut per_day = HashMap::new();
        per_day.insert(
            "2026-08-12".into(),
            DayDisplayRecord {
                pct: Some(40),
                lvl: Some(1),
            },
        );
        per_day.insert(
            "2026-08-13".into(),
            DayDisplayRecord {
                pct: Some(99),
                lvl: Some(2),
            },
        );

        let win = dense_window("2026-08-12", WINDOW_WEEKS, &per_day);
        assert_eq!(win.len(), WINDOW_DAYS);
        // Index 135 is Wed 2026-08-12
        assert_eq!(win[135], [40, 1]);
        // Future padding
        assert_eq!(win[136], [-1, -1]);
        assert_eq!(win[137], [-1, -1]);
        assert_eq!(win[138], [-1, -1]);
        assert_eq!(win[139], [-1, -1]);
    }

    #[test]
    fn test_store_save_load_and_atomic_quarantine() {
        let td = TestDir::new("atomic");
        let path = td.0.join("max-tracker.json");

        let store = MaxTrackerStore::new(path.clone());
        assert!(!store.is_load_error());

        store.observe_volume("claude", "2026-08-01", 100);
        store.observe_quota_on_date("claude", Some(55.0), None, "2026-08-01");
        store.save(Some("2026-08-01")).unwrap();
        assert!(path.exists());

        // Reload clean
        let reloaded = MaxTrackerStore::new(path.clone());
        let snap = reloaded.snapshot_on_date("2026-08-01", &HashMap::new());
        assert_eq!(snap["v"], 1);
        assert_eq!(snap["codingStreakDays"], 1);
        assert_eq!(snap["claude"]["avgPeakPct"], 55.0);

        // Corrupt file -> quarantine
        fs::write(&path, b"invalid json corrupt bytes").unwrap();
        let corrupted_store = MaxTrackerStore::new(path.clone());
        assert!(!path.exists()); // moved to quarantine
        let quarantined = fs::read_dir(&td.0)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map_or(false, |s| s.starts_with("max-tracker.json.corrupt-"))
            })
            .count();
        assert_eq!(quarantined, 1);
        assert_eq!(
            corrupted_store
                .snapshot_on_date("2026-08-01", &HashMap::new())["codingStreakDays"],
            Value::Null
        );
    }

    #[test]
    fn test_retention_pruning_on_save() {
        let td = TestDir::new("retention");
        let path = td.0.join("max-tracker.json");
        let store = MaxTrackerStore::new(path.clone());

        let kept_day = "2026-08-10";
        let pruned_day = "2025-06-01"; // > 400 days from 2026-08-10
        store.observe_volume("claude", kept_day, 500);
        store.observe_volume("claude", pruned_day, 500);
        store.save(Some("2026-08-10")).unwrap();

        let reloaded = MaxTrackerStore::new(path);
        let state = reloaded.state.lock().unwrap();
        assert!(state.providers["claude"].days.contains_key(kept_day));
        assert!(!state.providers["claude"].days.contains_key(pruned_day));
    }

    #[test]
    fn test_privacy_no_file_paths_in_backfill() {
        let td = TestDir::new("privacy");
        let path = td.0.join("max-tracker.json");
        let store = MaxTrackerStore::new(path.clone());

        let codex_dir = td.0.join("secret_sessions");
        let claude_dir = td.0.join("secret_claude");
        fs::create_dir_all(&codex_dir).unwrap();
        fs::create_dir_all(&claude_dir).unwrap();

        let rollout_path = codex_dir.join("rollout-test.jsonl");
        fs::write(
            &rollout_path,
            json!({
                "type": "event_msg",
                "timestamp": "2026-08-01T10:00:00Z",
                "payload": {
                    "type": "token_count",
                    "rate_limits": {
                        "primary": {
                            "used_percent": 42.0,
                            "window_minutes": 300
                        }
                    }
                }
            })
            .to_string()
                + "\n",
        )
        .unwrap();

        let more = store.backfill_step_with_budget(&claude_dir, &codex_dir, 1_000_000, None);
        assert!(!more);

        store.save(Some("2026-08-01")).unwrap();
        let saved_text = fs::read_to_string(&path).unwrap();
        assert!(!saved_text.contains("rollout-test"));
        assert!(!saved_text.contains("secret_sessions"));
        assert!(!saved_text.contains("secret_claude"));
        // Key in backfill must be digits
        let saved_val: Value = serde_json::from_str(&saved_text).unwrap();
        let backfill_obj = saved_val["codex"]["backfill"].as_object().unwrap();
        for k in backfill_obj.keys() {
            assert!(k.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn test_authoritative_loaded_level_vs_fresh_volume() {
        let td = TestDir::new("authoritative");
        let path = td.0.join("max-tracker.json");
        let store = MaxTrackerStore::new(path.clone());

        store.observe_volume("claude", "2026-08-01", 100);
        store.observe_volume("claude", "2026-08-02", 500);
        store.observe_volume("claude", "2026-08-03", 1000);
        store.save(Some("2026-08-03")).unwrap();

        let reloaded = MaxTrackerStore::new(path.clone());
        {
            let s = reloaded.state.lock().unwrap();
            assert_eq!(s.providers["claude"].days["2026-08-01"].loaded_lvl, Some(0));
            assert_eq!(s.providers["claude"].days["2026-08-02"].loaded_lvl, Some(1));
            assert_eq!(s.providers["claude"].days["2026-08-03"].loaded_lvl, Some(2));
        }

        // Fresh volume on day 1 clears loaded level and re-ranks
        reloaded.observe_volume("claude", "2026-08-01", 2000);
        {
            let s = reloaded.state.lock().unwrap();
            assert_eq!(s.providers["claude"].days["2026-08-01"].loaded_lvl, None);
        }
    }

    #[test]
    fn test_concurrent_observe_and_snapshot() {
        let td = TestDir::new("concurrency");
        let path = td.0.join("max-tracker.json");
        let store = Arc::new(MaxTrackerStore::new(path));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let s = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                for _ in 0..500 {
                    s.observe_volume("claude", "2026-08-01", 2);
                    let _ = s.snapshot_on_date("2026-08-01", &HashMap::new());
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let state = store.state.lock().unwrap();
        assert_eq!(state.providers["claude"].days["2026-08-01"].vol, 4000);
    }
}
