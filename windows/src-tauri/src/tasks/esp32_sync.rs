//! Sincronização bidirecional idempotente de tarefas entre a placa ESP32-S3 e o `TaskStore` + outbox L6.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use super::model::{
    format_date, local_parts_at, parse_date, parse_time, utc_ms_from_local, TodoItem,
    TodoPriority, TODO_DEFAULT_ALERT_MINUTES, TODO_TIME_ZONE,
};
use super::store::format_due_label;
use super::{emit_task_sync_state, emit_tasks_changed, get, LocalState};
use crate::whatsapp::gateway::{iso_utc, rfc3339_ms};

pub const ESP32_HTTP_MAX_TASKS: usize = 24;
pub const ESP32_HTTP_MAX_BYTES: usize = 3_800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Esp32TaskPayload {
    pub id: String,
    pub text: String,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Esp32TaskEnvelope {
    pub contract_version: u32,
    pub entity_id: String,
    pub updated_at: i64,
    pub origin_role: String,
    pub payload: Esp32TaskPayload,
}

fn parse_updated_ms(val: Option<&Value>, fallback_ms: i64) -> i64 {
    let Some(v) = val else {
        return fallback_ms;
    };
    if let Some(n) = v.as_i64() {
        if n <= 0 {
            return fallback_ms;
        }
        return if n > 100_000_000_000 { n } else { n * 1000 };
    }
    if let Some(f) = v.as_f64() {
        if f <= 0.0 {
            return fallback_ms;
        }
        let n = f as i64;
        return if n > 100_000_000_000 { n } else { n * 1000 };
    }
    if let Some(s) = v.as_str() {
        if let Some(ms) = rfc3339_ms(Some(s)) {
            return ms;
        }
        if let Ok(n) = s.trim().parse::<i64>() {
            if n > 0 {
                return if n > 100_000_000_000 { n } else { n * 1000 };
            }
        }
    }
    fallback_ms
}

fn due_ts_to_local(due_ts: Option<i64>) -> (Option<String>, Option<String>) {
    let Some(ts_sec) = due_ts.filter(|&v| v > 0) else {
        return (None, None);
    };
    let (y, m, d, hh, mm) = local_parts_at(ts_sec * 1000, TODO_TIME_ZONE);
    (Some(format_date(y, m, d)), Some(format!("{hh:02}:{mm:02}")))
}

fn local_to_due_ts(due_date: Option<&str>, due_time: Option<&str>) -> Option<i64> {
    let d_str = due_date?;
    let (y, m, d) = parse_date(d_str)?;
    let (hh, mm) = due_time.and_then(parse_time).unwrap_or((9, 0));
    let ms = utc_ms_from_local(y, m, d, hh, mm, TODO_TIME_ZONE);
    Some((ms / 1000).max(0))
}

fn item_updated_ms(local: &LocalState, item: &TodoItem) -> i64 {
    local
        .sync
        .outbox()
        .iter()
        .find(|e| e.envelope.entity_id == item.id)
        .and_then(|e| rfc3339_ms(Some(&e.envelope.updated_at)))
        .or_else(|| rfc3339_ms(item.done_at.as_deref()))
        .or_else(|| rfc3339_ms(Some(&item.created_at)))
        .unwrap_or(0)
}

fn to_envelope(local: &LocalState, item: &TodoItem) -> Value {
    let updated_secs = (item_updated_ms(local, item) / 1000).max(0);
    let due_ts = local_to_due_ts(item.due_date.as_deref(), item.due_time.as_deref());
    let text: String = item.text.chars().take(95).collect();
    let mut payload = json!({
        "id": item.id,
        "text": text,
        "done": item.done,
    });
    if let Some(ts) = due_ts {
        payload["due_ts"] = json!(ts);
    }
    json!({
        "contract_version": 1,
        "entity_id": item.id,
        "updated_at": updated_secs,
        "origin_role": "pc",
        "payload": payload,
    })
}

pub(crate) fn export_from_local(
    local: &LocalState,
    priority_ids: &HashSet<String>,
    max_items: usize,
    max_bytes: usize,
) -> Vec<Value> {
    let mut items = local.store.active_items();
    items.sort_by(|a, b| {
        let a_prio = priority_ids.contains(&a.id);
        let b_prio = priority_ids.contains(&b.id);
        b_prio
            .cmp(&a_prio)
            .then_with(|| a.done.cmp(&b.done))
            .then_with(|| item_updated_ms(local, b).cmp(&item_updated_ms(local, a)))
    });

    let mut out = Vec::new();
    for item in items.into_iter().take(max_items) {
        let env = to_envelope(local, &item);
        out.push(env);
        let probe = json!({
            "ok": true,
            "envelopes": &out,
            "count": out.len(),
        });
        let within = serde_json::to_vec(&probe)
            .map(|b| b.len() <= max_bytes)
            .unwrap_or(false);
        if !within {
            out.pop();
            break;
        }
    }
    out
}

pub(crate) fn sync_esp32_local(
    local: &mut LocalState,
    incoming: &[Value],
    now_ms: i64,
) -> Result<(Vec<Value>, bool), String> {
    let mut changed = false;
    let mut incoming_ids = HashSet::new();

    for raw in incoming {
        let Some(entity_id_raw) = raw
            .get("entity_id")
            .and_then(Value::as_str)
            .or_else(|| raw.get("payload").and_then(|p| p.get("id")).and_then(Value::as_str))
        else {
            continue;
        };
        let entity_id = entity_id_raw.trim();
        if entity_id.is_empty() || entity_id.len() > 128 {
            continue;
        }
        incoming_ids.insert(entity_id.to_string());

        let Some(payload) = raw.get("payload") else {
            continue;
        };
        let text: String = payload
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .chars()
            .take(500)
            .collect();
        let done = payload.get("done").and_then(Value::as_bool).unwrap_or(false);
        let due_ts = payload.get("due_ts").and_then(Value::as_i64);
        let (due_date, due_time) = due_ts_to_local(due_ts);
        let incoming_updated_ms = parse_updated_ms(raw.get("updated_at"), now_ms);

        if let Some(existing) = local.store.find_any(entity_id).cloned() {
            if existing.is_tombstoned() {
                let deleted_ms = rfc3339_ms(existing.deleted_at.as_deref()).unwrap_or(0);
                if deleted_ms >= incoming_updated_ms {
                    continue;
                }
            }

            // Guarda de idempotência contra `updated_at = time(NULL)` emitido por `tasks_ui.c`:
            // se conteúdo e agenda não mudaram, não muta versão nem reenfileira na outbox L6.
            if !existing.is_tombstoned()
                && existing.text == text
                && existing.done == done
                && existing.due_date == due_date
                && existing.due_time == due_time
            {
                continue;
            }

            let existing_ms = item_updated_ms(local, &existing);
            if incoming_updated_ms < existing_ms && !existing.is_tombstoned() {
                continue;
            }

            if text.is_empty() {
                continue;
            }

            let prev_version = existing.version;
            let mut updated = existing;
            updated.text = text;
            updated.done = done;
            updated.done_at = if done {
                Some(
                    updated
                        .done_at
                        .unwrap_or_else(|| iso_utc(incoming_updated_ms)),
                )
            } else {
                None
            };
            updated.due = format_due_label(due_date.as_deref(), due_time.as_deref());
            updated.due_date = due_date;
            updated.due_time = due_time;
            updated.deleted_at = None;
            updated.version = prev_version.saturating_add(1);

            local.store.upsert_remote(updated.clone())?;
            local
                .sync
                .enqueue_local_mutation(&updated, prev_version, now_ms)?;
            changed = true;
        } else {
            if text.is_empty() {
                continue;
            }
            let new_item = TodoItem {
                id: entity_id.to_string(),
                text,
                due: format_due_label(due_date.as_deref(), due_time.as_deref()),
                due_date,
                due_time,
                time_zone: TODO_TIME_ZONE.to_string(),
                alert_minutes: TODO_DEFAULT_ALERT_MINUTES,
                priority: TodoPriority::Normal,
                subtasks: Vec::new(),
                recurrence: None,
                series_id: None,
                done,
                created_at: iso_utc(incoming_updated_ms),
                done_at: done.then(|| iso_utc(incoming_updated_ms)),
                snoozed_until: None,
                source: None,
                source_stale: false,
                version: 1,
                deleted_at: None,
            };
            local.store.upsert_remote(new_item.clone())?;
            local.sync.enqueue_local_mutation(&new_item, 0, now_ms)?;
            changed = true;
        }
    }

    let exported = export_from_local(
        local,
        &incoming_ids,
        ESP32_HTTP_MAX_TASKS,
        ESP32_HTTP_MAX_BYTES,
    );
    Ok((exported, changed))
}

pub fn sync_esp32_envelopes(
    app: &AppHandle,
    incoming: &[Value],
    now_ms: i64,
) -> Result<Vec<Value>, String> {
    let Some(mgr) = get(app) else {
        return Err("Gerenciador de tarefas indisponível.".into());
    };
    let (exported, changed) = {
        let mut local = mgr.local.lock().unwrap();
        sync_esp32_local(&mut local, incoming, now_ms)?
    };
    if changed {
        let _ = mgr.wake_tx.send(());
        let _ = mgr.sync_wake_tx.send(());
        emit_tasks_changed(app);
        emit_task_sync_state(app, &mgr);
    }
    Ok(exported)
}

pub fn export_esp32_envelopes(
    app: &AppHandle,
    max_items: usize,
    max_bytes: usize,
) -> Vec<Value> {
    let Some(mgr) = get(app) else {
        return Vec::new();
    };
    let local = mgr.local.lock().unwrap();
    export_from_local(&local, &HashSet::new(), max_items, max_bytes)
}

// -----------------------------------------------------------------------------
// Sincronização MQTT (PROTOCOLO.md — um tópico retido por tarefa)
// -----------------------------------------------------------------------------

pub(crate) fn to_mqtt_envelope(local: &LocalState, item: &TodoItem) -> Value {
    let updated_secs = (item_updated_ms(local, item) / 1000).max(0);
    let due_ts = local_to_due_ts(item.due_date.as_deref(), item.due_time.as_deref());
    let text: String = item.text.chars().take(95).collect();
    let mut payload = json!({
        "id": item.id,
        "text": text,
        "done": item.done,
    });
    if let Some(ts) = due_ts {
        payload["due_ts"] = json!(ts);
    }
    json!({
        "contract_version": 1,
        "entity_type": "todo",
        "entity_id": item.id,
        "origin_role": "lumen",
        "updated_at": updated_secs,
        "payload": payload,
    })
}

pub(crate) fn export_mqtt_tasks_local(local: &LocalState) -> Vec<(String, Value)> {
    local
        .store
        .active_items()
        .into_iter()
        .map(|item| {
            let id = item.id.clone();
            let env = to_mqtt_envelope(local, &item);
            (id, env)
        })
        .collect()
}

/// Aplica um envelope MQTT de tarefa (`sm/<sala>/tarefas/<entity_id>`).
/// Conflito: vence estritamente o maior `updated_at` (em segundos Unix); empate mantém o local.
pub(crate) fn apply_mqtt_task_local(
    local: &mut LocalState,
    topic_entity_id: &str,
    raw: &Value,
    now_ms: i64,
) -> Result<bool, String> {
    let entity_id = raw
        .get("entity_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| topic_entity_id.trim());
    if entity_id.is_empty() || entity_id.len() > 128 {
        return Ok(false);
    }
    let Some(payload) = raw.get("payload") else {
        return Ok(false);
    };
    let text: String = payload
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .chars()
        .take(500)
        .collect();
    if text.is_empty() {
        return Ok(false);
    }
    let done = payload.get("done").and_then(Value::as_bool).unwrap_or(false);
    let due_ts = payload.get("due_ts").and_then(Value::as_i64);
    let (due_date, due_time) = due_ts_to_local(due_ts);
    let incoming_updated_ms = parse_updated_ms(raw.get("updated_at"), now_ms);
    let incoming_secs = incoming_updated_ms / 1000;

    if let Some(existing) = local.store.find_any(entity_id).cloned() {
        if existing.is_tombstoned() {
            let deleted_secs = rfc3339_ms(existing.deleted_at.as_deref()).unwrap_or(0) / 1000;
            if incoming_secs <= deleted_secs {
                return Ok(false);
            }
        } else {
            if existing.text == text
                && existing.done == done
                && existing.due_date == due_date
                && existing.due_time == due_time
            {
                return Ok(false);
            }
            let existing_secs = item_updated_ms(local, &existing) / 1000;
            // Contrato MQTT: vence o maior updated_at; em empate mantém o local.
            if incoming_secs <= existing_secs {
                return Ok(false);
            }
        }

        let prev_version = existing.version;
        let mut updated = existing;
        updated.text = text;
        updated.done = done;
        updated.done_at = if done {
            Some(updated.done_at.unwrap_or_else(|| iso_utc(incoming_updated_ms)))
        } else {
            None
        };
        updated.due = format_due_label(due_date.as_deref(), due_time.as_deref());
        updated.due_date = due_date;
        updated.due_time = due_time;
        updated.deleted_at = None;
        updated.version = prev_version.saturating_add(1);

        local.store.upsert_remote(updated.clone())?;
        // Decisão L10.3: mutações recebidas da placa via MQTT não entram na outbox L6,
        // porque o celular (F17) já sincroniza diretamente pelos tópicos retidos `sm/<sala>/tarefas/+`.
        Ok(true)
    } else {
        let new_item = TodoItem {
            id: entity_id.to_string(),
            text,
            due: format_due_label(due_date.as_deref(), due_time.as_deref()),
            due_date,
            due_time,
            time_zone: TODO_TIME_ZONE.to_string(),
            alert_minutes: TODO_DEFAULT_ALERT_MINUTES,
            priority: TodoPriority::Normal,
            subtasks: Vec::new(),
            recurrence: None,
            series_id: None,
            done,
            created_at: iso_utc(incoming_updated_ms),
            done_at: done.then(|| iso_utc(incoming_updated_ms)),
            snoozed_until: None,
            source: None,
            source_stale: false,
            version: 1,
            deleted_at: None,
        };
        local.store.upsert_remote(new_item)?;
        Ok(true)
    }
}

/// Apaga localmente uma tarefa quando um payload vazio chega pelo MQTT (sem republicar e sem outbox L6).
pub(crate) fn delete_mqtt_task_local(
    local: &mut LocalState,
    entity_id: &str,
    now_ms: i64,
) -> Result<bool, String> {
    let id = entity_id.trim();
    if id.is_empty() {
        return Ok(false);
    }
    if local.store.get(id).is_none() {
        return Ok(false);
    }
    local.store.delete(id, now_ms)?;
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttReconcileResult {
    pub deleted_ids: Vec<String>,
    pub should_republish_all: bool,
}

/// Limpeza ao reconectar depois da janela de retidas (~3 s), com trava de segurança:
/// - não apaga nada se `retained_count == 0`, ou se sumiria mais da metade da lista E mais de 3 tarefas;
/// - nesse caso sinaliza `should_republish_all = true`.
pub(crate) fn reconcile_mqtt_retained_local(
    local: &mut LocalState,
    known_synced_ids: &HashSet<String>,
    seen_retained_ids: &HashSet<String>,
    retained_count: usize,
    now_ms: i64,
) -> Result<MqttReconcileResult, String> {
    let known_active: Vec<String> = local
        .store
        .active_items()
        .into_iter()
        .map(|item| item.id)
        .filter(|id| known_synced_ids.contains(id))
        .collect();

    if retained_count == 0 {
        return Ok(MqttReconcileResult {
            deleted_ids: Vec::new(),
            should_republish_all: !known_active.is_empty(),
        });
    }

    let missing: Vec<String> = known_active
        .iter()
        .filter(|id| !seen_retained_ids.contains(*id))
        .cloned()
        .collect();

    if missing.len() * 2 > known_active.len() && missing.len() > 3 {
        return Ok(MqttReconcileResult {
            deleted_ids: Vec::new(),
            should_republish_all: true,
        });
    }

    let mut deleted_ids = Vec::new();
    for id in missing {
        if delete_mqtt_task_local(local, &id, now_ms)? {
            deleted_ids.push(id);
        }
    }

    Ok(MqttReconcileResult {
        deleted_ids,
        should_republish_all: false,
    })
}

pub fn export_mqtt_tasks(app: &AppHandle) -> Vec<(String, Value)> {
    let Some(mgr) = get(app) else {
        return Vec::new();
    };
    let local = mgr.local.lock().unwrap();
    export_mqtt_tasks_local(&local)
}

pub fn apply_mqtt_task(
    app: &AppHandle,
    topic_entity_id: &str,
    envelope: &Value,
    now_ms: i64,
) -> Result<bool, String> {
    let Some(mgr) = get(app) else {
        return Ok(false);
    };
    let changed = {
        let mut local = mgr.local.lock().unwrap();
        apply_mqtt_task_local(&mut local, topic_entity_id, envelope, now_ms)?
    };
    if changed {
        let _ = mgr.wake_tx.send(());
        let _ = tauri::Emitter::emit(app, "tasks-changed", ());
        emit_task_sync_state(app, &mgr);
    }
    Ok(changed)
}

pub fn delete_mqtt_task(app: &AppHandle, entity_id: &str, now_ms: i64) -> Result<bool, String> {
    let Some(mgr) = get(app) else {
        return Ok(false);
    };
    let deleted = {
        let mut local = mgr.local.lock().unwrap();
        delete_mqtt_task_local(&mut local, entity_id, now_ms)?
    };
    if deleted {
        let _ = mgr.wake_tx.send(());
        let _ = tauri::Emitter::emit(app, "tasks-changed", ());
        emit_task_sync_state(app, &mgr);
    }
    Ok(deleted)
}

pub fn reconcile_mqtt_retained(
    app: &AppHandle,
    known_synced_ids: &HashSet<String>,
    seen_retained_ids: &HashSet<String>,
    retained_count: usize,
    now_ms: i64,
) -> Result<MqttReconcileResult, String> {
    let Some(mgr) = get(app) else {
        return Ok(MqttReconcileResult {
            deleted_ids: Vec::new(),
            should_republish_all: false,
        });
    };
    let res = {
        let mut local = mgr.local.lock().unwrap();
        reconcile_mqtt_retained_local(
            &mut local,
            known_synced_ids,
            seen_retained_ids,
            retained_count,
            now_ms,
        )?
    };
    if !res.deleted_ids.is_empty() {
        let _ = mgr.wake_tx.send(());
        let _ = tauri::Emitter::emit(app, "tasks-changed", ());
        emit_task_sync_state(app, &mgr);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::store::TaskStore;
    use super::super::sync::SyncStore;

    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_local() -> (TempDir, LocalState) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lumen-esp32-sync-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = TaskStore::load_at(dir.clone());
        let sync = SyncStore::load_at(dir.clone());
        (TempDir(dir), LocalState { store, sync })
    }

    #[test]
    fn new_esp32_task_preserves_entity_id_and_enqueues_once() {
        let (_dir, mut local) = temp_local();
        let env = json!({
            "contract_version": 1,
            "entity_id": "esp-task-abc-123",
            "updated_at": 1_760_000_000i64,
            "origin_role": "esp32",
            "payload": {
                "id": "esp-task-abc-123",
                "text": "Comprar filtro de café",
                "done": false,
                "due_ts": 1_760_003_600i64
            }
        });

        let (out, changed) = sync_esp32_local(&mut local, &[env.clone()], 1_760_000_000_000).unwrap();
        assert!(changed);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["entity_id"], "esp-task-abc-123");
        assert_eq!(local.store.get("esp-task-abc-123").unwrap().text, "Comprar filtro de café");
        assert!(local.store.get("esp-task-abc-123").unwrap().due_date.is_some());
        assert_eq!(local.sync.outbox().len(), 1);

        // Reenvio idêntico com `updated_at` mais novo (time(NULL) da placa): não duplica nem reenfileira
        let mut repeat = env;
        repeat["updated_at"] = json!(1_760_000_500i64);
        let (_, changed_again) = sync_esp32_local(&mut local, &[repeat], 1_760_000_500_000).unwrap();
        assert!(!changed_again);
        assert_eq!(local.store.active_items().len(), 1);
        assert_eq!(local.sync.outbox().len(), 1);
    }

    #[test]
    fn newer_done_toggle_updates_and_old_update_loses() {
        let (_dir, mut local) = temp_local();
        let env = json!({
            "entity_id": "esp-task-2",
            "updated_at": 1_760_000_100i64,
            "payload": { "id": "esp-task-2", "text": "Revisar PR", "done": false }
        });
        sync_esp32_local(&mut local, &[env], 1_760_000_100_000).unwrap();

        let toggle = json!({
            "entity_id": "esp-task-2",
            "updated_at": 1_760_000_200i64,
            "payload": { "id": "esp-task-2", "text": "Revisar PR", "done": true }
        });
        let (_, changed) = sync_esp32_local(&mut local, &[toggle], 1_760_000_200_000).unwrap();
        assert!(changed);
        assert!(local.store.get("esp-task-2").unwrap().done);

        let stale = json!({
            "entity_id": "esp-task-2",
            "updated_at": 1_760_000_050i64,
            "payload": { "id": "esp-task-2", "text": "Revisar PR", "done": false }
        });
        let (_, changed_stale) = sync_esp32_local(&mut local, &[stale], 1_760_000_300_000).unwrap();
        assert!(!changed_stale);
        assert!(local.store.get("esp-task-2").unwrap().done);
    }

    #[test]
    fn response_stays_under_3800_bytes_even_with_many_tasks() {
        let (_dir, mut local) = temp_local();
        let mut incoming = Vec::new();
        for i in 0..45 {
            incoming.push(json!({
                "entity_id": format!("esp-bulk-{i:03}"),
                "updated_at": 1_760_000_000i64 + i,
                "payload": {
                    "id": format!("esp-bulk-{i:03}"),
                    "text": format!("Tarefa longa numero {i} para testar o limite de 4KB do ESP32"),
                    "done": false
                }
            }));
        }
        let (out, _) = sync_esp32_local(&mut local, &incoming, 1_760_000_100_000).unwrap();
        assert!(out.len() <= ESP32_HTTP_MAX_TASKS);
        let wire = serde_json::to_vec(&json!({"ok":true,"envelopes":out,"count":out.len()})).unwrap();
        assert!(wire.len() <= ESP32_HTTP_MAX_BYTES);
    }

    #[test]
    fn mqtt_envelope_format_and_lww_conflict_rules() {
        let (_dir, mut local) = temp_local();
        let initial = json!({
            "contract_version": 1,
            "entity_type": "todo",
            "entity_id": "esp-lww-1",
            "origin_role": "esp32",
            "updated_at": 1_764_240_100i64,
            "payload": {
                "id": "esp-lww-1",
                "text": "Versao local",
                "done": false,
                "due_ts": 1_764_280_800i64
            }
        });
        assert!(apply_mqtt_task_local(&mut local, "esp-lww-1", &initial, 1_764_240_100_000).unwrap());

        let exported = export_mqtt_tasks_local(&local);
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].0, "esp-lww-1");
        assert_eq!(exported[0].1["contract_version"], 1);
        assert_eq!(exported[0].1["entity_type"], "todo");
        assert_eq!(exported[0].1["origin_role"], "lumen");
        assert_eq!(exported[0].1["updated_at"], 1_764_240_100i64);

        // Velho perde
        let older = json!({
            "contract_version": 1,
            "entity_type": "todo",
            "entity_id": "esp-lww-1",
            "origin_role": "esp32",
            "updated_at": 1_764_240_050i64,
            "payload": { "id": "esp-lww-1", "text": "Antiga", "done": true }
        });
        assert!(!apply_mqtt_task_local(&mut local, "esp-lww-1", &older, 1_764_240_200_000).unwrap());
        assert_eq!(local.store.get("esp-lww-1").unwrap().text, "Versao local");

        // Empate mantém o local
        let tie = json!({
            "contract_version": 1,
            "entity_type": "todo",
            "entity_id": "esp-lww-1",
            "origin_role": "esp32",
            "updated_at": 1_764_240_100i64,
            "payload": { "id": "esp-lww-1", "text": "Empate remoto", "done": true }
        });
        assert!(!apply_mqtt_task_local(&mut local, "esp-lww-1", &tie, 1_764_240_200_000).unwrap());
        assert_eq!(local.store.get("esp-lww-1").unwrap().text, "Versao local");

        // Mais novo vence
        let newer = json!({
            "contract_version": 1,
            "entity_type": "todo",
            "entity_id": "esp-lww-1",
            "origin_role": "esp32",
            "updated_at": 1_764_240_200i64,
            "payload": { "id": "esp-lww-1", "text": "Versao nova", "done": true }
        });
        assert!(apply_mqtt_task_local(&mut local, "esp-lww-1", &newer, 1_764_240_200_000).unwrap());
        let item = local.store.get("esp-lww-1").unwrap();
        assert_eq!(item.text, "Versao nova");
        assert!(item.done);
        // Decisão L10.3: tarefas recebidas pelo MQTT não entram na outbox L6.
        assert!(local.sync.outbox().is_empty());
    }

    #[test]
    fn mqtt_delete_and_reconcile_prune_with_safety_guards() {
        let (_dir, mut local) = temp_local();
        let mut known = HashSet::new();
        for i in 1..=6 {
            let id = format!("t-{i}");
            let env = json!({
                "contract_version": 1,
                "entity_type": "todo",
                "entity_id": id,
                "origin_role": "esp32",
                "updated_at": 1_764_240_000i64 + i,
                "payload": { "id": format!("t-{i}"), "text": format!("Tarefa {i}"), "done": false }
            });
            apply_mqtt_task_local(&mut local, &format!("t-{i}"), &env, 1_764_240_000_000).unwrap();
            known.insert(format!("t-{i}"));
        }

        // Trava 1: nenhuma retida chegou -> não apaga e pede republicação
        let empty_seen = HashSet::new();
        let r0 = reconcile_mqtt_retained_local(&mut local, &known, &empty_seen, 0, 1_764_240_500_000).unwrap();
        assert!(r0.deleted_ids.is_empty());
        assert!(r0.should_republish_all);
        assert_eq!(local.store.active_items().len(), 6);

        // Trava 2: sumiria 4 de 6 (> metade e > 3) -> não apaga e pede republicação
        let seen_two: HashSet<String> = ["t-1", "t-2"].into_iter().map(String::from).collect();
        let r_guard = reconcile_mqtt_retained_local(&mut local, &known, &seen_two, 2, 1_764_240_500_000).unwrap();
        assert!(r_guard.deleted_ids.is_empty());
        assert!(r_guard.should_republish_all);
        assert_eq!(local.store.active_items().len(), 6);

        // Sem trava: sumiram apenas 2 de 6 -> apaga t-5 e t-6
        let seen_four: HashSet<String> = ["t-1", "t-2", "t-3", "t-4"].into_iter().map(String::from).collect();
        let r_ok = reconcile_mqtt_retained_local(&mut local, &known, &seen_four, 4, 1_764_240_600_000).unwrap();
        assert!(!r_ok.should_republish_all);
        assert_eq!(r_ok.deleted_ids.len(), 2);
        assert!(local.store.get("t-5").is_none());
        assert!(local.store.get("t-6").is_none());
        assert_eq!(local.store.active_items().len(), 4);

        // Delete remoto direto (payload vazio)
        assert!(delete_mqtt_task_local(&mut local, "t-4", 1_764_240_700_000).unwrap());
        assert!(local.store.get("t-4").is_none());
        assert!(!delete_mqtt_task_local(&mut local, "t-4", 1_764_240_700_000).unwrap());
    }
}
