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
}
