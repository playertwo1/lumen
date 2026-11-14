//! Persistência atômica de tarefas (`todo.json`, schema v2), quarentena de corrupção,
//! subtarefas, recorrência e configurações de atalhos.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{
    is_valid_date, is_valid_time, known_time_zone, next_upcoming_occurrence_date, now_utc_ms,
    TodoItem, TodoPriority, TodoRecurrence, TodoShortcutSettings, TodoSource, TodoSubtask,
    TODO_ALARM_SNOOZE_MINUTES, TODO_DEFAULT_ALERT_MINUTES, TODO_SCHEMA_VERSION,
};
use crate::whatsapp::gateway::iso_utc;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoEnvelope {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    items: Vec<TodoItem>,
}

fn default_schema_version() -> u32 {
    TODO_SCHEMA_VERSION
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTaskInput {
    pub text: String,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub due_time: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub alert_minutes: Option<i32>,
    #[serde(default)]
    pub priority: Option<TodoPriority>,
    #[serde(default)]
    pub subtasks: Vec<String>,
    #[serde(default)]
    pub recurrence: Option<TodoRecurrence>,
    #[serde(default)]
    pub source: Option<TodoSource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTaskInput {
    pub id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub due_date: Option<Option<String>>,
    #[serde(default)]
    pub due_time: Option<Option<String>>,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub alert_minutes: Option<i32>,
    #[serde(default)]
    pub priority: Option<TodoPriority>,
    #[serde(default)]
    pub recurrence: Option<Option<TodoRecurrence>>,
    #[serde(default)]
    pub clear_snooze: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToggleDoneOutcome {
    pub item: TodoItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_occurrence: Option<TodoItem>,
}

pub struct TaskStore {
    dir: PathBuf,
    items: Vec<TodoItem>,
    shortcuts: TodoShortcutSettings,
    quarantined_file: Option<String>,
}

impl TaskStore {
    pub fn load_at(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let (items, quarantined_file) = load_todo_items(&dir);
        let shortcuts = load_shortcut_settings(&dir);
        Self {
            dir,
            items,
            shortcuts,
            quarantined_file,
        }
    }

    pub fn active_items(&self) -> Vec<TodoItem> {
        let mut out: Vec<TodoItem> = self
            .items
            .iter()
            .filter(|item| !item.is_tombstoned())
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            a.done
                .cmp(&b.done)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        out
    }

    pub fn get(&self, id: &str) -> Option<&TodoItem> {
        self.items.iter().find(|item| item.id == id && !item.is_tombstoned())
    }

    pub fn shortcut_settings(&self) -> &TodoShortcutSettings {
        &self.shortcuts
    }

    pub fn quarantined_file(&self) -> Option<&str> {
        self.quarantined_file.as_deref()
    }

    pub fn set_shortcut_settings(&mut self, settings: TodoShortcutSettings) -> Result<(), String> {
        settings.validate()?;
        self.shortcuts = settings;
        self.persist_shortcuts()
    }

    pub fn create(&mut self, input: CreateTaskInput, now_ms: i64) -> Result<TodoItem, String> {
        let text = input.text.trim().to_string();
        if text.is_empty() {
            return Err("Escreva o título da tarefa antes de salvar.".into());
        }
        let due_date = clean_opt(input.due_date);
        if let Some(ref d) = due_date {
            if !is_valid_date(d) {
                return Err("Data inválida (use YYYY-MM-DD).".into());
            }
        }
        let due_time = clean_opt(input.due_time);
        if let Some(ref t) = due_time {
            if !is_valid_time(t) {
                return Err("Horário inválido (use HH:mm).".into());
            }
            if due_date.is_none() {
                return Err("Escolha uma data antes de definir um horário.".into());
            }
        }
        let recurrence = match input.recurrence {
            Some(rec) => Some(rec.normalized()?),
            None => None,
        };
        let alert_minutes = input
            .alert_minutes
            .unwrap_or(self.shortcuts.default_alert_minutes)
            .max(0);
        let time_zone = known_time_zone(input.time_zone.as_deref());
        let subtasks: Vec<TodoSubtask> = input
            .subtasks
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|text| TodoSubtask {
                id: crate::flow::text::new_id(),
                text,
                done: false,
            })
            .collect();

        let id = crate::flow::text::new_id();
        let series_id = recurrence.as_ref().map(|_| id.clone());
        let due_label = input
            .due
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format_due_label(due_date.as_deref(), due_time.as_deref()));

        let item = TodoItem {
            id,
            text,
            due: due_label,
            due_date,
            due_time,
            time_zone,
            alert_minutes,
            priority: input.priority.unwrap_or(TodoPriority::Normal),
            subtasks,
            recurrence,
            series_id,
            done: false,
            created_at: iso_utc(now_ms),
            done_at: None,
            snoozed_until: None,
            source: input.source,
            source_stale: false,
            version: 1,
            deleted_at: None,
        };
        self.items.push(item.clone());
        self.persist_items()?;
        Ok(item)
    }

    pub fn update(&mut self, input: UpdateTaskInput) -> Result<TodoItem, String> {
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == input.id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        let mut next = self.items[idx].clone();
        if let Some(text) = input.text {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                return Err("O título da tarefa não pode ficar vazio.".into());
            }
            next.text = trimmed;
        }
        if let Some(due_date) = input.due_date {
            match clean_opt(due_date) {
                Some(d) if is_valid_date(&d) => next.due_date = Some(d),
                Some(_) => return Err("Data inválida (use YYYY-MM-DD).".into()),
                None => {
                    next.due_date = None;
                    next.due_time = None;
                }
            }
        }
        if let Some(due_time) = input.due_time {
            match clean_opt(due_time) {
                Some(t) if is_valid_time(&t) => {
                    if next.due_date.is_none() {
                        return Err("Escolha uma data antes de definir um horário.".into());
                    }
                    next.due_time = Some(t);
                }
                Some(_) => return Err("Horário inválido (use HH:mm).".into()),
                None => next.due_time = None,
            }
        }
        if let Some(zone) = input.time_zone {
            next.time_zone = known_time_zone(Some(&zone));
        }
        if let Some(alert) = input.alert_minutes {
            next.alert_minutes = alert.max(0);
        }
        if let Some(priority) = input.priority {
            next.priority = priority;
        }
        if let Some(recurrence) = input.recurrence {
            next.recurrence = match recurrence {
                Some(rec) => {
                    if next.series_id.is_none() {
                        next.series_id = Some(next.id.clone());
                    }
                    Some(rec.normalized()?)
                }
                None => None,
            };
        }
        if let Some(due) = input.due {
            let d = due.trim().to_string();
            next.due = if d.is_empty() {
                format_due_label(next.due_date.as_deref(), next.due_time.as_deref())
            } else {
                d
            };
        } else {
            next.due = format_due_label(next.due_date.as_deref(), next.due_time.as_deref());
        }
        if input.clear_snooze {
            next.snoozed_until = None;
        }
        next.version = next.version.saturating_add(1);
        self.items[idx] = next.clone();
        self.persist_items()?;
        Ok(next)
    }

    pub fn toggle_done(&mut self, id: &str, force_done: Option<bool>, now_ms: i64) -> Result<ToggleDoneOutcome, String> {
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        let mut current = self.items[idx].clone();
        let next_done = force_done.unwrap_or(!current.done);
        if next_done == current.done {
            return Ok(ToggleDoneOutcome {
                item: current,
                next_occurrence: None,
            });
        }
        current.done = next_done;
        current.done_at = if next_done { Some(iso_utc(now_ms)) } else { None };
        current.snoozed_until = None;
        current.version = current.version.saturating_add(1);
        self.items[idx] = current.clone();

        let mut next_occurrence = None;
        if next_done {
            if let (Some(rec), Some(after_date)) = (&current.recurrence, current.due_date.as_deref()) {
                let series = current.series_id.clone().unwrap_or_else(|| current.id.clone());
                let already_open = self.items.iter().any(|other| {
                    !other.is_tombstoned()
                        && !other.done
                        && other.series_id.as_deref() == Some(series.as_str())
                });
                if !already_open {
                    if let Some(next_date) = next_upcoming_occurrence_date(
                        rec,
                        after_date,
                        now_ms,
                        &current.time_zone,
                        current.due_time.as_deref(),
                    ) {
                        let next_item = TodoItem {
                            id: crate::flow::text::new_id(),
                            text: current.text.clone(),
                            due: format_due_label(Some(&next_date), current.due_time.as_deref()),
                            due_date: Some(next_date),
                            due_time: current.due_time.clone(),
                            time_zone: current.time_zone.clone(),
                            alert_minutes: current.alert_minutes,
                            priority: current.priority,
                            subtasks: current
                                .subtasks
                                .iter()
                                .map(|s| TodoSubtask {
                                    id: crate::flow::text::new_id(),
                                    text: s.text.clone(),
                                    done: false,
                                })
                                .collect(),
                            recurrence: Some(rec.clone()),
                            series_id: Some(series),
                            done: false,
                            created_at: iso_utc(now_ms),
                            done_at: None,
                            snoozed_until: None,
                            source: current.source.clone(),
                            source_stale: current.source_stale,
                            version: 1,
                            deleted_at: None,
                        };
                        self.items.push(next_item.clone());
                        next_occurrence = Some(next_item);
                    }
                }
            }
        }

        self.persist_items()?;
        Ok(ToggleDoneOutcome {
            item: current,
            next_occurrence,
        })
    }

    pub fn snooze(&mut self, id: &str, minutes: Option<i64>, now_ms: i64) -> Result<TodoItem, String> {
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        let mins = minutes.unwrap_or(TODO_ALARM_SNOOZE_MINUTES).max(1);
        let until_ms = now_ms + mins * 60_000;
        let mut item = self.items[idx].clone();
        if item.done {
            return Err("Tarefa concluída não pode ser adiada.".into());
        }
        item.snoozed_until = Some(iso_utc(until_ms));
        item.version = item.version.saturating_add(1);
        self.items[idx] = item.clone();
        self.persist_items()?;
        Ok(item)
    }

    pub fn delete(&mut self, id: &str, now_ms: i64) -> Result<TodoItem, String> {
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        let mut item = self.items[idx].clone();
        item.deleted_at = Some(iso_utc(now_ms));
        item.version = item.version.saturating_add(1);
        self.items[idx] = item.clone();
        self.persist_items()?;
        Ok(item)
    }

    pub fn restore(&mut self, id: &str) -> Result<TodoItem, String> {
        let Some(idx) = self.items.iter().position(|i| i.id == id) else {
            return Err("Tarefa não encontrada para restauração.".into());
        };
        let mut item = self.items[idx].clone();
        item.deleted_at = None;
        item.version = item.version.saturating_add(1);
        self.items[idx] = item.clone();
        self.persist_items()?;
        Ok(item)
    }

    pub fn add_subtask(&mut self, task_id: &str, text: &str) -> Result<TodoItem, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("Escreva a subtarefa antes de adicionar.".into());
        }
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == task_id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        self.items[idx].subtasks.push(TodoSubtask {
            id: crate::flow::text::new_id(),
            text: trimmed.to_string(),
            done: false,
        });
        self.items[idx].version = self.items[idx].version.saturating_add(1);
        let updated = self.items[idx].clone();
        self.persist_items()?;
        Ok(updated)
    }

    pub fn toggle_subtask(&mut self, task_id: &str, subtask_id: &str) -> Result<TodoItem, String> {
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == task_id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        let Some(sub) = self.items[idx]
            .subtasks
            .iter_mut()
            .find(|s| s.id == subtask_id)
        else {
            return Err("Subtarefa não encontrada.".into());
        };
        sub.done = !sub.done;
        self.items[idx].version = self.items[idx].version.saturating_add(1);
        let updated = self.items[idx].clone();
        self.persist_items()?;
        Ok(updated)
    }

    pub fn remove_subtask(&mut self, task_id: &str, subtask_id: &str) -> Result<TodoItem, String> {
        let Some(idx) = self
            .items
            .iter()
            .position(|i| i.id == task_id && !i.is_tombstoned())
        else {
            return Err("Tarefa não encontrada.".into());
        };
        let before = self.items[idx].subtasks.len();
        self.items[idx].subtasks.retain(|s| s.id != subtask_id);
        if self.items[idx].subtasks.len() == before {
            return Err("Subtarefa não encontrada.".into());
        }
        self.items[idx].version = self.items[idx].version.saturating_add(1);
        let updated = self.items[idx].clone();
        self.persist_items()?;
        Ok(updated)
    }

    fn persist_items(&self) -> Result<(), String> {
        let envelope = TodoEnvelope {
            schema_version: TODO_SCHEMA_VERSION,
            items: self.items.clone(),
        };
        let data = serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())?;
        write_atomic(&self.dir.join("todo.json"), &data)
    }

    fn persist_shortcuts(&self) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(&self.shortcuts).map_err(|e| e.to_string())?;
        write_atomic(&self.dir.join("shortcut_settings.json"), &data)
    }
}

