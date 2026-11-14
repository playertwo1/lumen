//! Modelo de tarefas e regras de agenda em paridade com a F11 da Segunda Mente.
//! O esquema JSON usa `snake_case` idêntico ao `todo.json` do Android para facilitar o sync (L6).

use serde::{Deserialize, Serialize};

pub const TODO_SCHEMA_VERSION: u32 = 2;
pub const TODO_TIME_ZONE: &str = "America/Sao_Paulo";
pub const TODO_DEFAULT_ALERT_MINUTES: i32 = 20;
pub const TODO_ALARM_SNOOZE_MINUTES: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoRepeatFrequency {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSubtask {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoRecurrence {
    pub frequency: TodoRepeatFrequency,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weekdays: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_month: Option<u8>,
}

impl TodoRecurrence {
    pub fn validate(&self) -> Result<(), String> {
        match self.frequency {
            TodoRepeatFrequency::Daily => Ok(()),
            TodoRepeatFrequency::Weekly => {
                if self.weekdays.is_empty() || self.weekdays.iter().any(|&d| !(1..=7).contains(&d)) {
                    return Err("Recorrência semanal exige dias válidos (1..7).".into());
                }
                Ok(())
            }
            TodoRepeatFrequency::Monthly => match self.day_of_month {
                Some(day) if (1..=31).contains(&day) => Ok(()),
                _ => Err("Recorrência mensal exige dia do mês entre 1 e 31.".into()),
            },
        }
    }

    pub fn normalized(&self) -> Result<Self, String> {
        self.validate()?;
        match self.frequency {
            TodoRepeatFrequency::Daily => Ok(Self {
                frequency: TodoRepeatFrequency::Daily,
                weekdays: Vec::new(),
                day_of_month: None,
            }),
            TodoRepeatFrequency::Weekly => {
                let mut days = self.weekdays.clone();
                days.sort_unstable();
                days.dedup();
                Ok(Self {
                    frequency: TodoRepeatFrequency::Weekly,
                    weekdays: days,
                    day_of_month: None,
                })
            }
            TodoRepeatFrequency::Monthly => Ok(Self {
                frequency: TodoRepeatFrequency::Monthly,
                weekdays: Vec::new(),
                day_of_month: self.day_of_month,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSource {
    pub session_id: String,
    pub item_id: String,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub due: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub due_time: Option<String>,
    #[serde(default = "default_time_zone")]
    pub time_zone: String,
    #[serde(default = "default_alert_minutes")]
    pub alert_minutes: i32,
    #[serde(default)]
    pub priority: TodoPriority,
    #[serde(default)]
    pub subtasks: Vec<TodoSubtask>,
    #[serde(default)]
    pub recurrence: Option<TodoRecurrence>,
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub done: bool,
    pub created_at: String,
    #[serde(default)]
    pub done_at: Option<String>,
    #[serde(default)]
    pub snoozed_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<TodoSource>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub source_stale: bool,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

fn default_time_zone() -> String {
    TODO_TIME_ZONE.to_string()
}

fn default_alert_minutes() -> i32 {
    TODO_DEFAULT_ALERT_MINUTES
}

fn default_version() -> u32 {
    1
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl TodoItem {
    pub fn is_tombstoned(&self) -> bool {
        self.deleted_at.as_deref().is_some_and(|s| !s.trim().is_empty())
    }

    pub fn has_alarm_time(&self) -> bool {
        self.due_date.as_deref().is_some_and(is_valid_date)
            && self.due_time.as_deref().is_some_and(is_valid_time)
    }

    pub fn sanitize(&mut self) {
        self.time_zone = known_time_zone(Some(&self.time_zone));
        if self.alert_minutes < 0 {
            self.alert_minutes = 0;
        }
        if self.version == 0 {
            self.version = 1;
        }
        if self.due_date.as_deref().is_some_and(|d| !is_valid_date(d)) {
            self.due_date = None;
            self.due_time = None;
        }
        if self.due_time.as_deref().is_some_and(|t| !is_valid_time(t)) || self.due_date.is_none() {
            self.due_time = None;
        }
        if !self.done {
            self.done_at = None;
        }
        if let Some(rec) = &self.recurrence {
            if let Ok(norm) = rec.normalized() {
                self.recurrence = Some(norm);
            } else {
                self.recurrence = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TodoDateShortcut {
    Today,
    Tomorrow,
    InThreeDays,
    NextMonday,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoShortcutSettings {
    pub enabled_date_shortcuts: Vec<TodoDateShortcut>,
    pub time_shortcuts_minutes: [u16; 3],
    pub default_alert_minutes: i32,
}

impl Default for TodoShortcutSettings {
    fn default() -> Self {
        Self {
            enabled_date_shortcuts: vec![
                TodoDateShortcut::Today,
                TodoDateShortcut::Tomorrow,
                TodoDateShortcut::InThreeDays,
                TodoDateShortcut::NextMonday,
            ],
            time_shortcuts_minutes: [540, 840, 1200],
            default_alert_minutes: TODO_DEFAULT_ALERT_MINUTES,
        }
    }
}

impl TodoShortcutSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.time_shortcuts_minutes.iter().any(|&m| m >= 1440) {
            return Err("Atalho de horário fora de 0..1439 minutos.".into());
        }
        if self.default_alert_minutes < 0 {
            return Err("Alerta padrão não pode ser negativo.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoProposalDateState {
    Unspecified,
    Explicit,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoProposalTimeState {
    Unspecified,
    Explicit,
    Ambiguous,
}

/// Sugestão de tarefa gerada por captura rápida ou IA antes da confirmação do dono.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoProposal {
    pub id: String,
    pub title: String,
    pub date_state: TodoProposalDateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_phrase: Option<String>,
    pub time_state: TodoProposalTimeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_phrase: Option<String>,
    #[serde(default)]
    pub priority: TodoPriority,
}

impl TodoProposal {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.title.trim().is_empty() {
            return Err("Proposta de tarefa precisa de id e título.".into());
        }
        match self.date_state {
            TodoProposalDateState::Explicit => {
                if !self.due_date.as_deref().is_some_and(is_valid_date) {
                    return Err("Data explícita inválida na proposta.".into());
                }
            }
            _ => {
                if self.due_date.is_some() {
                    return Err("Apenas propostas com data explícita podem carregar due_date.".into());
                }
            }
        }
        match self.time_state {
            TodoProposalTimeState::Explicit => {
                if !self.due_time.as_deref().is_some_and(is_valid_time)
                    || self.date_state == TodoProposalDateState::Unspecified
                {
                    return Err("Horário explícito inválido ou sem data na proposta.".into());
                }
            }
            _ => {
                if self.due_time.is_some() {
                    return Err("Apenas propostas com horário explícito podem carregar due_time.".into());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoBucket {
    Overdue,
    Today,
    Upcoming,
    NoDate,
    Done,
}

// ── Calendário civil e fuso (sem dependências extras) ────────────────────────

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 1 = segunda-feira .. 7 = domingo (mesmo contrato de `DateTime.weekday` do Dart).
pub fn weekday_from_days(days: i64) -> u8 {
    ((days + 3).rem_euclid(7) + 1) as u8
}

pub fn parse_date(value: &str) -> Option<(i64, i64, i64)> {
    let s = value.trim();
    if s.len() != 10 || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[5..7].parse().ok()?;
    let d: i64 = s[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=days_in_month(y, m)).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

pub fn is_valid_date(value: &str) -> bool {
    parse_date(value).is_some()
}

pub fn format_date(y: i64, m: i64, d: i64) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn parse_time(value: &str) -> Option<(i64, i64)> {
    let s = value.trim();
    if s.len() != 5 || s.as_bytes()[2] != b':' {
        return None;
    }
    let h: i64 = s[0..2].parse().ok()?;
    let m: i64 = s[3..5].parse().ok()?;
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) {
        return None;
    }
    Some((h, m))
}

pub fn is_valid_time(value: &str) -> bool {
    parse_time(value).is_some()
}

/// Fuso conhecido para agenda local; padrão `America/Sao_Paulo` (UTC-03:00 fixo desde 2019).
pub fn known_time_zone(raw: Option<&str>) -> String {
    let name = raw.map(str::trim).unwrap_or("");
    if tz_offset_minutes(name).is_some() {
        name.to_string()
    } else {
        TODO_TIME_ZONE.to_string()
    }
}

pub fn tz_offset_minutes(zone: &str) -> Option<i64> {
    match zone.trim() {
        "America/Sao_Paulo" | "America/Fortaleza" | "America/Recife" | "America/Bahia"
        | "America/Belem" | "America/Argentina/Buenos_Aires" => Some(-180),
        "America/Manaus" | "America/Cuiaba" | "America/Porto_Velho" | "America/Boa_Vista" => Some(-240),
        "America/Rio_Branco" => Some(-300),
        "America/Noronha" => Some(-120),
        "UTC" | "Etc/UTC" | "GMT" => Some(0),
        other => {
            // Aceita offset fixo "+HH:MM" / "-HH:MM" se vier de fora.
            let b = other.as_bytes();
            if b.len() == 6 && (b[0] == b'+' || b[0] == b'-') && b[3] == b':' {
                let sign = if b[0] == b'-' { -1 } else { 1 };
                let hh: i64 = other[1..3].parse().ok()?;
                let mm: i64 = other[4..6].parse().ok()?;
                if hh <= 14 && mm <= 59 {
                    return Some(sign * (hh * 60 + mm));
                }
            }
            None
        }
    }
}

pub fn effective_offset_minutes(zone: &str) -> i64 {
    tz_offset_minutes(zone).unwrap_or(-180)
}

pub fn local_parts_at(utc_ms: i64, zone: &str) -> (i64, i64, i64, i64, i64) {
    let offset_ms = effective_offset_minutes(zone) * 60_000;
    let local_secs = (utc_ms + offset_ms).div_euclid(1000);
    let days = local_secs.div_euclid(86_400);
    let tod = local_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, tod / 3600, (tod % 3600) / 60)
}

pub fn utc_ms_from_local(y: i64, m: i64, d: i64, hour: i64, min: i64, zone: &str) -> i64 {
    let days = days_from_civil(y, m, d);
    let local_secs = days * 86_400 + hour * 3600 + min * 60;
    let offset_secs = effective_offset_minutes(zone) * 60;
    (local_secs - offset_secs) * 1000
}

#[allow(dead_code)]
pub fn today_date_string(now_ms: i64, zone: &str) -> String {
    let (y, m, d, _, _) = local_parts_at(now_ms, zone);
    format_date(y, m, d)
}

pub fn date_for_shortcut(shortcut: TodoDateShortcut, now_ms: i64, zone: &str) -> String {
    let (y, m, d, _, _) = local_parts_at(now_ms, zone);
    let today_days = days_from_civil(y, m, d);
    let target_days = match shortcut {
        TodoDateShortcut::Today => today_days,
        TodoDateShortcut::Tomorrow => today_days + 1,
        TodoDateShortcut::InThreeDays => today_days + 3,
        TodoDateShortcut::NextMonday => {
            let wd = weekday_from_days(today_days) as i64; // 1=Mon .. 7=Sun
            let delta = if wd == 1 { 7 } else { (1 - wd + 7) % 7 };
            today_days + delta
        }
    };
    let (ny, nm, nd) = civil_from_days(target_days);
    format_date(ny, nm, nd)
}

/// Calcula o instante UTC em ms do próximo alarme (alerta `alert_minutes` antes,
/// ou horário exato `due` se o alerta já passou, ou `snoozed_until` se adiada).
/// Paridade exata com `TodoScheduleCalculator.alarmDateTimeFor`.
pub fn alarm_timestamp_ms_for(
    due_date: Option<&str>,
    due_time: Option<&str>,
    time_zone: &str,
    alert_minutes: i32,
    snoozed_until: Option<&str>,
    now_ms: i64,
) -> Option<i64> {
    let (y, m, d) = parse_date(due_date?)?;
    let (hh, mm) = parse_time(due_time?)?;
    if alert_minutes < 0 {
        return None;
    }
    if let Some(snooze_ms) = crate::whatsapp::gateway::rfc3339_ms(snoozed_until) {
        return (snooze_ms > now_ms).then_some(snooze_ms);
    }
    let due_ms = utc_ms_from_local(y, m, d, hh, mm, time_zone);
    let alarm_ms = due_ms - (alert_minutes as i64) * 60_000;
    if alarm_ms > now_ms {
        Some(alarm_ms)
    } else if due_ms > now_ms {
        Some(due_ms)
    } else {
        None
    }
}

pub fn next_occurrence_date(recurrence: &TodoRecurrence, after_date: &str) -> Option<String> {
    let (y, m, d) = parse_date(after_date)?;
    let after_days = days_from_civil(y, m, d);
    match recurrence.frequency {
        TodoRepeatFrequency::Daily => {
            let (ny, nm, nd) = civil_from_days(after_days + 1);
            Some(format_date(ny, nm, nd))
        }
        TodoRepeatFrequency::Weekly => {
            recurrence.validate().ok()?;
            for offset in 1..=7 {
                let candidate = after_days + offset;
                let wd = weekday_from_days(candidate);
                if recurrence.weekdays.contains(&wd) {
                    let (ny, nm, nd) = civil_from_days(candidate);
                    return Some(format_date(ny, nm, nd));
                }
            }
            None
        }
        TodoRepeatFrequency::Monthly => {
            recurrence.validate().ok()?;
            let target_day = recurrence.day_of_month? as i64;
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            let clamped = target_day.min(days_in_month(ny, nm));
            Some(format_date(ny, nm, clamped))
        }
    }
}

pub fn next_upcoming_occurrence_date(
    recurrence: &TodoRecurrence,
    after_date: &str,
    now_ms: i64,
    time_zone: &str,
    due_time: Option<&str>,
) -> Option<String> {
    let (ty, tm, td, hh, mm) = local_parts_at(now_ms, time_zone);
    let today_days = days_from_civil(ty, tm, td);
    let time_passed = due_time
        .and_then(parse_time)
        .is_some_and(|(th, tmin)| th * 60 + tmin <= hh * 60 + mm);

    let mut next = next_occurrence_date(recurrence, after_date)?;
    for _ in 0..4000 {
        let (ny, nm, nd) = parse_date(&next)?;
        let next_days = days_from_civil(ny, nm, nd);
        if next_days > today_days || (next_days == today_days && !time_passed) {
            return Some(next);
        }
        next = next_occurrence_date(recurrence, &next)?;
    }
    Some(next)
}

pub fn bucket_for(item: &TodoItem, now_ms: i64) -> TodoBucket {
    if item.done {
        return TodoBucket::Done;
    }
    let Some(due_date) = item.due_date.as_deref() else {
        return TodoBucket::NoDate;
    };
    let Some((dy, dm, dd)) = parse_date(due_date) else {
        return TodoBucket::NoDate;
    };
    let due_days = days_from_civil(dy, dm, dd);
    let (ty, tm, td, hh, mm) = local_parts_at(now_ms, &item.time_zone);
    let today_days = days_from_civil(ty, tm, td);
    if due_days < today_days {
        return TodoBucket::Overdue;
    }
    if due_days > today_days {
        return TodoBucket::Upcoming;
    }
    if let Some((th, tmin)) = item.due_time.as_deref().and_then(parse_time) {
        if th * 60 + tmin <= hh * 60 + mm {
            return TodoBucket::Overdue;
        }
    }
    TodoBucket::Today
}

pub fn fold_pt(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

pub fn search_todos<'a>(items: &'a [TodoItem], query: &str) -> Vec<&'a TodoItem> {
    let needle = fold_pt(query);
    if needle.is_empty() {
        return items.iter().collect();
    }
    items
        .iter()
        .filter(|item| {
            let subtasks = item
                .subtasks
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let excerpt = item
                .source
                .as_ref()
                .map(|s| s.excerpt.as_str())
                .unwrap_or("");
            let haystack = fold_pt(&format!("{} {} {} {}", item.text, item.due, excerpt, subtasks));
            haystack.contains(&needle)
        })
        .collect()
}

pub fn now_utc_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(due_date: Option<&str>, due_time: Option<&str>, done: bool) -> TodoItem {
        TodoItem {
            id: "t1".into(),
            text: "Comprar café".into(),
            due: "".into(),
            due_date: due_date.map(str::to_string),
            due_time: due_time.map(str::to_string),
            time_zone: TODO_TIME_ZONE.into(),
            alert_minutes: 20,
            priority: TodoPriority::Normal,
            subtasks: Vec::new(),
            recurrence: None,
            series_id: None,
            done,
            created_at: "2026-11-14T12:00:00Z".into(),
            done_at: None,
            snoozed_until: None,
            source: None,
            source_stale: false,
            version: 1,
            deleted_at: None,
        }
    }

    #[test]
    fn shortcuts_match_sao_paulo_calendar() {
        // 2026-11-14 é sábado; em São Paulo 10:00 local = 13:00Z.
        let now_ms = utc_ms_from_local(2026, 11, 14, 10, 0, TODO_TIME_ZONE);
        assert_eq!(date_for_shortcut(TodoDateShortcut::Today, now_ms, TODO_TIME_ZONE), "2026-11-14");
        assert_eq!(date_for_shortcut(TodoDateShortcut::Tomorrow, now_ms, TODO_TIME_ZONE), "2026-11-15");
        assert_eq!(date_for_shortcut(TodoDateShortcut::InThreeDays, now_ms, TODO_TIME_ZONE), "2026-11-17");
        assert_eq!(date_for_shortcut(TodoDateShortcut::NextMonday, now_ms, TODO_TIME_ZONE), "2026-11-16");

        // Na segunda-feira (2026-11-16), "Semana que vem" pula +7 dias para 2026-11-23.
        let mon_ms = utc_ms_from_local(2026, 11, 16, 9, 0, TODO_TIME_ZONE);
        assert_eq!(date_for_shortcut(TodoDateShortcut::NextMonday, mon_ms, TODO_TIME_ZONE), "2026-11-23");
    }

    #[test]
    fn alarm_calculation_handles_alert_due_fallback_and_snooze() {
        let now_ms = utc_ms_from_local(2026, 11, 14, 13, 45, TODO_TIME_ZONE);
        // 14:30 com alerta 20 min -> 14:10 local.
        assert_eq!(
            alarm_timestamp_ms_for(Some("2026-11-14"), Some("14:30"), TODO_TIME_ZONE, 20, None, now_ms),
            Some(utc_ms_from_local(2026, 11, 14, 14, 10, TODO_TIME_ZONE)),
        );
        // 14:00 com alerta 20 min (13:40 já passou, 14:00 ainda vai acontecer) -> avisa às 14:00.
        assert_eq!(
            alarm_timestamp_ms_for(Some("2026-11-14"), Some("14:00"), TODO_TIME_ZONE, 20, None, now_ms),
            Some(utc_ms_from_local(2026, 11, 14, 14, 0, TODO_TIME_ZONE)),
        );
        // 13:30 já venceu -> None.
        assert_eq!(
            alarm_timestamp_ms_for(Some("2026-11-14"), Some("13:30"), TODO_TIME_ZONE, 20, None, now_ms),
            None,
        );
        // Snooze explícito futuro prevalece.
        let snooze_iso = "2026-11-14T16:55:00Z";
        assert_eq!(
            alarm_timestamp_ms_for(Some("2026-11-14"), Some("13:30"), TODO_TIME_ZONE, 20, Some(snooze_iso), now_ms),
            crate::whatsapp::gateway::rfc3339_ms(Some(snooze_iso)),
        );
    }

    #[test]
    fn buckets_and_accent_blind_search_work() {
        let now_ms = utc_ms_from_local(2026, 11, 14, 15, 0, TODO_TIME_ZONE);
        assert_eq!(bucket_for(&item(Some("2026-11-14"), Some("18:00"), false), now_ms), TodoBucket::Today);
        assert_eq!(bucket_for(&item(Some("2026-11-14"), Some("14:00"), false), now_ms), TodoBucket::Overdue);
        assert_eq!(bucket_for(&item(Some("2026-11-13"), None, false), now_ms), TodoBucket::Overdue);
        assert_eq!(bucket_for(&item(Some("2026-11-15"), None, false), now_ms), TodoBucket::Upcoming);
        assert_eq!(bucket_for(&item(None, None, false), now_ms), TodoBucket::NoDate);
        assert_eq!(bucket_for(&item(Some("2026-11-13"), None, true), now_ms), TodoBucket::Done);

        let mut list = vec![item(Some("2026-11-14"), None, true)];
        list[0].subtasks.push(TodoSubtask { id: "s1".into(), text: "Moer grãos".into(), done: false });
        assert_eq!(search_todos(&list, "cafe").len(), 1);
        assert_eq!(search_todos(&list, "graos").len(), 1);
        assert_eq!(search_todos(&list, "padaria").len(), 0);
    }

    #[test]
    fn recurrence_clamps_month_end_and_skips_elapsed_cycles() {
        let jan31 = "2026-01-31";
        let rec = TodoRecurrence {
            frequency: TodoRepeatFrequency::Monthly,
            weekdays: Vec::new(),
            day_of_month: Some(31),
        };
        assert_eq!(next_occurrence_date(&rec, jan31).as_deref(), Some("2026-02-28"));

        let daily = TodoRecurrence {
            frequency: TodoRepeatFrequency::Daily,
            weekdays: Vec::new(),
            day_of_month: None,
        };
        let now_ms = utc_ms_from_local(2026, 11, 14, 10, 0, TODO_TIME_ZONE);
        assert_eq!(
            next_upcoming_occurrence_date(&daily, "2026-11-10", now_ms, TODO_TIME_ZONE, Some("09:00")).as_deref(),
            Some("2026-11-15"),
        );
    }

    #[test]
    fn proposal_validation_rejects_ambiguous_dates_with_concrete_values() {
        let bad = TodoProposal {
            id: "p1".into(),
            title: "Pagar boleto".into(),
            date_state: TodoProposalDateState::Ambiguous,
            due_date: Some("2026-11-20".into()),
            date_phrase: Some("sexta".into()),
            time_state: TodoProposalTimeState::Unspecified,
            due_time: None,
            time_phrase: None,
            priority: TodoPriority::Normal,
        };
        assert!(bad.validate().is_err());
    }

    // Espelha a seção 8 de segundamente/docs/lumen/CONTRATO-SYNC.md (exemplos 8.1 a 8.3, sintéticos).
    // A fixture é cópia exata desses envelopes; se os dois divergirem, o contrato manda.
    #[test]
    fn contrato_sync_v1_task_envelopes_match_the_model() {
        let raw = include_str!("../../tests/fixtures/contrato-sync-v1-task.json");
        let envelopes: Vec<serde_json::Value> = serde_json::from_str(raw).expect("fixture is a JSON array");
        assert_eq!(envelopes.len(), 3);

        for env in &envelopes {
            assert_eq!(env["contract_version"], 1);
            assert!(env.get("op").is_none(), "contract v1 has no op");
            assert_eq!(env["entity_type"], "task");
            let base = env["base_version"].as_u64().unwrap();
            let new = env["new_version"].as_u64().unwrap();
            assert_eq!(new, base + 1);
            assert_eq!(env["entity_id"], env["payload"]["id"]);
            assert_eq!(new, env["payload"]["version"].as_u64().unwrap());
        }

        let item = |i: usize| -> TodoItem { serde_json::from_value(envelopes[i]["payload"].clone()).expect("payload is a TodoItem") };

        // 8.1: tarefa criada no PC.
        let created = item(0);
        assert_eq!(created.id, "task-syn-001");
        assert_eq!(created.text, "Revisar pauta da reunião semanal");
        assert_eq!(created.due, "2026-11-15 09:00");
        assert_eq!(created.due_date.as_deref(), Some("2026-11-15"));
        assert_eq!(created.due_time.as_deref(), Some("09:00"));
        assert_eq!(created.priority, TodoPriority::High);
        assert_eq!(
            created.subtasks,
            vec![TodoSubtask { id: "sub-syn-1".into(), text: "Separar tópicos".into(), done: false }]
        );
        assert_eq!(created.recurrence, None);
        assert!(!created.done);
        assert_eq!(created.done_at, None);
        assert_eq!(created.version, 1);
        assert_eq!(created.deleted_at, None);

        // 8.2: conclusão no celular.
        let done = item(1);
        assert_eq!(done.id, "task-syn-001");
        assert_eq!(done.text, "Revisar pauta da reunião semanal");
        assert_eq!(done.due, "2026-11-15 09:00");
        assert_eq!(done.due_date.as_deref(), Some("2026-11-15"));
        assert_eq!(done.due_time.as_deref(), Some("09:00"));
        assert_eq!(done.priority, TodoPriority::High);
        assert_eq!(
            done.subtasks,
            vec![TodoSubtask { id: "sub-syn-1".into(), text: "Separar tópicos".into(), done: true }]
        );
        assert_eq!(done.recurrence, None);
        assert!(done.done);
        assert_eq!(done.done_at.as_deref(), Some("2026-11-15T08:45:00Z"));
        assert_eq!(done.version, 2);
        assert_eq!(done.deleted_at, None);

        // 8.3: exclusão no PC (tombstone) de tarefa semanal sem data.
        let tombstone = item(2);
        assert_eq!(tombstone.id, "task-syn-002");
        assert_eq!(tombstone.text, "Regar as plantas da varanda");
        assert_eq!(tombstone.due, "");
        assert_eq!(tombstone.due_date, None);
        assert_eq!(tombstone.due_time, None);
        assert_eq!(tombstone.priority, TodoPriority::Low);
        assert!(tombstone.subtasks.is_empty());
        assert_eq!(
            tombstone.recurrence,
            Some(TodoRecurrence { frequency: TodoRepeatFrequency::Weekly, weekdays: vec![2, 5], day_of_month: None })
        );
        assert_eq!(tombstone.series_id.as_deref(), Some("series-syn-002"));
        assert!(!tombstone.done);
        assert_eq!(tombstone.done_at, None);
        assert_eq!(tombstone.source, None);
        assert!(!tombstone.source_stale);
        assert_eq!(tombstone.version, 4);
        assert_eq!(tombstone.deleted_at.as_deref(), Some("2026-11-16T10:00:00Z"));
    }
}
