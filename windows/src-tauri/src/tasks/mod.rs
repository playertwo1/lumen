//! Gerenciamento de Tarefas do Lumen (L4): loja local, captura por voz/texto com proposta
//! editável, scheduler de lembretes e integração com notificações nativas do Windows.

pub mod model;
pub mod proposal;
pub mod store;
pub mod sync;

#[allow(unused_imports)]
pub use self::sync::{TaskSyncState, TaskSyncStatus, TaskTextConflict};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ring::digest;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use self::model::{
    alarm_timestamp_ms_for, format_date, is_valid_date, is_valid_time, known_time_zone,
    local_parts_at, now_utc_ms, parse_date, utc_ms_from_local, TodoItem, TodoPriority,
    TodoProposal, TodoRecurrence, TodoShortcutSettings, TODO_ALARM_SNOOZE_MINUTES,
    TODO_DEFAULT_ALERT_MINUTES,
};
use self::store::{CreateTaskInput, TaskStore, ToggleDoneOutcome, UpdateTaskInput};
use self::sync::{RelayMailboxTransport, SyncCycleOutcome, SyncPauseReason, SyncStore};
use crate::speech::{self, audio};

const MAX_VOICE_SAMPLES: usize = speech::SAMPLE_RATE as usize * 60;
const TRIGGER_GRACE_MS: i64 = 5 * 60_000;
const SYNC_POLL_INTERVAL_SECS: u64 = 60;
const SYNC_BACKOFF_STEPS_SECS: [u64; 6] = [5, 15, 30, 60, 120, 300];

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
    pub sync: TaskSyncStatus,
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

struct LocalState {
    store: TaskStore,
    sync: SyncStore,
}

impl LocalState {
    fn create_and_enqueue(&mut self, input: CreateTaskInput, now_ms: i64) -> Result<TodoItem, String> {
        let item = self.store.create(input, now_ms)?;
        self.sync.enqueue_local_mutation(&item, 0, now_ms)?;
        Ok(item)
    }

    fn update_and_enqueue(&mut self, input: UpdateTaskInput, now_ms: i64) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(&input.id).map(|i| i.version).unwrap_or(0);
        let clear_text_conflict = input.text.is_some();
        let item = self.store.update(input)?;
        if clear_text_conflict {
            let _ = self.sync.remove_conflict(&format!("{}:text", item.id));
        }
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn toggle_done_and_enqueue(
        &mut self,
        id: &str,
        force_done: Option<bool>,
        now_ms: i64,
    ) -> Result<(ToggleDoneOutcome, bool), String> {
        let prev_version = self.store.find_any(id).map(|i| i.version).unwrap_or(0);
        let outcome = self.store.toggle_done(id, force_done, now_ms)?;
        let changed = outcome.item.version != prev_version;
        if changed {
            self.sync
                .enqueue_local_mutation(&outcome.item, prev_version, now_ms)?;
        }
        if let Some(ref next) = outcome.next_occurrence {
            self.sync.enqueue_local_mutation(next, 0, now_ms)?;
        }
        Ok((outcome, changed))
    }

    fn snooze_and_enqueue(
        &mut self,
        id: &str,
        minutes: Option<i64>,
        now_ms: i64,
    ) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(id).map(|i| i.version).unwrap_or(0);
        let item = self.store.snooze(id, minutes, now_ms)?;
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn delete_and_enqueue(&mut self, id: &str, now_ms: i64) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(id).map(|i| i.version).unwrap_or(0);
        let item = self.store.delete(id, now_ms)?;
        self.sync.clear_conflicts_for_entity(&item.id);
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn restore_and_enqueue(&mut self, id: &str, now_ms: i64) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(id).map(|i| i.version).unwrap_or(0);
        let item = self.store.restore(id)?;
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn add_subtask_and_enqueue(
        &mut self,
        task_id: &str,
        text: &str,
        now_ms: i64,
    ) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(task_id).map(|i| i.version).unwrap_or(0);
        let item = self.store.add_subtask(task_id, text)?;
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn toggle_subtask_and_enqueue(
        &mut self,
        task_id: &str,
        subtask_id: &str,
        now_ms: i64,
    ) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(task_id).map(|i| i.version).unwrap_or(0);
        let item = self.store.toggle_subtask(task_id, subtask_id)?;
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn remove_subtask_and_enqueue(
        &mut self,
        task_id: &str,
        subtask_id: &str,
        now_ms: i64,
    ) -> Result<TodoItem, String> {
        let prev_version = self.store.find_any(task_id).map(|i| i.version).unwrap_or(0);
        let item = self.store.remove_subtask(task_id, subtask_id)?;
        let _ = self
            .sync
            .remove_conflict(&format!("{task_id}:subtasks.{subtask_id}.text"));
        self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
        Ok(item)
    }

