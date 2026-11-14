//! Gerenciamento de Tarefas do Lumen (L4): loja local, captura por voz/texto com proposta
//! editável, scheduler de lembretes e integração com notificações nativas do Windows.

pub mod model;
pub mod proposal;
pub mod store;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use self::model::{
    alarm_timestamp_ms_for, format_date, is_valid_date, is_valid_time, known_time_zone,
    local_parts_at, now_utc_ms, parse_date, utc_ms_from_local, TodoItem, TodoPriority,
    TodoProposal, TodoRecurrence, TodoShortcutSettings, TODO_ALARM_SNOOZE_MINUTES,
    TODO_DEFAULT_ALERT_MINUTES,
};
use self::store::{CreateTaskInput, TaskStore, ToggleDoneOutcome, UpdateTaskInput};
use crate::speech::{self, audio};

const MAX_VOICE_SAMPLES: usize = speech::SAMPLE_RATE as usize * 60;
const TRIGGER_GRACE_MS: i64 = 5 * 60_000;

struct VoiceSession {
    capture: audio::Capture,
    samples: Arc<Mutex<Vec<f32>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskReminderPayload {
    pub id: String,
    pub text: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub priority: TodoPriority,
    pub alert_minutes: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TasksSnapshot {
    pub items: Vec<TodoItem>,
    pub shortcuts: TodoShortcutSettings,
    pub windows_notifications_available: bool,
    pub voice_ready: bool,
    pub recording: bool,
    pub quarantined_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfirmProposalInput {
    pub text: String,
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
}

pub struct TaskManager {
    store: Mutex<TaskStore>,
    voice: Mutex<Option<VoiceSession>>,
    fired: Mutex<HashMap<String, (u32, i64)>>,
    wake_tx: Sender<()>,
    stop_flag: Arc<AtomicBool>,
}

pub fn folder() -> PathBuf {
    crate::settings::local_dir().join("Todo")
}

pub fn get(app: &AppHandle) -> Option<Arc<TaskManager>> {
    app.try_state::<Arc<TaskManager>>()
        .map(|s| s.inner().clone())
}

pub fn setup(app: &AppHandle) {
    let (wake_tx, wake_rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let manager = Arc::new(TaskManager {
        store: Mutex::new(TaskStore::load_at(folder())),
        voice: Mutex::new(None),
        fired: Mutex::new(HashMap::new()),
        wake_tx,
        stop_flag: stop_flag.clone(),
    });
    app.manage(manager.clone());

    let handle = app.clone();
    std::thread::Builder::new()
        .name("lumen-task-scheduler".into())
        .spawn(move || scheduler_loop(handle, manager, wake_rx, stop_flag))
        .ok();
}

pub fn shutdown(app: &AppHandle) {
    let Some(mgr) = get(app) else { return };
    mgr.stop_flag.store(true, Ordering::Relaxed);
    if let Some(mut session) = mgr.voice.lock().unwrap().take() {
        session.capture.stop();
    }
    let _ = mgr.wake_tx.send(());
}

fn scheduler_loop(
    app: AppHandle,
    mgr: Arc<TaskManager>,
    wake_rx: mpsc::Receiver<()>,
    stop_flag: Arc<AtomicBool>,
) {
    while !stop_flag.load(Ordering::Relaxed) {
        let now = now_utc_ms();
        let due_list = collect_due_alarms(&mgr, now);
        for (item, trigger_ms) in due_list {
            let mut fired = mgr.fired.lock().unwrap();
            fired.insert(item.id.clone(), (item.version, trigger_ms));
            drop(fired);
            dispatch_reminder(&app, &item);
        }

        let wait = next_wait_duration(&mgr, now_utc_ms());
        match wake_rx.recv_timeout(wait) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn next_wait_duration(mgr: &TaskManager, now_ms: i64) -> Duration {
    let items = mgr.store.lock().unwrap().active_items();
    let mut soonest: Option<i64> = None;
    for item in &items {
        if item.done || item.is_tombstoned() {
            continue;
        }
        if let Some(at) = alarm_timestamp_ms_for(
            item.due_date.as_deref(),
            item.due_time.as_deref(),
            &item.time_zone,
            item.alert_minutes,
            item.snoozed_until.as_deref(),
            now_ms,
        ) {
            soonest = Some(soonest.map_or(at, |cur| cur.min(at)));
        }
    }
    match soonest {
        Some(target) => {
            let delta_ms = (target - now_ms).clamp(500, 15_000) as u64;
            Duration::from_millis(delta_ms)
        }
        None => Duration::from_secs(15),
    }
}

fn collect_due_alarms(mgr: &TaskManager, now_ms: i64) -> Vec<(TodoItem, i64)> {
    let items = mgr.store.lock().unwrap().active_items();
    let fired = mgr.fired.lock().unwrap();
    let mut out = Vec::new();
    for item in items {
        let already = fired.get(&item.id).copied();
        if let Some(trigger_ms) = due_trigger_for_now(&item, now_ms, already) {
            out.push((item, trigger_ms));
        }
    }
    out
}

/// Calcula se a tarefa deve disparar aviso em `now_ms`, evitando duplicidade no mesmo `(version, trigger_ms)`.
pub fn due_trigger_for_now(
    item: &TodoItem,
    now_ms: i64,
    last_fired: Option<(u32, i64)>,
) -> Option<i64> {
    if item.done || item.is_tombstoned() || !item.has_alarm_time() {
        return None;
    }
    let trigger_ms = if let Some(snooze_ms) = crate::whatsapp::gateway::rfc3339_ms(item.snoozed_until.as_deref()) {
        snooze_ms
    } else {
        let (y, m, d) = parse_date(item.due_date.as_deref()?)?;
        let (hh, mm) = model::parse_time(item.due_time.as_deref()?)?;
        let due_ms = utc_ms_from_local(y, m, d, hh, mm, &item.time_zone);
        let alert_ms = due_ms - (item.alert_minutes.max(0) as i64) * 60_000;
        let created_ms = crate::whatsapp::gateway::rfc3339_ms(Some(&item.created_at)).unwrap_or(0);
        if alert_ms + 1000 >= created_ms {
            alert_ms
        } else {
            due_ms
        }
    };

    if trigger_ms <= now_ms && (now_ms - trigger_ms) <= TRIGGER_GRACE_MS {
        if last_fired == Some((item.version, trigger_ms)) {
            return None;
        }
        return Some(trigger_ms);
    }
    None
}

fn format_reminder_subtitle(item: &TodoItem) -> String {
    let when = match (item.due_date.as_deref(), item.due_time.as_deref()) {
        (Some(d), Some(t)) => {
            if let Some((y, m, day)) = parse_date(d) {
                format!("{day:02}/{m:02}/{y:04} às {t}")
            } else {
                format!("{d} às {t}")
            }
        }
        (Some(d), None) => d.to_string(),
        _ => "Agora".into(),
    };
    if item.snoozed_until.is_some() {
        format!("Lembrete adiado · {when}")
    } else if item.alert_minutes > 0 {
        format!("Vence às {} ({when})", item.due_time.as_deref().unwrap_or(""))
    } else {
        format!("Horário da tarefa · {when}")
    }
}

fn dispatch_reminder(app: &AppHandle, item: &TodoItem) {
    let subtitle = format_reminder_subtitle(item);
    #[cfg(windows)]
    if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
        crate::permission_notifications::show_task_reminder(
            &toasts,
            item.id.clone(),
            item.text.clone(),
            subtitle,
        );
    }

    let payload = TaskReminderPayload {
        id: item.id.clone(),
        text: item.text.clone(),
        due_date: item.due_date.clone(),
        due_time: item.due_time.clone(),
        priority: item.priority,
        alert_minutes: item.alert_minutes,
    };
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-reminder", &payload);
    let _ = app.emit_to(crate::HUB_LABEL, "task-reminder", &payload);
}

fn clear_task_toast(app: &AppHandle, task_id: &str) {
    #[cfg(windows)]
    if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
        crate::permission_notifications::clear_task_reminder(&toasts, task_id);
    }
    #[cfg(not(windows))]
    let _ = (app, task_id);
}

#[cfg(windows)]
pub fn handle_toast_action(
    app: &AppHandle,
    task_id: &str,
    action: crate::permission_notifications::TaskToastAction,
) {
    use crate::permission_notifications::TaskToastAction;
    let Some(mgr) = get(app) else { return };
    let now = now_utc_ms();
    match action {
        TaskToastAction::Snooze => {
            if let Ok(item) = mgr.store.lock().unwrap().snooze(task_id, Some(TODO_ALARM_SNOOZE_MINUTES), now) {
                mgr.fired.lock().unwrap().remove(task_id);
                let _ = mgr.wake_tx.send(());
                emit_tasks_changed(app);
                let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-snoozed", &item.id);
            }
        }
        TaskToastAction::Complete => {
            if let Ok(outcome) = mgr.store.lock().unwrap().toggle_done(task_id, Some(true), now) {
                mgr.fired.lock().unwrap().remove(task_id);
                let _ = mgr.wake_tx.send(());
                emit_tasks_changed(app);
                let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-completed", &outcome.item.id);
            }
        }
        TaskToastAction::Open => {
            crate::show_hub(app, Some("tasks"));
            let _ = app.emit_to(crate::HUB_LABEL, "task-focus", task_id.to_string());
        }
    }
}

fn emit_tasks_changed(app: &AppHandle) {
    let _ = app.emit("tasks-changed", ());
}

fn build_snapshot(app: &AppHandle, mgr: &TaskManager) -> TasksSnapshot {
    let store = mgr.store.lock().unwrap();
    let recording = mgr.voice.lock().unwrap().is_some();
    #[cfg(windows)]
    let windows_notifications_available = app
        .try_state::<crate::permission_notifications::PermissionToasts>()
        .is_some_and(|t| t.is_available());
    #[cfg(not(windows))]
    let windows_notifications_available = false;

    TasksSnapshot {
        items: store.active_items(),
        shortcuts: store.shortcut_settings().clone(),
        windows_notifications_available,
        voice_ready: speech::is_ready(),
        recording,
        quarantined_file: store.quarantined_file().map(str::to_string),
    }
}

// ── Comandos Tauri ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn tasks_state(app: AppHandle, mgr: State<Arc<TaskManager>>) -> TasksSnapshot {
    build_snapshot(&app, &mgr)
}

#[tauri::command]
pub async fn tasks_propose_from_text(
    app: AppHandle,
    text: String,
) -> Result<TodoProposal, String> {
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("Escreva ou dite a tarefa antes de gerar o cartão.".into());
    }
    let settings = app
        .try_state::<crate::Shared>()
        .map(|s| s.settings.lock().unwrap().clone());
    let now = now_utc_ms();
    let zone = model::TODO_TIME_ZONE;
    proposal::propose_with(&trimmed, now, zone, |sys, usr| async move {
        let Some(cfg) = settings else {
            return Err("Configurações indisponíveis".into());
        };
        crate::chat::complete_oneshot(&cfg, &sys, &usr, 256, Duration::from_secs(20)).await
    })
    .await
}

#[tauri::command]
pub fn tasks_voice_start(app: AppHandle, mgr: State<Arc<TaskManager>>) -> Result<(), String> {
    if !speech::is_ready() {
        return Err(
            "Baixe o modelo de voz em Configurações → Reconhecimento para ditar tarefas."
                .into(),
        );
    }
    let mut voice_slot = mgr.voice.lock().unwrap();
    if voice_slot.is_some() {
        return Ok(());
    }
    let mic = crate::flow::get(&app).and_then(|f| {
        let m = f.store.lock().unwrap().prefs.mic.clone();
        (!m.is_empty()).then_some(m)
    });
    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink_buf = samples.clone();
    let capture = audio::Capture::start(audio::Source::Mic(mic), move |chunk| {
        let mut buf = sink_buf.lock().unwrap();
        if buf.len() < MAX_VOICE_SAMPLES {
            let room = MAX_VOICE_SAMPLES - buf.len();
            buf.extend_from_slice(&chunk[..chunk.len().min(room)]);
        }
    })
    .map_err(|_| "Microfone indisponível ou sem permissão.".to_string())?;

    *voice_slot = Some(VoiceSession { capture, samples });
    let _ = app.emit("tasks-voice-state", true);
    Ok(())
}

#[tauri::command]
pub async fn tasks_voice_stop(
    app: AppHandle,
    mgr: State<'_, Arc<TaskManager>>,
) -> Result<Option<TodoProposal>, String> {
    let samples = {
        let mut voice_slot = mgr.voice.lock().unwrap();
        let Some(mut session) = voice_slot.take() else {
            return Ok(None);
        };
        session.capture.stop();
        let buf = std::mem::take(&mut *session.samples.lock().unwrap());
        buf
    };
    let _ = app.emit("tasks-voice-state", false);
    if samples.len() < (speech::SAMPLE_RATE as usize) / 4 {
        return Ok(None);
    }
    let transcript = tauri::async_runtime::spawn_blocking(move || speech::transcribe(&samples))
        .await
        .map_err(|e| e.to_string())?;
    let Some(t) = transcript else {
        return Ok(None);
    };
    let text = t.text.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }
    let prop = tasks_propose_from_text(app, text).await?;
    Ok(Some(prop))
}

#[tauri::command]
pub fn tasks_voice_cancel(app: AppHandle, mgr: State<Arc<TaskManager>>) {
    if let Some(mut session) = mgr.voice.lock().unwrap().take() {
        session.capture.stop();
    }
    let _ = app.emit("tasks-voice-state", false);
}

#[tauri::command]
pub fn tasks_confirm_proposal(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    input: ConfirmProposalInput,
) -> Result<TodoItem, String> {
    let text = input.text.trim().to_string();
    if text.is_empty() {
        return Err("Escreva o título da tarefa antes de confirmar.".into());
    }
    let item = mgr.store.lock().unwrap().create(
        CreateTaskInput {
            text,
            due: None,
            due_date: input.due_date,
            due_time: input.due_time,
            time_zone: input.time_zone,
            alert_minutes: input.alert_minutes,
            priority: input.priority,
            subtasks: input.subtasks,
            recurrence: input.recurrence,
            source: None,
        },
        now_utc_ms(),
    )?;
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_create(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    input: CreateTaskInput,
) -> Result<TodoItem, String> {
    let item = mgr.store.lock().unwrap().create(input, now_utc_ms())?;
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_update(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    input: UpdateTaskInput,
) -> Result<TodoItem, String> {
    let item = mgr.store.lock().unwrap().update(input)?;
    mgr.fired.lock().unwrap().remove(&item.id);
    clear_task_toast(&app, &item.id);
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_toggle_done(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    id: String,
    done: Option<bool>,
) -> Result<ToggleDoneOutcome, String> {
    let outcome = mgr.store.lock().unwrap().toggle_done(&id, done, now_utc_ms())?;
    mgr.fired.lock().unwrap().remove(&id);
    clear_task_toast(&app, &id);
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    if outcome.item.done {
        let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-completed", &outcome.item.id);
    }
    Ok(outcome)
}

#[tauri::command]
pub fn tasks_snooze(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    id: String,
    minutes: Option<i64>,
) -> Result<TodoItem, String> {
    let item = mgr
        .store
        .lock()
        .unwrap()
        .snooze(&id, minutes, now_utc_ms())?;
    mgr.fired.lock().unwrap().remove(&id);
    clear_task_toast(&app, &id);
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-snoozed", &item.id);
    Ok(item)
}

#[tauri::command]
pub fn tasks_delete(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    id: String,
) -> Result<TodoItem, String> {
    let item = mgr.store.lock().unwrap().delete(&id, now_utc_ms())?;
    mgr.fired.lock().unwrap().remove(&id);
    clear_task_toast(&app, &id);
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_restore(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    id: String,
) -> Result<TodoItem, String> {
    let item = mgr.store.lock().unwrap().restore(&id)?;
    let _ = mgr.wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_add_subtask(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    task_id: String,
    text: String,
) -> Result<TodoItem, String> {
    let item = mgr.store.lock().unwrap().add_subtask(&task_id, &text)?;
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_toggle_subtask(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    task_id: String,
    subtask_id: String,
) -> Result<TodoItem, String> {
    let item = mgr
        .store
        .lock()
        .unwrap()
        .toggle_subtask(&task_id, &subtask_id)?;
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_remove_subtask(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    task_id: String,
    subtask_id: String,
) -> Result<TodoItem, String> {
    let item = mgr
        .store
        .lock()
        .unwrap()
        .remove_subtask(&task_id, &subtask_id)?;
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_save_shortcut_settings(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    settings: TodoShortcutSettings,
) -> Result<TodoShortcutSettings, String> {
    mgr.store.lock().unwrap().set_shortcut_settings(settings.clone())?;
    emit_tasks_changed(&app);
    Ok(settings)
}

#[tauri::command]
pub fn tasks_open_page(app: AppHandle, task_id: Option<String>) {
    crate::show_hub(&app, Some("tasks"));
    if let Some(id) = task_id {
        let _ = app.emit_to(crate::HUB_LABEL, "task-focus", id);
    }
}

/// Dispara manualmente o aviso (Windows + Ilha) de uma tarefa específica para conferência.
#[tauri::command]
pub fn tasks_preview_reminder(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    id: String,
) -> Result<(), String> {
    let item = mgr
        .store
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "Tarefa não encontrada.".to_string())?;
    dispatch_reminder(&app, &item);
    Ok(())
}

/// Helper puro para checar data/hora válidas em comandos rápidos da UI/testes.
#[cfg(test)]
pub fn local_today_parts(now_ms: i64, zone: Option<&str>) -> (String, String) {
    let tz = known_time_zone(zone);
    let (y, m, d, hh, mm) = local_parts_at(now_ms, &tz);
    let date = format_date(y, m, d);
    let time = format!("{hh:02}:{mm:02}");
    debug_assert!(is_valid_date(&date) && is_valid_time(&time));
    (date, time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::TODO_TIME_ZONE;

    fn sample_item(created_at: &str, due_date: &str, due_time: &str, alert_minutes: i32) -> TodoItem {
        TodoItem {
            id: "task-1".into(),
            text: "Enviar orçamento".into(),
            due: "".into(),
            due_date: Some(due_date.into()),
            due_time: Some(due_time.into()),
            time_zone: TODO_TIME_ZONE.into(),
            alert_minutes,
            priority: TodoPriority::Normal,
            subtasks: Vec::new(),
            recurrence: None,
            series_id: None,
            done: false,
            created_at: created_at.into(),
            done_at: None,
            snoozed_until: None,
            source: None,
            source_stale: false,
            version: 1,
            deleted_at: None,
        }
    }

    #[test]
    fn scheduler_triggers_once_per_version_and_respects_due_fallback() {
        // Criada às 13:00 local (16:00Z), vence 14:30 local com alerta 20m -> dispara 14:10 local.
        let item = sample_item("2026-11-14T16:00:00Z", "2026-11-14", "14:30", TODO_DEFAULT_ALERT_MINUTES);
        let at_14_09 = utc_ms_from_local(2026, 11, 14, 14, 9, TODO_TIME_ZONE);
        let at_14_10_05 = utc_ms_from_local(2026, 11, 14, 14, 10, TODO_TIME_ZONE) + 5000;
        assert_eq!(due_trigger_for_now(&item, at_14_09, None), None);
        let trigger = due_trigger_for_now(&item, at_14_10_05, None).expect("dispara às 14:10");
        assert_eq!(due_trigger_for_now(&item, at_14_10_05 + 10_000, Some((1, trigger))), None);

        // Criada às 14:15 local com vencimento 14:30 e alerta 20m (alerta 14:10 já passou): dispara às 14:30.
        let late_item = sample_item("2026-11-14T17:15:00Z", "2026-11-14", "14:30", 20);
        let at_14_20 = utc_ms_from_local(2026, 11, 14, 14, 20, TODO_TIME_ZONE);
        let at_14_30 = utc_ms_from_local(2026, 11, 14, 14, 30, TODO_TIME_ZONE);
        assert_eq!(due_trigger_for_now(&late_item, at_14_20, None), None);
        assert_eq!(due_trigger_for_now(&late_item, at_14_30, None), Some(at_14_30));
    }

    #[test]
    fn snoozed_task_fires_at_snoozed_until() {
        let mut item = sample_item("2026-11-14T16:00:00Z", "2026-11-14", "14:30", 20);
        item.version = 2;
        item.snoozed_until = Some("2026-11-14T17:25:00Z".into()); // 14:25 local
        let at_14_24 = utc_ms_from_local(2026, 11, 14, 14, 24, TODO_TIME_ZONE);
        let at_14_25 = utc_ms_from_local(2026, 11, 14, 14, 25, TODO_TIME_ZONE);
        assert_eq!(due_trigger_for_now(&item, at_14_24, None), None);
        assert_eq!(due_trigger_for_now(&item, at_14_25, None), Some(at_14_25));
    }
}