fn clean_opt(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn format_due_label(due_date: Option<&str>, due_time: Option<&str>) -> String {
    match (due_date, due_time) {
        (Some(d), Some(t)) => format!("{d} {t}"),
        (Some(d), None) => d.to_string(),
        (None, _) => String::new(),
    }
}

fn load_todo_items(dir: &Path) -> (Vec<TodoItem>, Option<String>) {
    let path = dir.join("todo.json");
    let Ok(bytes) = fs::read(&path) else {
        return (Vec::new(), None);
    };
    if let Ok(envelope) = serde_json::from_slice::<TodoEnvelope>(&bytes) {
        let mut items = envelope.items;
        for item in &mut items {
            item.sanitize();
        }
        return (items, None);
    }
    if let Ok(mut legacy) = serde_json::from_slice::<Vec<TodoItem>>(&bytes) {
        for item in &mut legacy {
            if item.alert_minutes <= 0 {
                item.alert_minutes = TODO_DEFAULT_ALERT_MINUTES;
            }
            item.sanitize();
        }
        return (legacy, None);
    }
    let quarantine_name = format!("todo.corrupted.{}.json", now_utc_ms());
    let _ = fs::rename(&path, dir.join(&quarantine_name));
    (Vec::new(), Some(quarantine_name))
}

fn load_shortcut_settings(dir: &Path) -> TodoShortcutSettings {
    let path = dir.join("shortcut_settings.json");
    let Ok(bytes) = fs::read(path) else {
        return TodoShortcutSettings::default();
    };
    let Ok(parsed) = serde_json::from_slice::<TodoShortcutSettings>(&bytes) else {
        return TodoShortcutSettings::default();
    };
    if parsed.validate().is_ok() {
        parsed
    } else {
        TodoShortcutSettings::default()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::model::{TodoRepeatFrequency, TODO_TIME_ZONE};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-tasks-test-{name}-{}-{}",
            std::process::id(),
            now_utc_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_updates_snoozes_and_tombstones_tasks() {
        let dir = temp_dir("crud");
        let now = 1_794_668_400_000;
        let mut store = TaskStore::load_at(dir.clone());
        let created = store
            .create(
                CreateTaskInput {
                    text: "Enviar relatório".into(),
                    due: None,
                    due_date: Some("2026-11-15".into()),
                    due_time: Some("14:00".into()),
                    time_zone: None,
                    alert_minutes: None,
                    priority: Some(TodoPriority::High),
                    subtasks: vec!["Revisar números".into()],
                    recurrence: None,
                    source: None,
                },
                now,
            )
            .unwrap();
        assert_eq!(created.alert_minutes, 20);
        assert_eq!(created.subtasks.len(), 1);
        assert_eq!(created.priority, TodoPriority::High);

        let snoozed = store.snooze(&created.id, Some(10), now).unwrap();
        assert!(snoozed.snoozed_until.is_some());
        assert_eq!(snoozed.version, 2);

        store.delete(&created.id, now).unwrap();
        assert!(store.active_items().is_empty());

        let restored = store.restore(&created.id).unwrap();
        assert_eq!(restored.deleted_at, None);
        assert_eq!(store.active_items().len(), 1);

        let reloaded = TaskStore::load_at(dir.clone());
        assert_eq!(reloaded.active_items().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn completing_recurring_task_schedules_next_occurrence_once() {
        let dir = temp_dir("recur");
        let now = crate::tasks::model::utc_ms_from_local(2026, 11, 14, 10, 0, TODO_TIME_ZONE);
        let mut store = TaskStore::load_at(dir.clone());
        let first = store
            .create(
                CreateTaskInput {
                    text: "Tomar água".into(),
                    due: None,
                    due_date: Some("2026-11-14".into()),
                    due_time: Some("09:00".into()),
                    time_zone: None,
                    alert_minutes: Some(20),
                    priority: None,
                    subtasks: vec!["Copo cheio".into()],
                    recurrence: Some(TodoRecurrence {
                        frequency: TodoRepeatFrequency::Daily,
                        weekdays: Vec::new(),
                        day_of_month: None,
                    }),
                    source: None,
                },
                now,
            )
            .unwrap();

        let outcome = store.toggle_done(&first.id, Some(true), now).unwrap();
        assert!(outcome.item.done);
        let next = outcome.next_occurrence.expect("próxima ocorrência criada");
        assert_eq!(next.due_date.as_deref(), Some("2026-11-15"));
        assert_eq!(next.series_id, first.series_id);
        assert!(!next.done);
        assert!(!next.subtasks[0].done);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupted_todo_json_is_quarantined_without_crash() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join("todo.json"), b"{not valid json").unwrap();
        let store = TaskStore::load_at(dir.clone());
        assert!(store.active_items().is_empty());
        assert!(store
            .quarantined_file()
            .is_some_and(|n| n.starts_with("todo.corrupted.")));
        let _ = fs::remove_dir_all(dir);
    }
}