    fn resolve_conflict(&mut self, conflict_id: &str, choice: &str, now_ms: i64) -> Result<bool, String> {
        let Some(conflict) = self.sync.conflict(conflict_id).cloned() else {
            return Err("Conflito não encontrado.".into());
        };
        let chosen_text = match choice {
            "local" => conflict.local_text,
            "remote" => conflict.remote_text,
            _ => return Err("Escolha inválida (use 'local' ou 'remote').".into()),
        };
        let updated = self
            .store
            .apply_conflict_choice(&conflict.entity_id, &conflict.field, &chosen_text)?;
        self.sync.remove_conflict(conflict_id)?;
        if let Some((prev_version, item)) = updated {
            self.sync.enqueue_local_mutation(&item, prev_version, now_ms)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[derive(Default)]
struct SyncRuntimeState {
    creds_fingerprint: Option<[u8; 32]>,
    paused_fatal_or_revoked: bool,
    backoff_secs: u64,
}

impl SyncRuntimeState {
    fn creds_fingerprint(
        target: &crate::phone_link::relay_client::Target,
        creds: &crate::phone_link::relay_client::Credentials,
    ) -> [u8; 32] {
        let mut ctx = digest::Context::new(&digest::SHA256);
        ctx.update(target.http_base_url().as_bytes());
        ctx.update(b"|");
        ctx.update(creds.room.as_bytes());
        ctx.update(b"|");
        ctx.update(creds.access.as_bytes());
        let d = ctx.finish();
        let mut out = [0u8; 32];
        out.copy_from_slice(d.as_ref());
        out
    }

    fn next_backoff_secs(current: u64) -> u64 {
        SYNC_BACKOFF_STEPS_SECS
            .iter()
            .copied()
            .find(|&step| step > current)
            .unwrap_or(*SYNC_BACKOFF_STEPS_SECS.last().unwrap())
    }

    fn sync_credentials(&mut self, fp: [u8; 32]) -> bool {
        if self.creds_fingerprint == Some(fp) {
            return false;
        }
        self.creds_fingerprint = Some(fp);
        self.paused_fatal_or_revoked = false;
        self.backoff_secs = 0;
        true
    }

    fn record_outcome(&mut self, pause: Option<SyncPauseReason>) -> Duration {
        match pause {
            None => {
                self.paused_fatal_or_revoked = false;
                self.backoff_secs = 0;
                Duration::from_secs(SYNC_POLL_INTERVAL_SECS)
            }
            Some(SyncPauseReason::Revoked | SyncPauseReason::Fatal) => {
                self.paused_fatal_or_revoked = true;
                self.backoff_secs = 0;
                Duration::from_secs(SYNC_POLL_INTERVAL_SECS)
            }
            Some(SyncPauseReason::Backoff) => {
                self.paused_fatal_or_revoked = false;
                self.backoff_secs = Self::next_backoff_secs(self.backoff_secs);
                Duration::from_secs(self.backoff_secs)
            }
        }
    }
}

pub struct TaskManager {
    local: Mutex<LocalState>,
    voice: Mutex<Option<VoiceSession>>,
    fired: Mutex<HashMap<String, (u32, i64)>>,
    wake_tx: Sender<()>,
    sync_wake_tx: Sender<()>,
    sync_exec_lock: Mutex<()>,
    sync_runtime: Mutex<SyncRuntimeState>,
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
    let (sync_wake_tx, sync_wake_rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let dir = folder();
    let manager = Arc::new(TaskManager {
        local: Mutex::new(LocalState {
            store: TaskStore::load_at(dir.clone()),
            sync: SyncStore::load_at(dir),
        }),
        voice: Mutex::new(None),
        fired: Mutex::new(HashMap::new()),
        wake_tx,
        sync_wake_tx,
        sync_exec_lock: Mutex::new(()),
        sync_runtime: Mutex::new(SyncRuntimeState::default()),
        stop_flag: stop_flag.clone(),
    });
    app.manage(manager.clone());

    let handle = app.clone();
    let sched_mgr = manager.clone();
    let sched_stop = stop_flag.clone();
    std::thread::Builder::new()
        .name("lumen-task-scheduler".into())
        .spawn(move || scheduler_loop(handle, sched_mgr, wake_rx, sched_stop))
        .ok();

    let sync_handle = app.clone();
    std::thread::Builder::new()
        .name("lumen-task-sync".into())
        .spawn(move || sync_loop(sync_handle, manager, sync_wake_rx, stop_flag))
        .ok();
}

pub fn shutdown(app: &AppHandle) {
    let Some(mgr) = get(app) else { return };
    mgr.stop_flag.store(true, Ordering::Relaxed);
    if let Some(mut session) = mgr.voice.lock().unwrap().take() {
        session.capture.stop();
    }
    let _ = mgr.wake_tx.send(());
    let _ = mgr.sync_wake_tx.send(());
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

fn dismiss_if_active(app: &AppHandle, mgr: &TaskManager, item_id: &str) {
    mgr.fired.lock().unwrap().remove(item_id);
    clear_task_toast(app, item_id);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-dismiss", item_id.to_string());
    let _ = app.emit_to(crate::HUB_LABEL, "task-dismiss", item_id.to_string());
}

fn run_sync_once_for_app(
    app: &AppHandle,
    mgr: &TaskManager,
    manual: bool,
) -> Result<Option<(SyncCycleOutcome, Duration)>, String> {
    let Some((target, creds)) = crate::phone_link::mailbox_credentials(app) else {
        *mgr.sync_runtime.lock().unwrap() = SyncRuntimeState::default();
        return Ok(None);
    };
    let fp = SyncRuntimeState::creds_fingerprint(&target, &creds);
    let _exec_guard = mgr.sync_exec_lock.lock().unwrap();

    {
        let mut rt = mgr.sync_runtime.lock().unwrap();
        let changed = rt.sync_credentials(fp);
        if manual {
            rt.paused_fatal_or_revoked = false;
            rt.backoff_secs = 0;
        } else if !changed && rt.paused_fatal_or_revoked {
            return Ok(None);
        }
    }

    let transport = RelayMailboxTransport;
    let now_ms = now_utc_ms();
    let outcome = sync::run_sync_cycle(&target, &creds, &transport, now_ms, |op| {
        let mut local = mgr.local.lock().unwrap();
        let LocalState { store, sync } = &mut *local;
        op(store, sync)
    })?;

    let next_wait = {
        let mut rt = mgr.sync_runtime.lock().unwrap();
        rt.sync_credentials(fp);
        rt.record_outcome(outcome.pause)
    };

    for id in &outcome.dismiss_ids {
        dismiss_if_active(app, mgr, id);
    }
    if outcome.changed_tasks {
        let _ = mgr.wake_tx.send(());
    }
    if outcome.changed_tasks || outcome.status_changed {
        emit_tasks_changed(app);
    }

    Ok(Some((outcome, next_wait)))
}

fn sync_loop(
    app: AppHandle,
    mgr: Arc<TaskManager>,
    sync_wake_rx: mpsc::Receiver<()>,
    stop_flag: Arc<AtomicBool>,
) {
    while !stop_flag.load(Ordering::Relaxed) {
        let wait = match run_sync_once_for_app(&app, &mgr, false) {
            Ok(Some((_, next_wait))) => next_wait,
            Ok(None) | Err(_) => Duration::from_secs(SYNC_POLL_INTERVAL_SECS),
        };
        match sync_wake_rx.recv_timeout(wait) {
            Ok(()) => {
                while sync_wake_rx.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn next_wait_duration(mgr: &TaskManager, now_ms: i64) -> Duration {
    let items = mgr.local.lock().unwrap().store.active_items();
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
    let items = mgr.local.lock().unwrap().store.active_items();
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

/// Calcula se a tarefa deve disparar aviso em `now_ms`, evitando duplicidade para o mesmo `trigger_ms`.
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
        if last_fired.map(|(_, fired_ms)| fired_ms) == Some(trigger_ms) {
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
            if let Ok(item) = mgr
                .local
                .lock()
                .unwrap()
                .snooze_and_enqueue(task_id, Some(TODO_ALARM_SNOOZE_MINUTES), now)
            {
                dismiss_if_active(app, &mgr, task_id);
                let _ = mgr.wake_tx.send(());
                let _ = mgr.sync_wake_tx.send(());
                emit_tasks_changed(app);
                let _ = app.emit_to(crate::island::WINDOW_LABEL, "task-snoozed", &item.id);
            }
        }
        TaskToastAction::Complete => {
            if let Ok((outcome, changed)) = mgr
                .local
                .lock()
                .unwrap()
                .toggle_done_and_enqueue(task_id, Some(true), now)
            {
                dismiss_if_active(app, &mgr, task_id);
                let _ = mgr.wake_tx.send(());
                if changed || outcome.next_occurrence.is_some() {
                    let _ = mgr.sync_wake_tx.send(());
                }
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
    let creds = crate::phone_link::mailbox_credentials(app);
    let paired = creds.is_some();
    let mut room_changed = false;
    let (items, shortcuts, quarantined_file, sync) = {
        let mut local = mgr.local.lock().unwrap();
        if let Some((_, ref c)) = creds {
            let LocalState { store, sync } = &mut *local;
            if let Ok(changed) = sync.ensure_room(&c.room, store.all_items(), now_utc_ms()) {
                room_changed = changed;
            }
        }
        (
            local.store.active_items(),
            local.store.shortcut_settings().clone(),
            local.store.quarantined_file().map(str::to_string),
            local.sync.status(paired),
        )
    };
    if let Some((ref target, ref c)) = creds {
        let fp = SyncRuntimeState::creds_fingerprint(target, c);
        let creds_changed = {
            let mut rt = mgr.sync_runtime.lock().unwrap();
            let changed = rt.sync_credentials(fp);
            if room_changed {
                rt.paused_fatal_or_revoked = false;
                rt.backoff_secs = 0;
            }
            changed
        };
        if room_changed || creds_changed {
            let _ = mgr.sync_wake_tx.send(());
        }
    } else {
        *mgr.sync_runtime.lock().unwrap() = SyncRuntimeState::default();
    }

    let recording = mgr.voice.lock().unwrap().is_some();
    #[cfg(windows)]
    let windows_notifications_available = app
        .try_state::<crate::permission_notifications::PermissionToasts>()
        .is_some_and(|t| t.is_available());
    #[cfg(not(windows))]
    let windows_notifications_available = false;

    TasksSnapshot {
        items,
        shortcuts,
        windows_notifications_available,
        voice_ready: speech::is_ready(),
        recording,
        quarantined_file,
        sync,
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
    let item = mgr.local.lock().unwrap().create_and_enqueue(
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
    let _ = mgr.sync_wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_create(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    input: CreateTaskInput,
) -> Result<TodoItem, String> {
    let item = mgr
        .local
        .lock()
        .unwrap()
        .create_and_enqueue(input, now_utc_ms())?;
    let _ = mgr.wake_tx.send(());
    let _ = mgr.sync_wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_update(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    input: UpdateTaskInput,
) -> Result<TodoItem, String> {
    let item = mgr
        .local
        .lock()
        .unwrap()
        .update_and_enqueue(input, now_utc_ms())?;
    dismiss_if_active(&app, &mgr, &item.id);
    let _ = mgr.wake_tx.send(());
    let _ = mgr.sync_wake_tx.send(());
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
    let (outcome, changed) = mgr
        .local
        .lock()
        .unwrap()
        .toggle_done_and_enqueue(&id, done, now_utc_ms())?;
    dismiss_if_active(&app, &mgr, &id);
    let _ = mgr.wake_tx.send(());
    if changed || outcome.next_occurrence.is_some() {
        let _ = mgr.sync_wake_tx.send(());
    }
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
        .local
        .lock()
        .unwrap()
        .snooze_and_enqueue(&id, minutes, now_utc_ms())?;
    dismiss_if_active(&app, &mgr, &id);
    let _ = mgr.wake_tx.send(());
    let _ = mgr.sync_wake_tx.send(());
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
    let item = mgr
        .local
        .lock()
        .unwrap()
        .delete_and_enqueue(&id, now_utc_ms())?;
    dismiss_if_active(&app, &mgr, &id);
    let _ = mgr.wake_tx.send(());
    let _ = mgr.sync_wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_restore(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    id: String,
) -> Result<TodoItem, String> {
    let item = mgr
        .local
        .lock()
        .unwrap()
        .restore_and_enqueue(&id, now_utc_ms())?;
    let _ = mgr.wake_tx.send(());
    let _ = mgr.sync_wake_tx.send(());
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
    let item = mgr
        .local
        .lock()
        .unwrap()
        .add_subtask_and_enqueue(&task_id, &text, now_utc_ms())?;
    let _ = mgr.sync_wake_tx.send(());
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
    let item = mgr.local.lock().unwrap().toggle_subtask_and_enqueue(
        &task_id,
        &subtask_id,
        now_utc_ms(),
    )?;
    let _ = mgr.sync_wake_tx.send(());
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
    let item = mgr.local.lock().unwrap().remove_subtask_and_enqueue(
        &task_id,
        &subtask_id,
        now_utc_ms(),
    )?;
    let _ = mgr.sync_wake_tx.send(());
    emit_tasks_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn tasks_save_shortcut_settings(
    app: AppHandle,
    mgr: State<Arc<TaskManager>>,
    settings: TodoShortcutSettings,
) -> Result<TodoShortcutSettings, String> {
    mgr.local
        .lock()
        .unwrap()
        .store
        .set_shortcut_settings(settings.clone())?;
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
        .local
        .lock()
        .unwrap()
        .store
        .get(&id)
        .cloned()
        .ok_or_else(|| "Tarefa não encontrada.".to_string())?;
    dispatch_reminder(&app, &item);
    Ok(())
}

#[tauri::command]
pub async fn tasks_sync_now(app: AppHandle) -> Result<TasksSnapshot, String> {
    let mgr = get(&app).ok_or_else(|| "Gerenciador de tarefas indisponível.".to_string())?;
    let app_clone = app.clone();
    let mgr_clone = mgr.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_sync_once_for_app(&app_clone, &mgr_clone, true).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(build_snapshot(&app, &mgr))
}

#[tauri::command]
pub fn tasks_resolve_conflict(
    app: AppHandle,
    conflict_id: String,
    choice: String,
) -> Result<TasksSnapshot, String> {
    let mgr = get(&app).ok_or_else(|| "Gerenciador de tarefas indisponível.".to_string())?;
    let now = now_utc_ms();
    let changed = mgr
        .local
        .lock()
        .unwrap()
        .resolve_conflict(&conflict_id, &choice, now)?;
    if changed {
        let _ = mgr.sync_wake_tx.send(());
    }
    emit_tasks_changed(&app);
    Ok(build_snapshot(&app, &mgr))
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
    use self::sync::{run_sync_cycle, MailboxTransport, SyncEnvelope};
    use crate::phone_link::relay_client::{
        Credentials, MailboxAckResponse, MailboxEnvelopeItem, MailboxHttpError,
        MailboxListResponse, MailboxPostOutcome, MailboxStatsResponse, Target,
    };
    use crate::phone_link::relay_crypto::{
        base64url_encode, k_mailbox, seal_mailbox, MailboxRole, PairingKey,
    };
    use model::{TodoRecurrence, TodoRepeatFrequency, TODO_TIME_ZONE};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-tasks-mod-{name}-{}-{}",
            std::process::id(),
            sync::new_uuid_v4()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_creds(seed: u8) -> (Target, Credentials) {
        (
            Target {
                tls: false,
                host: "127.0.0.1".into(),
                port: 8787,
            },
            Credentials {
                room: base64url_encode(&[seed; 16]),
                key: PairingKey::from_bytes([seed.wrapping_add(1); 32]),
                access: base64url_encode(&[seed.wrapping_add(2); 32]),
            },
        )
    }

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

    #[test]
    fn remote_text_or_priority_edit_does_not_refire_but_new_schedule_or_cleared_fired_does() {
        // Criada às 13:00 local (16:00Z), vence 14:30 local com alerta 20m -> dispara 14:10 local.
        let mut item = sample_item("2026-11-14T16:00:00Z", "2026-11-14", "14:30", 20);
        let trigger_14_10 = utc_ms_from_local(2026, 11, 14, 14, 10, TODO_TIME_ZONE);
        let at_14_10_05 = trigger_14_10 + 5_000;
        let at_14_12 = trigger_14_10 + 120_000; // ainda dentro de TRIGGER_GRACE_MS (5m)

        assert_eq!(due_trigger_for_now(&item, at_14_10_05, None), Some(trigger_14_10));

        // Edição remota só de texto/prioridade/subtarefa muda `version`, mantendo o mesmo `trigger_ms`.
        item.text = "Enviar orçamento revisado".into();
        item.priority = TodoPriority::Urgent;
        item.subtasks.push(model::TodoSubtask {
            id: "sub-1".into(),
            text: "Anexar planilha".into(),
            done: false,
        });
        item.version = 2;
        assert_eq!(
            due_trigger_for_now(&item, at_14_12, Some((1, trigger_14_10))),
            None,
            "mesmo trigger_ms não deve re-disparar quando só a versão mudou"
        );

        // Se `fired` tiver sido limpo explicitamente, volta a disparar dentro da janela de tolerância.
        assert_eq!(due_trigger_for_now(&item, at_14_12, None), Some(trigger_14_10));

        // Novo `snoozed_until` muda `trigger_ms`: cancela o anterior e dispara no novo horário.
        let trigger_14_25 = utc_ms_from_local(2026, 11, 14, 14, 25, TODO_TIME_ZONE);
        item.snoozed_until = Some("2026-11-14T17:25:00Z".into());
        item.version = 3;
        assert_eq!(
            due_trigger_for_now(&item, at_14_12, Some((1, trigger_14_10))),
            None
        );
        assert_eq!(
            due_trigger_for_now(&item, trigger_14_25 + 5_000, Some((1, trigger_14_10))),
            Some(trigger_14_25)
        );
        assert_eq!(
            due_trigger_for_now(&item, trigger_14_25 + 5_000, None),
            Some(trigger_14_25)
        );

        // Nova edição de texto após disparo do snooze também não re-dispara.
        item.text = "Enviar orçamento final".into();
        item.version = 4;
        assert_eq!(
            due_trigger_for_now(&item, trigger_14_25 + 60_000, Some((3, trigger_14_25))),
            None
        );

        // Alterar `due_time` (limpando `snoozed_until`) produz novo `trigger_ms` e volta a disparar.
        item.snoozed_until = None;
        item.due_time = Some("15:00".into()); // alerta 20m -> 14:40 local
        item.version = 5;
        let trigger_14_40 = utc_ms_from_local(2026, 11, 14, 14, 40, TODO_TIME_ZONE);
        assert_eq!(
            due_trigger_for_now(&item, trigger_14_40 + 5_000, Some((3, trigger_14_25))),
            Some(trigger_14_40)
        );
    }

    #[test]
    fn local_state_enqueues_all_mutations_and_recurrence_two_entries() {
        let dir = temp_dir("mutations");
        let mut local = LocalState {
            store: TaskStore::load_at(dir.clone()),
            sync: SyncStore::load_at(dir.clone()),
        };
        let t0 = 1_763_120_000_000i64;
        let created = local
            .create_and_enqueue(
                CreateTaskInput {
                    text: "Revisar contrato".into(),
                    due: None,
                    due_date: Some("2026-11-20".into()),
                    due_time: Some("10:00".into()),
                    time_zone: None,
                    alert_minutes: Some(15),
                    priority: None,
                    subtasks: Vec::new(),
                    recurrence: Some(TodoRecurrence {
                        frequency: TodoRepeatFrequency::Daily,
                        weekdays: Vec::new(),
                        day_of_month: None,
                    }),
                    source: None,
                },
                t0,
            )
            .unwrap();
        assert_eq!(local.sync.outbox().len(), 1);
        assert_eq!(local.sync.outbox()[0].envelope.base_version, 0);
        assert_eq!(local.sync.outbox()[0].envelope.new_version, 1);

        let updated = local
            .update_and_enqueue(
                UpdateTaskInput {
                    id: created.id.clone(),
                    text: Some("Revisar contrato anual".into()),
                    due: None,
                    due_date: None,
                    due_time: None,
                    time_zone: None,
                    alert_minutes: None,
                    priority: Some(TodoPriority::High),
                    recurrence: None,
                    clear_snooze: false,
                },
                t0 + 1000,
            )
            .unwrap();
        assert_eq!(updated.version, 2);
        // Coalescida mantendo base_version = 0 e new_version = 2
        assert_eq!(local.sync.outbox().len(), 1);
        assert_eq!(local.sync.outbox()[0].envelope.base_version, 0);
        assert_eq!(local.sync.outbox()[0].envelope.new_version, 2);

        let with_sub = local
            .add_subtask_and_enqueue(&created.id, "Checar anexo", t0 + 2000)
            .unwrap();
        let sub_id = with_sub.subtasks[0].id.clone();
        local
            .toggle_subtask_and_enqueue(&created.id, &sub_id, t0 + 3000)
            .unwrap();
        local
            .snooze_and_enqueue(&created.id, Some(10), t0 + 4000)
            .unwrap();
        local
            .remove_subtask_and_enqueue(&created.id, &sub_id, t0 + 5000)
            .unwrap();

        // Toggle done em tarefa recorrente gera 2 entradas distintas na outbox (concluída + next_occurrence)
        let (outcome, changed) = local
            .toggle_done_and_enqueue(&created.id, Some(true), t0 + 6000)
            .unwrap();
        assert!(changed);
        let next = outcome.next_occurrence.expect("gera próxima ocorrência");
        assert_eq!(local.sync.outbox().len(), 2);
        assert_eq!(local.sync.outbox()[0].envelope.entity_id, created.id);
        assert_eq!(local.sync.outbox()[1].envelope.entity_id, next.id);
        assert_eq!(local.sync.outbox()[1].envelope.base_version, 0);
        assert_eq!(local.sync.outbox()[1].envelope.new_version, 1);

        // Toggle redundante (já done = true) não cria mutação extra
        let (_, noop_changed) = local
            .toggle_done_and_enqueue(&created.id, Some(true), t0 + 7000)
            .unwrap();
        assert!(!noop_changed);
        assert_eq!(local.sync.outbox().len(), 2);

        // Delete + restore mantêm entidade tombstonada/restaurada na outbox
        local.delete_and_enqueue(&next.id, t0 + 8000).unwrap();
        let deleted = local.store.find_any(&next.id).unwrap().clone();
        assert!(deleted.is_tombstoned());
        local.restore_and_enqueue(&next.id, t0 + 9000).unwrap();
        let restored = local.store.find_any(&next.id).unwrap().clone();
        assert!(!restored.is_tombstoned());

        let _ = std::fs::remove_dir_all(&dir);
    }

    struct OneShotMailbox {
        to_pc: Mutex<Vec<MailboxEnvelopeItem>>,
        acked: Mutex<u64>,
        posted: Mutex<usize>,
    }

    impl MailboxTransport for OneShotMailbox {
        fn post(
            &self,
            _target: &Target,
            _creds: &Credentials,
            _to: MailboxRole,
            msg_id: &str,
            _body: &[u8],
        ) -> Result<MailboxPostOutcome, MailboxHttpError> {
            *self.posted.lock().unwrap() += 1;
            Ok(MailboxPostOutcome {
                seq: 1,
                id: msg_id.to_string(),
                duplicate: false,
            })
        }

        fn list(
            &self,
            _target: &Target,
            _creds: &Credentials,
            _for_role: MailboxRole,
            after_seq: u64,
            _limit: u32,
        ) -> Result<MailboxListResponse, MailboxHttpError> {
            let messages: Vec<_> = self
                .to_pc
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.seq > after_seq)
                .cloned()
                .collect();
            let latest_seq = messages.last().map(|m| m.seq).unwrap_or(after_seq);
            Ok(MailboxListResponse {
                messages,
                latest_seq,
                pending_count: 0,
            })
        }

        fn ack(
            &self,
            _target: &Target,
            _creds: &Credentials,
            _for_role: MailboxRole,
            upto_seq: u64,
        ) -> Result<MailboxAckResponse, MailboxHttpError> {
            let mut acked = self.acked.lock().unwrap();
            *acked = (*acked).max(upto_seq);
            Ok(MailboxAckResponse {
                acked_upto: *acked,
                deleted: 1,
                remaining: 0,
            })
        }

        fn stats(
            &self,
            _target: &Target,
            _creds: &Credentials,
        ) -> Result<MailboxStatsResponse, MailboxHttpError> {
            Ok(MailboxStatsResponse {
                pending_pc: 0,
                pending_phone: 0,
                stored_bytes: 0,
                revoked: false,
            })
        }
    }

    #[test]
    fn local_state_resolve_conflict_updates_item_and_clears_conflict() {
        let dir = temp_dir("conflict");
        let (target, creds) = test_creds(9);
        let mut local = LocalState {
            store: TaskStore::load_at(dir.clone()),
            sync: SyncStore::load_at(dir.clone()),
        };
        let t0 = 1_763_120_000_000i64;
        let created = local
            .create_and_enqueue(
                CreateTaskInput {
                    text: "Texto base".into(),
                    due: None,
                    due_date: None,
                    due_time: None,
                    time_zone: None,
                    alert_minutes: None,
                    priority: None,
                    subtasks: Vec::new(),
                    recurrence: None,
                    source: None,
                },
                t0,
            )
            .unwrap();

        // Sincroniza v1 inicialmente para estabelecer base_copy
        let empty_transport = OneShotMailbox {
            to_pc: Mutex::new(Vec::new()),
            acked: Mutex::new(0),
            posted: Mutex::new(0),
        };
        run_sync_cycle(&target, &creds, &empty_transport, t0 + 1000, |op| {
            let LocalState { store, sync } = &mut local;
            op(store, sync)
        })
        .unwrap();

        // Edição local concorda em conflito com edição remota
        local
            .update_and_enqueue(
                UpdateTaskInput {
                    id: created.id.clone(),
                    text: Some("Título do PC".into()),
                    due: None,
                    due_date: None,
                    due_time: None,
                    time_zone: None,
                    alert_minutes: None,
                    priority: None,
                    recurrence: None,
                    clear_snooze: false,
                },
                t0 + 2000,
            )
            .unwrap();

        let mut remote = local.store.find_any(&created.id).unwrap().clone();
        remote.text = "Título do Celular".into();
        remote.version = 2;
        let remote_env = SyncEnvelope {
            contract_version: 1,
            mutation_id: "mut-remote-conflict".into(),
            device_id: "phone-1".into(),
            origin_role: "phone".into(),
            entity_type: "task".into(),
            entity_id: remote.id.clone(),
            base_version: 1,
            new_version: 2,
            updated_at: "2026-11-14T23:59:59Z".into(),
            payload: serde_json::to_value(&remote).unwrap(),
        };
        let km = k_mailbox(&creds.key);
        let sealed = seal_mailbox(
            &km,
            &creds.room,
            MailboxRole::Pc,
            &serde_json::to_string(&remote_env).unwrap(),
        )
        .unwrap();
        let conflict_transport = OneShotMailbox {
            to_pc: Mutex::new(vec![MailboxEnvelopeItem {
                seq: 1,
                id: "mut-remote-conflict".into(),
                created_at: 1,
                size_bytes: sealed.len(),
                payload: sealed,
            }]),
            acked: Mutex::new(0),
            posted: Mutex::new(0),
        };
        run_sync_cycle(&target, &creds, &conflict_transport, t0 + 3000, |op| {
            let LocalState { store, sync } = &mut local;
            op(store, sync)
        })
        .unwrap();

        let conflict_id = format!("{}:text", created.id);
        assert!(local.sync.conflict(&conflict_id).is_some());
        assert!(local
            .resolve_conflict(&conflict_id, "invalida", t0 + 4000)
            .is_err());

        let applied = local
            .resolve_conflict(&conflict_id, "local", t0 + 5000)
            .unwrap();
        assert!(applied);
        assert!(local.sync.conflict(&conflict_id).is_none());
        assert_eq!(local.store.find_any(&created.id).unwrap().text, "Título do PC");
        assert_eq!(local.sync.outbox().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_runtime_state_backoff_and_credential_reset() {
        let mut rt = SyncRuntimeState::default();
        let (target1, creds1) = test_creds(1);
        let (target2, creds2) = test_creds(2);
        let fp1 = SyncRuntimeState::creds_fingerprint(&target1, &creds1);
        let fp2 = SyncRuntimeState::creds_fingerprint(&target2, &creds2);

        assert!(rt.sync_credentials(fp1));
        assert!(!rt.sync_credentials(fp1));

        let expected = [5, 15, 30, 60, 120, 300, 300];
        for secs in expected {
            assert_eq!(
                rt.record_outcome(Some(SyncPauseReason::Backoff)),
                Duration::from_secs(secs)
            );
            assert!(!rt.paused_fatal_or_revoked);
        }

        rt.record_outcome(Some(SyncPauseReason::Revoked));
        assert!(rt.paused_fatal_or_revoked);
        assert_eq!(rt.backoff_secs, 0);

        // Novas credenciais limpam pausa fatal/revogada e backoff
        assert!(rt.sync_credentials(fp2));
        assert!(!rt.paused_fatal_or_revoked);
        assert_eq!(rt.backoff_secs, 0);
    }
}
