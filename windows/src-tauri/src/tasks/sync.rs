//! Motor de sincronização de tarefas v1 (`Todo/sync.json`, caixa `coucou-mailbox/v1`).
//! Implementa outbox local-first com coalescência, pull/ack/push idempotente,
//! quarentena, contagem de mensagens venenosas sem logar segredos/conteúdo e
//! reconciliação simétrica de 3 vias com resolução manual de conflitos de texto livre.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::{TodoItem, TodoSubtask, TODO_TIME_ZONE};
use super::store::{write_atomic, TaskStore};
use crate::phone_link::relay_client::{
    self, Credentials, MailboxAckResponse, MailboxEnvelopeItem, MailboxHttpError,
    MailboxListResponse, MailboxPostOutcome, MailboxStatsResponse, Target,
};
use crate::phone_link::relay_crypto::{k_mailbox, open_mailbox, seal_mailbox, MailboxRole};
use crate::whatsapp::gateway::{iso_utc, rfc3339_ms};

pub const SYNC_SCHEMA_VERSION: u32 = 1;
pub const NINETY_DAYS_MS: i64 = 90 * 86_400 * 1000;
const PULL_PAGE_LIMIT: u32 = 100;

const KNOWN_TASK_KEYS: &[&str] = &[
    "id",
    "text",
    "due",
    "due_date",
    "due_time",
    "time_zone",
    "alert_minutes",
    "priority",
    "subtasks",
    "recurrence",
    "series_id",
    "done",
    "created_at",
    "done_at",
    "snoozed_until",
    "source",
    "source_stale",
    "version",
    "deleted_at",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSyncState {
    LocalOnly,
    SavedOnPc,
    WaitingPhone,
    Synced,
    Revoked,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskTextConflict {
    pub id: String,
    pub entity_id: String,
    pub field: String,
    pub local_text: String,
    pub remote_text: String,
    pub winner_side: String,
    pub winner_text: String,
    pub loser_text: String,
    pub local_updated_at: String,
    pub remote_updated_at: String,
    pub conflict_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSyncStatus {
    pub state: TaskSyncState,
    pub paired: bool,
    pub device_id: String,
    pub pending_outbox: usize,
    pub pending_peer: Option<u64>,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub quarantined_count: usize,
    pub poison_count: u32,
    pub revoked: bool,
    pub conflicts: Vec<TaskTextConflict>,
    pub unsynced_item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncEnvelope {
    pub contract_version: u32,
    pub mutation_id: String,
    pub device_id: String,
    pub origin_role: String,
    pub entity_type: String,
    pub entity_id: String,
    pub base_version: u32,
    pub new_version: u32,
    pub updated_at: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutboxEntry {
    pub envelope: SyncEnvelope,
    pub queued_at: String,
    #[serde(default)]
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppliedMutation {
    pub mutation_id: String,
    pub applied_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ItemSyncMeta {
    pub last_updated_at: String,
    pub last_device_id: String,
    #[serde(default)]
    pub unsynced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuarantinedEnvelope {
    pub seq: u64,
    pub received_at: String,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncFileState {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    device_id: String,
    #[serde(default)]
    paired_room: Option<String>,
    #[serde(default)]
    last_pc_seq: u64,
    #[serde(default)]
    outbox: Vec<OutboxEntry>,
    #[serde(default)]
    applied_mutations: Vec<AppliedMutation>,
    #[serde(default)]
    base_copies: BTreeMap<String, TodoItem>,
    #[serde(default)]
    extra_fields: BTreeMap<String, BTreeMap<String, Value>>,
    #[serde(default)]
    item_meta: BTreeMap<String, ItemSyncMeta>,
    #[serde(default)]
    conflicts: BTreeMap<String, TaskTextConflict>,
    #[serde(default)]
    quarantine: Vec<QuarantinedEnvelope>,
    #[serde(default)]
    poison_count: u32,
    #[serde(default)]
    pending_peer: Option<u64>,
    #[serde(default)]
    revoked: bool,
    #[serde(default)]
    last_synced_at: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
}

fn default_schema_version() -> u32 {
    SYNC_SCHEMA_VERSION
}

impl SyncFileState {
    fn new_with_device(device_id: String) -> Self {
        Self {
            schema_version: SYNC_SCHEMA_VERSION,
            device_id,
            paired_room: None,
            last_pc_seq: 0,
            outbox: Vec::new(),
            applied_mutations: Vec::new(),
            base_copies: BTreeMap::new(),
            extra_fields: BTreeMap::new(),
            item_meta: BTreeMap::new(),
            conflicts: BTreeMap::new(),
            quarantine: Vec::new(),
            poison_count: 0,
            pending_peer: None,
            revoked: false,
            last_synced_at: None,
            last_error: None,
        }
    }
}

pub struct SyncStore {
    dir: PathBuf,
    state: SyncFileState,
}

impl SyncStore {
    pub fn load_at(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("sync.json");
        let mut needs_persist = false;
        let mut state = match fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SyncFileState>(&bytes).ok())
        {
            Some(s) => s,
            None => {
                needs_persist = true;
                SyncFileState::new_with_device(new_uuid_v4())
            }
        };
        if state.schema_version == 0 {
            state.schema_version = SYNC_SCHEMA_VERSION;
            needs_persist = true;
        }
        if state.device_id.trim().is_empty() {
            state.device_id = new_uuid_v4();
            needs_persist = true;
        }
        let store = Self { dir, state };
        if needs_persist {
            let _ = store.persist();
        }
        store
    }

    #[cfg(test)]
    pub fn device_id(&self) -> &str {
        &self.state.device_id
    }

    pub fn last_pc_seq(&self) -> u64 {
        self.state.last_pc_seq
    }

    pub fn is_revoked(&self) -> bool {
        self.state.revoked
    }

    pub fn outbox(&self) -> &[OutboxEntry] {
        &self.state.outbox
    }

    pub fn conflict(&self, conflict_id: &str) -> Option<&TaskTextConflict> {
        self.state.conflicts.get(conflict_id)
    }

    #[cfg(test)]
    pub fn quarantined(&self) -> &[QuarantinedEnvelope] {
        &self.state.quarantine
    }

    pub fn status(&self, paired: bool) -> TaskSyncStatus {
        let mut unsynced: BTreeSet<String> = self
            .state
            .item_meta
            .iter()
            .filter_map(|(id, meta)| meta.unsynced.then_some(id.clone()))
            .collect();
        for entry in &self.state.outbox {
            unsynced.insert(entry.envelope.entity_id.clone());
        }
        let unsynced_item_ids: Vec<String> = unsynced.into_iter().collect();
        let revoked = paired && self.state.revoked;
        let last_error = if paired && !revoked {
            self.state.last_error.clone()
        } else {
            None
        };
        let state = if !paired {
            TaskSyncState::LocalOnly
        } else if revoked {
            TaskSyncState::Revoked
        } else if last_error.is_some() {
            TaskSyncState::Error
        } else if !self.state.outbox.is_empty() {
            TaskSyncState::SavedOnPc
        } else if self.state.pending_peer.is_some_and(|n| n > 0) {
            TaskSyncState::WaitingPhone
        } else if self.state.pending_peer == Some(0) && unsynced_item_ids.is_empty() {
            TaskSyncState::Synced
        } else {
            TaskSyncState::SavedOnPc
        };

        TaskSyncStatus {
            state,
            paired,
            device_id: self.state.device_id.clone(),
            pending_outbox: self.state.outbox.len(),
            pending_peer: if paired { self.state.pending_peer } else { None },
            last_synced_at: self.state.last_synced_at.clone(),
            last_error,
            quarantined_count: self.state.quarantine.len(),
            poison_count: self.state.poison_count,
            revoked,
            conflicts: self.state.conflicts.values().cloned().collect(),
            unsynced_item_ids,
        }
    }

    /// Detecta troca de sala/pareamento (armazenando apenas o digest SHA-256 de `room`, nunca a sala em claro)
    /// e enfileira o snapshot das tarefas locais atuais (ou tombstones < 90d) na outbox da nova sala.
    pub fn ensure_room(&mut self, room: &str, all_items: &[TodoItem], now_ms: i64) -> Result<bool, String> {
        let tag = room_tag(room);
        if self.state.paired_room.as_deref() == Some(tag.as_str()) {
            return Ok(false);
        }
        let had_previous = self.state.paired_room.is_some();
        self.state.paired_room = Some(tag);
        self.state.last_pc_seq = 0;
        self.state.revoked = false;
        self.state.pending_peer = None;
        self.state.last_error = None;
        if had_previous {
            self.state.outbox.clear();
            self.state.applied_mutations.clear();
            self.state.base_copies.clear();
            self.state.conflicts.clear();
            self.state.item_meta.clear();
        }
        for item in all_items {
            if !should_sync_in_snapshot(item, now_ms) {
                continue;
            }
            if !had_previous && self.state.outbox.iter().any(|e| e.envelope.entity_id == item.id) {
                if let Some(meta) = self.state.item_meta.get_mut(&item.id) {
                    meta.unsynced = true;
                }
                continue;
            }
            self.push_outbox_entry(item, 0, now_ms);
        }
        self.persist()?;
        Ok(true)
    }

    /// Enfileira mutação local na outbox (coalescendo por `entity_id` mantendo o primeiro `base_version`)
    /// e persiste `sync.json` antes de qualquer comunicação de rede.
    pub fn enqueue_local_mutation(
        &mut self,
        item: &TodoItem,
        base_version: u32,
        now_ms: i64,
    ) -> Result<(), String> {
        self.push_outbox_entry(item, base_version, now_ms);
        self.state.pending_peer = None;
        self.persist()
    }

    pub fn remove_conflict(&mut self, conflict_id: &str) -> Result<bool, String> {
        let removed = self.state.conflicts.remove(conflict_id).is_some();
        if removed {
            self.persist()?;
        }
        Ok(removed)
    }

    pub fn clear_conflicts_for_entity(&mut self, entity_id: &str) {
        self.state.conflicts.retain(|_, c| c.entity_id != entity_id);
    }

    fn push_outbox_entry(&mut self, item: &TodoItem, base_version: u32, now_ms: i64) {
        let updated_at = iso_utc(now_ms);
        let payload = serialize_task_payload(item, self.state.extra_fields.get(&item.id));
        let mutation_id = new_uuid_v4();

        if let Some(existing) = self
            .state
            .outbox
            .iter_mut()
            .find(|e| e.envelope.entity_id == item.id)
        {
            existing.envelope.mutation_id = mutation_id;
            existing.envelope.new_version = item.version;
            existing.envelope.updated_at = updated_at.clone();
            existing.envelope.payload = payload;
            existing.queued_at = updated_at.clone();
            existing.attempts = 0;
        } else {
            self.state.outbox.push(OutboxEntry {
                envelope: SyncEnvelope {
                    contract_version: 1,
                    mutation_id,
                    device_id: self.state.device_id.clone(),
                    origin_role: "pc".to_string(),
                    entity_type: "task".to_string(),
                    entity_id: item.id.clone(),
                    base_version,
                    new_version: item.version,
                    updated_at: updated_at.clone(),
                    payload,
                },
                queued_at: updated_at.clone(),
                attempts: 0,
            });
        }

        self.state.item_meta.insert(
            item.id.clone(),
            ItemSyncMeta {
                last_updated_at: updated_at,
                last_device_id: self.state.device_id.clone(),
                unsynced: true,
            },
        );
    }

    fn replace_outbox_with_merged(
        &mut self,
        merged: &TodoItem,
        merged_extras: &BTreeMap<String, Value>,
        base_version: u32,
        now_ms: i64,
    ) {
        self.state.outbox.retain(|e| e.envelope.entity_id != merged.id);
        self.set_extra_fields(&merged.id, merged_extras.clone());
        self.push_outbox_entry(merged, base_version, now_ms);
    }

    fn set_extra_fields(&mut self, id: &str, extras: BTreeMap<String, Value>) {
        if extras.is_empty() {
            self.state.extra_fields.remove(id);
        } else {
            self.state.extra_fields.insert(id.to_string(), extras);
        }
    }

    fn has_applied(&self, mutation_id: &str) -> bool {
        self.state
            .applied_mutations
            .iter()
            .any(|m| m.mutation_id == mutation_id)
    }

    fn record_applied(&mut self, mutation_id: String, now_ms: i64) {
        if !self.has_applied(&mutation_id) {
            self.state.applied_mutations.push(AppliedMutation {
                mutation_id,
                applied_at: iso_utc(now_ms),
            });
        }
    }

    fn prune_applied_mutations(&mut self, now_ms: i64) {
        self.state.applied_mutations.retain(|entry| {
            rfc3339_ms(Some(&entry.applied_at))
                .map(|ms| now_ms.saturating_sub(ms) <= NINETY_DAYS_MS)
                .unwrap_or(true)
        });
    }

    fn set_last_pc_seq(&mut self, seq: u64) -> Result<(), String> {
        if seq > self.state.last_pc_seq {
            self.state.last_pc_seq = seq;
            self.persist()?;
        }
        Ok(())
    }

    fn mark_outbox_sent(&mut self, mutation_id: &str, sent_item: TodoItem) -> Result<(), String> {
        if self
            .state
            .outbox
            .first()
            .is_some_and(|e| e.envelope.mutation_id == mutation_id)
        {
            self.state.outbox.remove(0);
            self.state
                .base_copies
                .insert(sent_item.id.clone(), sent_item);
        } else if let Some(entry) = self
            .state
            .outbox
            .iter_mut()
            .find(|e| e.envelope.entity_id == sent_item.id)
        {
            // Se o usuário editou a mesma tarefa enquanto o POST estava em voo, o próximo envio parte da versão já entregue.
            entry.envelope.base_version = sent_item.version;
            self.state
                .base_copies
                .insert(sent_item.id.clone(), sent_item);
        }
        self.persist()
    }

    fn increment_outbox_attempt(&mut self, mutation_id: &str) -> Result<(), String> {
        if let Some(entry) = self
            .state
            .outbox
            .iter_mut()
            .find(|e| e.envelope.mutation_id == mutation_id)
        {
            entry.attempts = entry.attempts.saturating_add(1);
            self.persist()?;
        }
        Ok(())
    }

    fn apply_stats(&mut self, stats: &MailboxStatsResponse, now_ms: i64) -> Result<(), String> {
        self.state.pending_peer = Some(stats.pending_phone);
        self.state.revoked = stats.revoked;
        self.state.last_error = None;
        if self.state.outbox.is_empty() && stats.pending_phone == 0 && !stats.revoked {
            for meta in self.state.item_meta.values_mut() {
                meta.unsynced = false;
            }
            self.state.last_synced_at = Some(iso_utc(now_ms));
        }
        self.persist()
    }

    fn set_error(&mut self, msg: String) -> Result<(), String> {
        self.state.last_error = Some(msg);
        self.persist()
    }

    fn set_revoked(&mut self) -> Result<(), String> {
        self.state.revoked = true;
        self.state.last_error = None;
        self.persist()
    }

    pub fn persist(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&self.state).map_err(|e| e.to_string())?;
        write_atomic(&self.dir.join("sync.json"), &bytes)
    }
}

pub trait MailboxTransport: Send + Sync {
    fn post(
        &self,
        target: &Target,
        creds: &Credentials,
        to: MailboxRole,
        msg_id: &str,
        body: &[u8],
    ) -> Result<MailboxPostOutcome, MailboxHttpError>;

    fn list(
        &self,
        target: &Target,
        creds: &Credentials,
        for_role: MailboxRole,
        after_seq: u64,
        limit: u32,
    ) -> Result<MailboxListResponse, MailboxHttpError>;

    fn ack(
        &self,
        target: &Target,
        creds: &Credentials,
        for_role: MailboxRole,
        upto_seq: u64,
    ) -> Result<MailboxAckResponse, MailboxHttpError>;

    fn stats(
        &self,
        target: &Target,
        creds: &Credentials,
    ) -> Result<MailboxStatsResponse, MailboxHttpError>;
}

#[derive(Default, Clone, Copy)]
pub struct RelayMailboxTransport;

impl MailboxTransport for RelayMailboxTransport {
    fn post(
        &self,
        target: &Target,
        creds: &Credentials,
        to: MailboxRole,
        msg_id: &str,
        body: &[u8],
    ) -> Result<MailboxPostOutcome, MailboxHttpError> {
        relay_client::mailbox_post(target, creds, to, msg_id, body)
    }

    fn list(
        &self,
        target: &Target,
        creds: &Credentials,
        for_role: MailboxRole,
        after_seq: u64,
        limit: u32,
    ) -> Result<MailboxListResponse, MailboxHttpError> {
        relay_client::mailbox_list(target, creds, for_role, after_seq, limit)
    }

    fn ack(
        &self,
        target: &Target,
        creds: &Credentials,
        for_role: MailboxRole,
        upto_seq: u64,
    ) -> Result<MailboxAckResponse, MailboxHttpError> {
        relay_client::mailbox_ack(target, creds, for_role, upto_seq)
    }

    fn stats(
        &self,
        target: &Target,
        creds: &Credentials,
    ) -> Result<MailboxStatsResponse, MailboxHttpError> {
        relay_client::mailbox_stats(target, creds)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPauseReason {
    Revoked,
    Fatal,
    Backoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCycleOutcome {
    pub changed_tasks: bool,
    pub dismiss_ids: Vec<String>,
    pub pause: Option<SyncPauseReason>,
    pub status_changed: bool,
}

enum PulledItem {
    Poison {
        seq: u64,
    },
    Quarantine {
        seq: u64,
        mutation_id: Option<String>,
        raw: Value,
    },
    Task {
        seq: u64,
        envelope: SyncEnvelope,
        item: TodoItem,
        extra_fields: BTreeMap<String, Value>,
    },
}

/// Executa um ciclo completo (`pull` → aplicação atômica → `ack` contíguo → `push` FIFO → `stats`),
/// sem nunca manter `local_lock` travado durante chamadas de rede.
pub fn run_sync_cycle<F, R>(
    target: &Target,
    creds: &Credentials,
    transport: &dyn MailboxTransport,
    now_ms: i64,
    mut with_local: F,
) -> Result<SyncCycleOutcome, String>
where
    F: FnMut(&mut dyn FnMut(&mut TaskStore, &mut SyncStore) -> Result<(), String>) -> Result<R, String>
        + Sync
        + Send,
{
    let mut after_seq = 0u64;
    let mut already_revoked = false;
    let mut status_changed = false;

    with_local(&mut |store, sync| {
        status_changed = sync.ensure_room(&creds.room, store.all_items(), now_ms)?;
        after_seq = sync.last_pc_seq();
        already_revoked = sync.is_revoked();
        Ok(())
    })?;

    if already_revoked {
        return Ok(SyncCycleOutcome {
            changed_tasks: false,
            dismiss_ids: Vec::new(),
            pause: Some(SyncPauseReason::Revoked),
            status_changed,
        });
    }

    let km = k_mailbox(&creds.key);
    let mut any_changed_tasks = false;
    let mut all_dismiss_ids = Vec::new();

    // 1) Pull + aplicação atômica + ack contíguo
    loop {
        let list = match transport.list(target, creds, MailboxRole::Pc, after_seq, PULL_PAGE_LIMIT) {
            Ok(resp) => resp,
            Err(err) => {
                let mut res = record_http_error(err, &mut with_local)?;
                res.changed_tasks |= any_changed_tasks;
                res.dismiss_ids.extend(all_dismiss_ids);
                return Ok(res);
            }
        };
        if list.messages.is_empty() {
            break;
        }
        let mut sorted = list.messages;
        sorted.sort_by_key(|m| m.seq);
        let count_in_page = sorted.len() as u64;
        let pulled = decrypt_and_parse_batch(&km, &creds.room, sorted);

        let mut max_seq = after_seq;
        let mut batch_changed = false;
        let mut batch_dismiss = Vec::new();

        with_local(&mut |store, sync| {
            let outcome = apply_pulled_batch(store, sync, &pulled, now_ms)?;
            max_seq = outcome.max_seq.max(after_seq);
            batch_changed = outcome.changed_tasks;
            batch_dismiss = outcome.dismiss_ids;
            Ok(())
        })?;

        if batch_changed {
            any_changed_tasks = true;
        }
        all_dismiss_ids.extend(batch_dismiss);

        if max_seq > after_seq {
            match transport.ack(target, creds, MailboxRole::Pc, max_seq) {
                Ok(_) => {
                    with_local(&mut |_, sync| sync.set_last_pc_seq(max_seq))?;
                    after_seq = max_seq;
                }
                Err(err) => {
                    let mut res = record_http_error(err, &mut with_local)?;
                    res.changed_tasks |= any_changed_tasks;
                    res.dismiss_ids.extend(all_dismiss_ids);
                    return Ok(res);
                }
            }
        } else {
            break;
        }

        if count_in_page < u64::from(PULL_PAGE_LIMIT) && list.pending_count <= count_in_page {
            break;
        }
    }

    // 2) Push FIFO da outbox
    loop {
        let mut front: Option<OutboxEntry> = None;
        with_local(&mut |_, sync| {
            if !sync.is_revoked() {
                front = sync.outbox().first().cloned();
            }
            Ok(())
        })?;
        let Some(entry) = front else { break };

        let json = serde_json::to_string(&entry.envelope).map_err(|e| e.to_string())?;
        let sealed = match seal_mailbox(&km, &creds.room, MailboxRole::Phone, &json) {
            Ok(bytes) => bytes,
            Err(_) => {
                with_local(&mut |_, sync| {
                    sync.increment_outbox_attempt(&entry.envelope.mutation_id)?;
                    sync.set_error("Uma tarefa excedeu o limite de 64 KiB para sincronização.".into())
                })?;
                return Ok(SyncCycleOutcome {
                    changed_tasks: any_changed_tasks,
                    dismiss_ids: all_dismiss_ids,
                    pause: Some(SyncPauseReason::Fatal),
                    status_changed: true,
                });
            }
        };

        match transport.post(
            target,
            creds,
            MailboxRole::Phone,
            &entry.envelope.mutation_id,
            &sealed,
        ) {
            Ok(_) => {
                let sent_item: TodoItem = serde_json::from_value(entry.envelope.payload.clone())
                    .map_err(|e| e.to_string())?;
                with_local(&mut |_, sync| {
                    sync.mark_outbox_sent(&entry.envelope.mutation_id, sent_item.clone())
                })?;
            }
            Err(err) => {
                with_local(&mut |_, sync| {
                    sync.increment_outbox_attempt(&entry.envelope.mutation_id)
                })?;
                let mut res = record_http_error(err, &mut with_local)?;
                res.changed_tasks |= any_changed_tasks;
                res.dismiss_ids.extend(all_dismiss_ids);
                return Ok(res);
            }
        }
    }

    // 3) Stats para decidir "Sincronizado" estrito
    match transport.stats(target, creds) {
        Ok(stats) => {
            with_local(&mut |_, sync| sync.apply_stats(&stats, now_ms))?;
            Ok(SyncCycleOutcome {
                changed_tasks: any_changed_tasks,
                dismiss_ids: all_dismiss_ids,
                pause: if stats.revoked {
                    Some(SyncPauseReason::Revoked)
                } else {
                    None
                },
                status_changed: true,
            })
        }
        Err(err) => {
            let mut res = record_http_error(err, &mut with_local)?;
            res.changed_tasks |= any_changed_tasks;
            res.dismiss_ids.extend(all_dismiss_ids);
            Ok(res)
        }
    }
}

fn record_http_error<F, R>(err: MailboxHttpError, with_local: &mut F) -> Result<SyncCycleOutcome, String>
where
    F: FnMut(&mut dyn FnMut(&mut TaskStore, &mut SyncStore) -> Result<(), String>) -> Result<R, String>,
{
    let pause = match &err {
        MailboxHttpError::Revoked => SyncPauseReason::Revoked,
        MailboxHttpError::Unauthorized
        | MailboxHttpError::Forbidden
        | MailboxHttpError::PayloadTooLarge => SyncPauseReason::Fatal,
        MailboxHttpError::RateLimited
        | MailboxHttpError::MailboxFull
        | MailboxHttpError::BadRequest(_)
        | MailboxHttpError::Transport(_) => SyncPauseReason::Backoff,
    };
    with_local(&mut |_, sync| {
        if pause == SyncPauseReason::Revoked {
            sync.set_revoked()
        } else {
            sync.set_error(safe_http_error(&err))
        }
    })?;
    Ok(SyncCycleOutcome {
        changed_tasks: false,
        dismiss_ids: Vec::new(),
        pause: Some(pause),
        status_changed: true,
    })
}

pub fn safe_http_error(err: &MailboxHttpError) -> String {
    match err {
        MailboxHttpError::Unauthorized => "Credencial de acesso ao relay recusada (401).".into(),
        MailboxHttpError::Forbidden => "Chave da sala incompatível com o relay (403).".into(),
        MailboxHttpError::Revoked => "Pareamento revogado na caixa de correio (410).".into(),
        MailboxHttpError::PayloadTooLarge => {
            "Uma tarefa excedeu o limite de 64 KiB da caixa de correio (413).".into()
        }
        MailboxHttpError::MailboxFull => {
            "Caixa de correio cheia no relay (507). Aguarde o celular sincronizar.".into()
        }
        MailboxHttpError::RateLimited => {
            "Muitas requisições ao relay (429). Tentaremos novamente em instantes.".into()
        }
        MailboxHttpError::BadRequest(_) => "Requisição recusada pelo relay (400).".into(),
        MailboxHttpError::Transport(detail) => {
            if detail.contains("timeout") {
                "Tempo esgotado ao conectar ao relay.".into()
            } else if detail.contains("unreachable") {
                "Relay inacessível no momento.".into()
            } else {
                "Erro de comunicação com o relay.".into()
            }
        }
    }
}

fn decrypt_and_parse_batch(
    km: &crate::phone_link::relay_crypto::Secret32,
    room: &str,
    items: Vec<MailboxEnvelopeItem>,
) -> Vec<PulledItem> {
    let mut out = Vec::with_capacity(items.len());
    for msg in items {
        let Ok(json_text) = open_mailbox(km, room, MailboxRole::Pc, &msg.payload) else {
            out.push(PulledItem::Poison { seq: msg.seq });
            continue;
        };
        out.push(parse_pulled_json(msg.seq, &json_text));
    }
    out
}

fn parse_pulled_json(seq: u64, json_text: &str) -> PulledItem {
    let Ok(val) = serde_json::from_str::<Value>(json_text) else {
        return PulledItem::Poison { seq };
    };
    let Some(obj) = val.as_object() else {
        return PulledItem::Poison { seq };
    };
    let Some(cv) = obj.get("contract_version").and_then(|v| v.as_u64()) else {
        return PulledItem::Poison { seq };
    };
    if cv == 0 {
        return PulledItem::Poison { seq };
    }
    let mutation_id = obj
        .get("mutation_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if cv > 1 {
        return PulledItem::Quarantine {
            seq,
            mutation_id,
            raw: val,
        };
    }
    let Some(entity_type) = obj.get("entity_type").and_then(|v| v.as_str()) else {
        return PulledItem::Poison { seq };
    };
    if entity_type != "task" {
        return PulledItem::Quarantine {
            seq,
            mutation_id,
            raw: val,
        };
    }
    let Ok(envelope) = serde_json::from_value::<SyncEnvelope>(val) else {
        return PulledItem::Poison { seq };
    };
    if envelope.mutation_id.trim().is_empty()
        || envelope.device_id.trim().is_empty()
        || envelope.entity_id.trim().is_empty()
        || envelope.new_version == 0
    {
        return PulledItem::Poison { seq };
    }
    let Some(payload_obj) = envelope.payload.as_object() else {
        return PulledItem::Poison { seq };
    };
    let Ok(mut item) = serde_json::from_value::<TodoItem>(envelope.payload.clone()) else {
        return PulledItem::Poison { seq };
    };
    if item.id != envelope.entity_id || item.version != envelope.new_version {
        return PulledItem::Poison { seq };
    }
    if item.time_zone.trim().is_empty() {
        item.time_zone = TODO_TIME_ZONE.to_string();
    }
    let mut extra_fields = BTreeMap::new();
    for (k, v) in payload_obj {
        if !KNOWN_TASK_KEYS.contains(&k.as_str()) {
            extra_fields.insert(k.clone(), v.clone());
        }
    }
    PulledItem::Task {
        seq,
        envelope,
        item,
        extra_fields,
    }
}

struct ApplyBatchResult {
    max_seq: u64,
    changed_tasks: bool,
    dismiss_ids: Vec<String>,
}

fn apply_pulled_batch(
    store: &mut TaskStore,
    sync: &mut SyncStore,
    pulled: &[PulledItem],
    now_ms: i64,
) -> Result<ApplyBatchResult, String> {
    let mut max_seq = sync.state.last_pc_seq;
    let mut changed_tasks = false;
    let mut dismiss_ids = Vec::new();
    let mut dirty_store = false;

    for entry in pulled {
        match entry {
            PulledItem::Poison { seq } => {
                sync.state.poison_count = sync.state.poison_count.saturating_add(1);
                max_seq = max_seq.max(*seq);
            }
            PulledItem::Quarantine {
                seq,
                mutation_id,
                raw,
            } => {
                let already = mutation_id
                    .as_deref()
                    .is_some_and(|mid| sync.has_applied(mid));
                if !already {
                    sync.state.quarantine.push(QuarantinedEnvelope {
                        seq: *seq,
                        received_at: iso_utc(now_ms),
                        raw: raw.clone(),
                    });
                    if let Some(mid) = mutation_id {
                        sync.record_applied(mid.clone(), now_ms);
                    }
                }
                max_seq = max_seq.max(*seq);
            }
            PulledItem::Task {
                seq,
                envelope,
                item: remote_item,
                extra_fields: remote_extras,
            } => {
                if sync.has_applied(&envelope.mutation_id)
                    || envelope.device_id == sync.state.device_id
                {
                    max_seq = max_seq.max(*seq);
                    continue;
                }
                let id = &envelope.entity_id;
                let local_opt = store.find_any(id).cloned();
                let in_outbox = sync
                    .state
                    .outbox
                    .iter()
                    .any(|e| e.envelope.entity_id == *id);
                let is_unsynced = sync
                    .state
                    .item_meta
                    .get(id)
                    .is_some_and(|m| m.unsynced);
                let base_matches_local = sync
                    .state
                    .base_copies
                    .get(id)
                    .is_some_and(|b| local_opt.as_ref().is_some_and(|l| b.version == l.version));

                match local_opt {
                    None => {
                        store.upsert_remote(remote_item.clone())?;
                        sync.state
                            .base_copies
                            .insert(id.clone(), remote_item.clone());
                        sync.set_extra_fields(id, remote_extras.clone());
                        sync.state.item_meta.insert(
                            id.clone(),
                            ItemSyncMeta {
                                last_updated_at: envelope.updated_at.clone(),
                                last_device_id: envelope.device_id.clone(),
                                unsynced: false,
                            },
                        );
                        sync.clear_conflicts_for_entity(id);
                        if remote_item.done
                            || remote_item.is_tombstoned()
                            || remote_item.snoozed_until.is_some()
                        {
                            dismiss_ids.push(id.clone());
                        }
                        changed_tasks = true;
                        dirty_store = true;
                    }
                    Some(local)
                        if envelope.base_version == local.version
                            && !in_outbox
                            && (!is_unsynced || base_matches_local) =>
                    {
                        let became_done = remote_item.done && !local.done;
                        let became_deleted = remote_item.is_tombstoned() && !local.is_tombstoned();
                        let snooze_changed = remote_item.snoozed_until.is_some()
                            && remote_item.snoozed_until != local.snoozed_until;

                        store.upsert_remote(remote_item.clone())?;
                        sync.state
                            .base_copies
                            .insert(id.clone(), remote_item.clone());
                        sync.set_extra_fields(id, remote_extras.clone());
                        sync.state.item_meta.insert(
                            id.clone(),
                            ItemSyncMeta {
                                last_updated_at: envelope.updated_at.clone(),
                                last_device_id: envelope.device_id.clone(),
                                unsynced: false,
                            },
                        );
                        sync.clear_conflicts_for_entity(id);
                        if became_done || became_deleted || snooze_changed {
                            dismiss_ids.push(id.clone());
                        }
                        changed_tasks = true;
                        dirty_store = true;
                    }
                    Some(local) => {
                        let base = sync.state.base_copies.get(id);
                        let local_extras = sync.state.extra_fields.get(id);
                        let (local_ts, local_dev) = resolve_local_tiebreak(sync, &local);
                        let merge = reconcile_three_way(
                            base,
                            &local,
                            local_extras,
                            &local_ts,
                            &local_dev,
                            remote_item,
                            remote_extras,
                            &envelope.updated_at,
                            &envelope.device_id,
                            envelope.new_version,
                        );
                        let became_done = merge.item.done && !local.done;
                        let became_deleted = merge.item.is_tombstoned() && !local.is_tombstoned();
                        let snooze_changed = merge.item.snoozed_until.is_some()
                            && merge.item.snoozed_until != local.snoozed_until;

                        store.upsert_remote(merge.item.clone())?;
                        for c in merge.conflicts {
                            sync.state.conflicts.insert(c.id.clone(), c);
                        }
                        if became_done || became_deleted || snooze_changed {
                            dismiss_ids.push(id.clone());
                        }
                        let diverges_from_remote = !merge.item.same_content_ignoring_version(remote_item)
                            || merge.extra_fields != *remote_extras;
                        if diverges_from_remote {
                            // Mantém `remote_item` como base comum até a outbox recém-enfileirada ser enviada com sucesso.
                            sync.state
                                .base_copies
                                .insert(id.clone(), remote_item.clone());
                            sync.replace_outbox_with_merged(
                                &merge.item,
                                &merge.extra_fields,
                                envelope.new_version,
                                now_ms,
                            );
                        } else {
                            sync.state.outbox.retain(|e| e.envelope.entity_id != *id);
                            sync.state
                                .base_copies
                                .insert(id.clone(), merge.item.clone());
                            sync.set_extra_fields(id, merge.extra_fields);
                            sync.state.item_meta.insert(
                                id.clone(),
                                ItemSyncMeta {
                                    last_updated_at: envelope.updated_at.clone(),
                                    last_device_id: envelope.device_id.clone(),
                                    unsynced: false,
                                },
                            );
                        }
                        changed_tasks = true;
                        dirty_store = true;
                    }
                }
                sync.record_applied(envelope.mutation_id.clone(), now_ms);
                max_seq = max_seq.max(*seq);
            }
        }
    }

    let _ = dirty_store;
    sync.prune_applied_mutations(now_ms);
    sync.persist()?;
    Ok(ApplyBatchResult {
        max_seq,
        changed_tasks,
        dismiss_ids,
    })
}

fn resolve_local_tiebreak(sync: &SyncStore, local: &TodoItem) -> (String, String) {
    if let Some(entry) = sync
        .state
        .outbox
        .iter()
        .find(|e| e.envelope.entity_id == local.id)
    {
        return (
            entry.envelope.updated_at.clone(),
            entry.envelope.device_id.clone(),
        );
    }
    if let Some(meta) = sync.state.item_meta.get(&local.id) {
        if !meta.last_updated_at.is_empty() {
            return (meta.last_updated_at.clone(), meta.last_device_id.clone());
        }
    }
    let fallback_ts = local
        .deleted_at
        .clone()
        .or_else(|| local.done_at.clone())
        .unwrap_or_else(|| local.created_at.clone());
    (fallback_ts, sync.state.device_id.clone())
}

pub struct ReconcileResult {
    pub item: TodoItem,
    pub extra_fields: BTreeMap<String, Value>,
    pub conflicts: Vec<TaskTextConflict>,
}

#[allow(clippy::too_many_arguments)]
pub fn reconcile_three_way(
    base: Option<&TodoItem>,
    local: &TodoItem,
    local_extras: Option<&BTreeMap<String, Value>>,
    local_ts: &str,
    local_device_id: &str,
    remote: &TodoItem,
    remote_extras: &BTreeMap<String, Value>,
    remote_ts: &str,
    remote_device_id: &str,
    remote_new_version: u32,
) -> ReconcileResult {
    let remote_wins = remote_wins_lww(local_ts, local_device_id, remote_ts, remote_device_id);
    let mut conflicts = Vec::new();

    let (text, text_conflict) = merge_free_text(
        &local.id,
        "text",
        base.map(|b| b.text.as_str()),
        &local.text,
        &remote.text,
        local_ts,
        remote_ts,
        remote_wins,
    );
    if let Some(c) = text_conflict {
        conflicts.push(c);
    }

    let (done, done_at) = merge_done(base, local, remote);
    let deleted_at = merge_deleted_at(
        base.map(|b| &b.deleted_at),
        &local.deleted_at,
        &remote.deleted_at,
        remote_wins,
    );
    let snoozed_until = merge_snoozed_until(
        base.map(|b| &b.snoozed_until),
        &local.snoozed_until,
        &remote.snoozed_until,
        remote_wins,
    );
    let subtasks = merge_subtasks(
        &local.id,
        base.map(|b| b.subtasks.as_slice()),
        &local.subtasks,
        &remote.subtasks,
        local_ts,
        remote_ts,
        remote_wins,
        &mut conflicts,
    );

    let merged_item = TodoItem {
        id: local.id.clone(),
        text,
        due: merge_field_lww(base.map(|b| &b.due), &local.due, &remote.due, remote_wins),
        due_date: merge_field_lww(
            base.map(|b| &b.due_date),
            &local.due_date,
            &remote.due_date,
            remote_wins,
        ),
        due_time: merge_field_lww(
            base.map(|b| &b.due_time),
            &local.due_time,
            &remote.due_time,
            remote_wins,
        ),
        time_zone: merge_field_lww(
            base.map(|b| &b.time_zone),
            &local.time_zone,
            &remote.time_zone,
            remote_wins,
        ),
        alert_minutes: merge_field_lww(
            base.map(|b| &b.alert_minutes),
            &local.alert_minutes,
            &remote.alert_minutes,
            remote_wins,
        ),
        priority: merge_field_lww(
            base.map(|b| &b.priority),
            &local.priority,
            &remote.priority,
            remote_wins,
        ),
        subtasks,
        recurrence: merge_field_lww(
            base.map(|b| &b.recurrence),
            &local.recurrence,
            &remote.recurrence,
            remote_wins,
        ),
        series_id: merge_field_lww(
            base.map(|b| &b.series_id),
            &local.series_id,
            &remote.series_id,
            remote_wins,
        ),
        done,
        created_at: merge_field_lww(
            base.map(|b| &b.created_at),
            &local.created_at,
            &remote.created_at,
            remote_wins,
        ),
        done_at,
        snoozed_until,
        source: merge_field_lww(
            base.map(|b| &b.source),
            &local.source,
            &remote.source,
            remote_wins,
        ),
        source_stale: merge_field_lww(
            base.map(|b| &b.source_stale),
            &local.source_stale,
            &remote.source_stale,
            remote_wins,
        ),
        version: local.version.max(remote_new_version).saturating_add(1),
        deleted_at,
    };

    let mut merged_extras = BTreeMap::new();
    let empty_extras = BTreeMap::new();
    let l_extras = local_extras.unwrap_or(&empty_extras);
    for (k, v) in l_extras {
        if !remote_extras.contains_key(k) {
            merged_extras.insert(k.clone(), v.clone());
        }
    }
    for (k, r_val) in remote_extras {
        if let Some(l_val) = l_extras.get(k) {
            if l_val == r_val || remote_wins {
                merged_extras.insert(k.clone(), r_val.clone());
            } else {
                merged_extras.insert(k.clone(), l_val.clone());
            }
        } else {
            merged_extras.insert(k.clone(), r_val.clone());
        }
    }

    ReconcileResult {
        item: merged_item,
        extra_fields: merged_extras,
        conflicts,
    }
}

fn remote_wins_lww(local_ts: &str, local_dev: &str, remote_ts: &str, remote_dev: &str) -> bool {
    match (rfc3339_ms(Some(remote_ts)), rfc3339_ms(Some(local_ts))) {
        (Some(r), Some(l)) if r != l => r > l,
        (Some(_), Some(_)) => remote_dev > local_dev,
        _ => match remote_ts.cmp(local_ts) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => remote_dev > local_dev,
        },
    }
}

fn merge_field_lww<T: Clone + PartialEq>(
    base: Option<&T>,
    local: &T,
    remote: &T,
    remote_wins: bool,
) -> T {
    match base {
        Some(b) => {
            let l_changed = local != b;
            let r_changed = remote != b;
            match (l_changed, r_changed) {
                (false, true) => remote.clone(),
                (true, false) | (false, false) => local.clone(),
                (true, true) => {
                    if local == remote || !remote_wins {
                        local.clone()
                    } else {
                        remote.clone()
                    }
                }
            }
        }
        None => {
            if local == remote || !remote_wins {
                local.clone()
            } else {
                remote.clone()
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_free_text(
    entity_id: &str,
    field: &str,
    base: Option<&str>,
    local: &str,
    remote: &str,
    local_ts: &str,
    remote_ts: &str,
    remote_wins: bool,
) -> (String, Option<TaskTextConflict>) {
    let both_diverged = match base {
        Some(b) => local != b && remote != b && local != remote,
        None => local != remote,
    };
    if both_diverged {
        let (winner_side, winner_text, loser_text) = if remote_wins {
            ("remote", remote.to_string(), local.to_string())
        } else {
            ("local", local.to_string(), remote.to_string())
        };
        let conflict = TaskTextConflict {
            id: format!("{entity_id}:{field}"),
            entity_id: entity_id.to_string(),
            field: field.to_string(),
            local_text: local.to_string(),
            remote_text: remote.to_string(),
            winner_side: winner_side.to_string(),
            winner_text: winner_text.clone(),
            loser_text,
            local_updated_at: local_ts.to_string(),
            remote_updated_at: remote_ts.to_string(),
            conflict_state: "pending_review".to_string(),
        };
        return (winner_text, Some(conflict));
    }
    let chosen = match base {
        Some(b) => {
            if local == b && remote != b {
                remote.to_string()
            } else {
                local.to_string()
            }
        }
        None => local.to_string(),
    };
    (chosen, None)
}

fn earlier_opt_iso(a: Option<&String>, b: Option<&String>) -> Option<String> {
    match (a, b) {
        (Some(sa), Some(sb)) => match (rfc3339_ms(Some(sa)), rfc3339_ms(Some(sb))) {
            (Some(ma), Some(mb)) if ma != mb => {
                if ma <= mb { Some(sa.clone()) } else { Some(sb.clone()) }
            }
            _ => if sa <= sb { Some(sa.clone()) } else { Some(sb.clone()) },
        },
        (Some(sa), None) => Some(sa.clone()),
        (None, Some(sb)) => Some(sb.clone()),
        (None, None) => None,
    }
}

fn later_opt_iso(a: Option<&String>, b: Option<&String>, remote_wins: bool) -> Option<String> {
    match (a, b) {
        (Some(sa), Some(sb)) => match (rfc3339_ms(Some(sa)), rfc3339_ms(Some(sb))) {
            (Some(ma), Some(mb)) if ma != mb => {
                if ma > mb { Some(sa.clone()) } else { Some(sb.clone()) }
            }
            _ => if remote_wins { Some(sb.clone()) } else { Some(sa.clone()) },
        },
        (Some(sa), None) => Some(sa.clone()),
        (None, Some(sb)) => Some(sb.clone()),
        (None, None) => None,
    }
}

fn merge_done(base: Option<&TodoItem>, local: &TodoItem, remote: &TodoItem) -> (bool, Option<String>) {
    if let Some(b) = base {
        let l_changed = local.done != b.done || local.done_at != b.done_at;
        let r_changed = remote.done != b.done || remote.done_at != b.done_at;
        match (l_changed, r_changed) {
            (false, true) => return (remote.done, remote.done_at.clone()),
            (true, false) | (false, false) => return (local.done, local.done_at.clone()),
            (true, true) => {}
        }
    }
    match (local.done, remote.done) {
        (true, true) => (
            true,
            earlier_opt_iso(local.done_at.as_ref(), remote.done_at.as_ref()),
        ),
        (true, false) => (true, local.done_at.clone()),
        (false, true) => (true, remote.done_at.clone()),
        (false, false) => (false, None),
    }
}

fn merge_deleted_at(
    base: Option<&Option<String>>,
    local: &Option<String>,
    remote: &Option<String>,
    remote_wins: bool,
) -> Option<String> {
    if let Some(b) = base {
        if b.is_some() {
            if local == b && remote.is_none() {
                return None;
            }
            if remote == b && local.is_none() {
                return None;
            }
        }
    }
    match (local, remote) {
        (None, None) => None,
        (Some(l), None) => Some(l.clone()),
        (None, Some(r)) => Some(r.clone()),
        (Some(l), Some(r)) => {
            if l == r || !remote_wins {
                Some(l.clone())
            } else {
                Some(r.clone())
            }
        }
    }
}

fn merge_snoozed_until(
    base: Option<&Option<String>>,
    local: &Option<String>,
    remote: &Option<String>,
    remote_wins: bool,
) -> Option<String> {
    if let Some(b) = base {
        let l_changed = local != b;
        let r_changed = remote != b;
        match (l_changed, r_changed) {
            (false, true) => return remote.clone(),
            (true, false) | (false, false) => return local.clone(),
            (true, true) => {}
        }
    }
    later_opt_iso(local.as_ref(), remote.as_ref(), remote_wins)
}

#[allow(clippy::too_many_arguments)]
fn merge_subtasks(
    entity_id: &str,
    base: Option<&[TodoSubtask]>,
    local: &[TodoSubtask],
    remote: &[TodoSubtask],
    local_ts: &str,
    remote_ts: &str,
    remote_wins: bool,
    conflicts: &mut Vec<TaskTextConflict>,
) -> Vec<TodoSubtask> {
    let base_map: BTreeMap<&str, &TodoSubtask> = base
        .unwrap_or(&[])
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();
    let local_map: BTreeMap<&str, &TodoSubtask> =
        local.iter().map(|s| (s.id.as_str(), s)).collect();
    let remote_map: BTreeMap<&str, &TodoSubtask> =
        remote.iter().map(|s| (s.id.as_str(), s)).collect();

    let ordered_ids = collect_subtask_order(base, local, remote, remote_wins);
    let mut out = Vec::new();

    for sub_id in ordered_ids {
        let b_sub = base_map.get(sub_id.as_str()).copied();
        let l_sub = local_map.get(sub_id.as_str()).copied();
        let r_sub = remote_map.get(sub_id.as_str()).copied();

        match (b_sub, l_sub, r_sub) {
            (_, Some(l), Some(r)) => {
                let done = match b_sub {
                    Some(b) => {
                        if l.done == b.done && r.done != b.done {
                            r.done
                        } else if r.done == b.done && l.done != b.done {
                            l.done
                        } else {
                            l.done || r.done
                        }
                    }
                    None => l.done || r.done,
                };
                let field = format!("subtasks.{sub_id}.text");
                let (text, conflict) = merge_free_text(
                    entity_id,
                    &field,
                    b_sub.map(|b| b.text.as_str()),
                    &l.text,
                    &r.text,
                    local_ts,
                    remote_ts,
                    remote_wins,
                );
                if let Some(c) = conflict {
                    conflicts.push(c);
                }
                out.push(TodoSubtask {
                    id: sub_id,
                    text,
                    done,
                });
            }
            (Some(b), Some(l), None) => {
                if l != b && !remote_wins {
                    out.push(l.clone());
                }
            }
            (Some(b), None, Some(r)) => {
                if r != b && remote_wins {
                    out.push(r.clone());
                }
            }
            (None, Some(l), None) => out.push(l.clone()),
            (None, None, Some(r)) => out.push(r.clone()),
            _ => {}
        }
    }
    out
}

fn collect_subtask_order(
    base: Option<&[TodoSubtask]>,
    local: &[TodoSubtask],
    remote: &[TodoSubtask],
    remote_wins: bool,
) -> Vec<String> {
    let mut order = Vec::new();
    let mut push_unique = |id: &str| {
        if !order.iter().any(|existing| existing == id) {
            order.push(id.to_string());
        }
    };

    if let Some(b) = base {
        let base_ids: BTreeSet<&str> = b.iter().map(|s| s.id.as_str()).collect();
        let b_existing: Vec<&str> = b
            .iter()
            .filter(|s| {
                local.iter().any(|l| l.id == s.id) && remote.iter().any(|r| r.id == s.id)
            })
            .map(|s| s.id.as_str())
            .collect();
        let l_existing: Vec<&str> = local
            .iter()
            .filter(|s| base_ids.contains(s.id.as_str()) && remote.iter().any(|r| r.id == s.id))
            .map(|s| s.id.as_str())
            .collect();
        let r_existing: Vec<&str> = remote
            .iter()
            .filter(|s| base_ids.contains(s.id.as_str()) && local.iter().any(|l| l.id == s.id))
            .map(|s| s.id.as_str())
            .collect();

        let (first_slice, second_slice) = if r_existing == b_existing && l_existing != b_existing {
            (local, remote)
        } else if l_existing == b_existing && r_existing != b_existing {
            (remote, local)
        } else if remote_wins {
            (remote, local)
        } else {
            (local, remote)
        };

        for s in first_slice {
            if base_ids.contains(s.id.as_str()) {
                push_unique(&s.id);
            }
        }
        for s in second_slice {
            if base_ids.contains(s.id.as_str()) {
                push_unique(&s.id);
            }
        }
        let (winner, loser) = if remote_wins {
            (remote, local)
        } else {
            (local, remote)
        };
        for s in winner {
            if !base_ids.contains(s.id.as_str()) {
                push_unique(&s.id);
            }
        }
        for s in loser {
            if !base_ids.contains(s.id.as_str()) {
                push_unique(&s.id);
            }
        }
    } else {
        let (winner, loser) = if remote_wins {
            (remote, local)
        } else {
            (local, remote)
        };
        for s in winner {
            push_unique(&s.id);
        }
        for s in loser {
            push_unique(&s.id);
        }
    }

    order
}

fn should_sync_in_snapshot(item: &TodoItem, now_ms: i64) -> bool {
    if !item.is_tombstoned() {
        return true;
    }
    rfc3339_ms(item.deleted_at.as_deref())
        .map(|del_ms| now_ms.saturating_sub(del_ms) < NINETY_DAYS_MS)
        .unwrap_or(true)
}

pub fn serialize_task_payload(
    item: &TodoItem,
    extra: Option<&BTreeMap<String, Value>>,
) -> Value {
    let mut map = match serde_json::to_value(item) {
        Ok(Value::Object(obj)) => obj,
        _ => serde_json::Map::new(),
    };
    if let Some(extras) = extra {
        for (k, v) in extras {
            if !KNOWN_TASK_KEYS.contains(&k.as_str()) && !map.contains_key(k) {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(map)
}

fn room_tag(room: &str) -> String {
    let sum = digest::digest(&digest::SHA256, room.as_bytes());
    sum.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_uuid_v4() -> String {
    let mut b = [0u8; 16];
    let _ = SystemRandom::new().fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone_link::relay_crypto::{base64url_encode, PairingKey};
    use crate::tasks::model::{TodoPriority, TODO_TIME_ZONE};
    use crate::tasks::store::CreateTaskInput;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use base64::Engine;
    use std::sync::Mutex;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-sync-test-{name}-{}-{}",
            std::process::id(),
            new_uuid_v4()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
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

    fn sample_item(id: &str, text: &str, version: u32) -> TodoItem {
        TodoItem {
            id: id.into(),
            text: text.into(),
            due: String::new(),
            due_date: Some("2026-11-20".into()),
            due_time: Some("10:00".into()),
            time_zone: TODO_TIME_ZONE.into(),
            alert_minutes: 20,
            priority: TodoPriority::Normal,
            subtasks: Vec::new(),
            recurrence: None,
            series_id: None,
            done: false,
            created_at: "2026-11-14T10:00:00Z".into(),
            done_at: None,
            snoozed_until: None,
            source: None,
            source_stale: false,
            version,
            deleted_at: None,
        }
    }

    struct MemMailbox {
        to_pc: Mutex<Vec<MailboxEnvelopeItem>>,
        to_phone: Mutex<Vec<(String, Vec<u8>)>>,
        next_seq: Mutex<u64>,
        acked_pc: Mutex<u64>,
        pending_phone_override: Mutex<Option<u64>>,
        revoked: Mutex<bool>,
        fail_with: Mutex<Option<MailboxHttpError>>,
    }

    impl Default for MemMailbox {
        fn default() -> Self {
            Self {
                to_pc: Mutex::new(Vec::new()),
                to_phone: Mutex::new(Vec::new()),
                next_seq: Mutex::new(1),
                acked_pc: Mutex::new(0),
                pending_phone_override: Mutex::new(None),
                revoked: Mutex::new(false),
                fail_with: Mutex::new(None),
            }
        }
    }

    impl MemMailbox {
        fn push_for_pc_raw(&self, id: &str, sealed_bytes: Vec<u8>) -> u64 {
            let mut seq_guard = self.next_seq.lock().unwrap();
            let seq = *seq_guard;
            *seq_guard += 1;
            self.to_pc.lock().unwrap().push(MailboxEnvelopeItem {
                seq,
                id: id.to_string(),
                created_at: 1_794_668_400_000,
                size_bytes: sealed_bytes.len(),
                payload: sealed_bytes,
            });
            seq
        }

        fn push_for_pc_envelope(&self, creds: &Credentials, env: &SyncEnvelope) -> u64 {
            let km = k_mailbox(&creds.key);
            let json = serde_json::to_string(env).unwrap();
            let sealed = seal_mailbox(&km, &creds.room, MailboxRole::Pc, &json).unwrap();
            self.push_for_pc_raw(&env.mutation_id, sealed)
        }
    }

    impl MailboxTransport for MemMailbox {
        fn post(
            &self,
            _target: &Target,
            _creds: &Credentials,
            to: MailboxRole,
            msg_id: &str,
            body: &[u8],
        ) -> Result<MailboxPostOutcome, MailboxHttpError> {
            if let Some(err) = self.fail_with.lock().unwrap().clone() {
                return Err(err);
            }
            if *self.revoked.lock().unwrap() {
                return Err(MailboxHttpError::Revoked);
            }
            assert_eq!(to, MailboxRole::Phone);
            let mut phone_q = self.to_phone.lock().unwrap();
            if let Some(pos) = phone_q.iter().position(|(id, _)| id == msg_id) {
                return Ok(MailboxPostOutcome {
                    seq: (pos + 1) as u64,
                    id: msg_id.to_string(),
                    duplicate: true,
                });
            }
            phone_q.push((msg_id.to_string(), body.to_vec()));
            Ok(MailboxPostOutcome {
                seq: phone_q.len() as u64,
                id: msg_id.to_string(),
                duplicate: false,
            })
        }

        fn list(
            &self,
            _target: &Target,
            _creds: &Credentials,
            for_role: MailboxRole,
            after_seq: u64,
            limit: u32,
        ) -> Result<MailboxListResponse, MailboxHttpError> {
            if let Some(err) = self.fail_with.lock().unwrap().clone() {
                return Err(err);
            }
            if *self.revoked.lock().unwrap() {
                return Err(MailboxHttpError::Revoked);
            }
            assert_eq!(for_role, MailboxRole::Pc);
            let pending: Vec<MailboxEnvelopeItem> = self
                .to_pc
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.seq > after_seq)
                .cloned()
                .collect();
            let total_pending = pending.len() as u64;
            let messages: Vec<MailboxEnvelopeItem> =
                pending.into_iter().take(limit as usize).collect();
            let latest_seq = messages.last().map(|m| m.seq).unwrap_or(after_seq);
            Ok(MailboxListResponse {
                messages,
                latest_seq,
                pending_count: total_pending,
            })
        }

        fn ack(
            &self,
            _target: &Target,
            _creds: &Credentials,
            for_role: MailboxRole,
            upto_seq: u64,
        ) -> Result<MailboxAckResponse, MailboxHttpError> {
            if let Some(err) = self.fail_with.lock().unwrap().clone() {
                return Err(err);
            }
            if *self.revoked.lock().unwrap() {
                return Err(MailboxHttpError::Revoked);
            }
            assert_eq!(for_role, MailboxRole::Pc);
            let mut q = self.to_pc.lock().unwrap();
            let before = q.len();
            q.retain(|m| m.seq > upto_seq);
            let deleted = (before - q.len()) as u64;
            *self.acked_pc.lock().unwrap() = upto_seq;
            Ok(MailboxAckResponse {
                acked_upto: upto_seq,
                deleted,
                remaining: q.len() as u64,
            })
        }

        fn stats(
            &self,
            _target: &Target,
            _creds: &Credentials,
        ) -> Result<MailboxStatsResponse, MailboxHttpError> {
            if let Some(err) = self.fail_with.lock().unwrap().clone() {
                return Err(err);
            }
            let revoked = *self.revoked.lock().unwrap();
            if revoked {
                return Err(MailboxHttpError::Revoked);
            }
            let pending_pc = self.to_pc.lock().unwrap().len() as u64;
            let pending_phone = self
                .pending_phone_override
                .lock()
                .unwrap()
                .unwrap_or_else(|| self.to_phone.lock().unwrap().len() as u64);
            Ok(MailboxStatsResponse {
                pending_pc,
                pending_phone,
                stored_bytes: 0,
                revoked,
            })
        }
    }

    fn run_local_sync(
        store: &mut TaskStore,
        sync: &mut SyncStore,
        target: &Target,
        creds: &Credentials,
        transport: &dyn MailboxTransport,
        now_ms: i64,
    ) -> SyncCycleOutcome {
        let cell = Mutex::new((store, sync));
        run_sync_cycle(
            target,
            creds,
            transport,
            now_ms,
            |f: &mut dyn FnMut(&mut TaskStore, &mut SyncStore) -> Result<(), String>| {
                let mut guard = cell.lock().unwrap();
                let (ref mut st, ref mut sy) = *guard;
                f(st, sy)
            },
        )
        .unwrap()
    }

    #[test]
    fn device_id_persists_across_restarts_and_sync_json_contains_no_secrets() {
        let dir = temp_dir("persist");
        let (target, creds) = test_creds(10);
        let first_id = {
            let mut sync = SyncStore::load_at(dir.clone());
            assert_eq!(sync.device_id().len(), 36);
            sync.ensure_room(&creds.room, &[], 1_794_668_400_000)
                .unwrap();
            sync.device_id().to_string()
        };
        let reloaded = SyncStore::load_at(dir.clone());
        assert_eq!(reloaded.device_id(), first_id);

        let raw = fs::read_to_string(dir.join("sync.json")).unwrap();
        assert!(!raw.contains(&creds.room));
        assert!(!raw.contains(&creds.access));
        assert!(!raw.contains(&creds.key.to_base64url()));
        let _ = target;
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn outbox_coalesces_local_mutations_preserving_initial_base_version() {
        let dir = temp_dir("coalesce");
        let now = 1_794_668_400_000;
        let mut store = TaskStore::load_at(dir.clone());
        let mut sync = SyncStore::load_at(dir.clone());

        let created = store
            .create(
                CreateTaskInput {
                    text: "Comprar café".into(),
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
                now,
            )
            .unwrap();
        sync.enqueue_local_mutation(&created, 0, now).unwrap();
        let m1 = sync.outbox()[0].envelope.mutation_id.clone();

        let updated = store
            .update(crate::tasks::store::UpdateTaskInput {
                id: created.id.clone(),
                text: Some("Comprar café em grãos".into()),
                due: None,
                due_date: None,
                due_time: None,
                time_zone: None,
                alert_minutes: None,
                priority: Some(TodoPriority::High),
                recurrence: None,
                clear_snooze: false,
            })
            .unwrap();
        sync.enqueue_local_mutation(&updated, created.version, now + 5000)
            .unwrap();

        assert_eq!(sync.outbox().len(), 1);
        let entry = &sync.outbox()[0];
        assert_eq!(entry.envelope.base_version, 0);
        assert_eq!(entry.envelope.new_version, 2);
        assert_ne!(entry.envelope.mutation_id, m1);
        assert_eq!(entry.envelope.payload["text"], "Comprar café em grãos");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fixture_envelopes_apply_idempotently_and_preserve_tombstone() {
        let dir = temp_dir("fixture");
        let now = 1_794_760_000_000;
        let mut store = TaskStore::load_at(dir.clone());
        let mut sync = SyncStore::load_at(dir.clone());
        let (target, creds) = test_creds(20);
        let mailbox = MemMailbox::default();

        let raw = include_str!("../../tests/fixtures/contrato-sync-v1-task.json");
        let envelopes: Vec<SyncEnvelope> = serde_json::from_str(raw).unwrap();
        for env in &envelopes {
            mailbox.push_for_pc_envelope(&creds, env);
        }
        // Duplicate of env[1] with same mutation_id -> must be ignored idempotently.
        mailbox.push_for_pc_envelope(&creds, &envelopes[1]);

        let out = run_local_sync(&mut store, &mut sync, &target, &creds, &mailbox, now);
        assert!(out.changed_tasks);
        assert!(out.dismiss_ids.contains(&"task-syn-001".to_string()));
        assert_eq!(*mailbox.acked_pc.lock().unwrap(), 4);
        assert_eq!(sync.last_pc_seq(), 4);

        let t1 = store.find_any("task-syn-001").unwrap();
        assert!(t1.done);
        assert_eq!(t1.version, 2);
        let t2 = store.find_any("task-syn-002").unwrap();
        assert!(t2.is_tombstoned());
        assert_eq!(t2.version, 4);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_fields_are_preserved_across_roundtrip_and_future_entities_quarantined() {
        let dir = temp_dir("quarantine-extra");
        let now = 1_794_760_000_000;
        let mut store = TaskStore::load_at(dir.clone());
        let mut sync = SyncStore::load_at(dir.clone());
        let (target, creds) = test_creds(30);
        let mailbox = MemMailbox::default();

        let mut env = SyncEnvelope {
            contract_version: 1,
            mutation_id: "mut-extra-001".into(),
            device_id: "dev-phone-01".into(),
            origin_role: "phone".into(),
            entity_type: "task".into(),
            entity_id: "t-extra".into(),
            base_version: 0,
            new_version: 1,
            updated_at: "2026-11-14T12:00:00Z".into(),
            payload: serde_json::json!({
                "id": "t-extra",
                "text": "Com campo futuro",
                "due": "",
                "due_date": null,
                "due_time": null,
                "time_zone": "America/Sao_Paulo",
                "alert_minutes": 20,
                "priority": "normal",
                "subtasks": [],
                "recurrence": null,
                "series_id": null,
                "done": false,
                "created_at": "2026-11-14T12:00:00Z",
                "done_at": null,
                "snoozed_until": null,
                "version": 1,
                "deleted_at": null,
                "future_badge": { "color": "amber" }
            }),
        };
        mailbox.push_for_pc_envelope(&creds, &env);

        // v2 contract envelope -> quarantined
        let env_v2 = serde_json::json!({
            "contract_version": 2,
            "mutation_id": "mut-v2-001",
            "device_id": "dev-phone-01",
            "origin_role": "phone",
            "entity_type": "task",
            "entity_id": "t-v2",
            "payload": { "anything": true }
        });
        let km = k_mailbox(&creds.key);
        mailbox.push_for_pc_raw(
            "mut-v2-001",
            seal_mailbox(&km, &creds.room, MailboxRole::Pc, &env_v2.to_string()).unwrap(),
        );

        // non-task v1.1 envelope -> quarantined
        env.mutation_id = "mut-note-001".into();
        env.entity_type = "note".into();
        mailbox.push_for_pc_envelope(&creds, &env);

        // poison bytes -> poison_count incremented and acked
        mailbox.push_for_pc_raw("mut-poison-001", vec![0x01; 160]);

        run_local_sync(&mut store, &mut sync, &target, &creds, &mailbox, now);
        assert_eq!(sync.quarantined().len(), 2);
        let status = sync.status(true);
        assert_eq!(status.quarantined_count, 2);
        assert_eq!(status.poison_count, 1);
        assert_eq!(status.state, TaskSyncState::Synced);

        // Local update on t-extra preserves `future_badge` in outgoing envelope payload.
        let updated = store
            .update(crate::tasks::store::UpdateTaskInput {
                id: "t-extra".into(),
                text: Some("Com campo futuro editado".into()),
                due: None,
                due_date: None,
                due_time: None,
                time_zone: None,
                alert_minutes: None,
                priority: None,
                recurrence: None,
                clear_snooze: false,
            })
            .unwrap();
        sync.enqueue_local_mutation(&updated, 1, now + 1000).unwrap();
        assert_eq!(
            sync.outbox()[0].envelope.payload["future_badge"],
            serde_json::json!({ "color": "amber" })
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn three_way_merge_handles_tombstone_restore_done_snooze_subtasks_and_text_conflict() {
        let base = TodoItem {
            id: "t-merge".into(),
            text: "Base text".into(),
            due: "".into(),
            due_date: Some("2026-11-20".into()),
            due_time: Some("09:00".into()),
            time_zone: TODO_TIME_ZONE.into(),
            alert_minutes: 20,
            priority: TodoPriority::Normal,
            subtasks: vec![
                TodoSubtask {
                    id: "s1".into(),
                    text: "Sub 1".into(),
                    done: false,
                },
                TodoSubtask {
                    id: "s2".into(),
                    text: "Sub 2".into(),
                    done: false,
                },
            ],
            recurrence: None,
            series_id: None,
            done: false,
            created_at: "2026-11-14T10:00:00Z".into(),
            done_at: None,
            snoozed_until: None,
            source: None,
            source_stale: false,
            version: 1,
            deleted_at: None,
        };

        let mut local = base.clone();
        local.text = "PC text".into();
        local.priority = TodoPriority::Urgent;
        local.snoozed_until = Some("2026-11-14T12:00:00Z".into());
        local.subtasks[0].text = "Sub 1 PC".into();
        // s2 untouched locally; s3 added locally
        local.subtasks.push(TodoSubtask {
            id: "s3".into(),
            text: "Sub 3 PC".into(),
            done: false,
        });
        local.version = 2;

        let mut remote = base.clone();
        remote.text = "Phone text".into();
        remote.done = true;
        remote.done_at = Some("2026-11-14T11:30:00Z".into());
        remote.snoozed_until = Some("2026-11-14T12:15:00Z".into());
        remote.subtasks = vec![
            TodoSubtask {
                id: "s1".into(),
                text: "Sub 1 Phone".into(),
                done: true,
            },
            // s2 removed on phone while untouched on PC
            TodoSubtask {
                id: "s4".into(),
                text: "Sub 4 Phone".into(),
                done: false,
            },
        ];
        remote.version = 2;

        let empty = BTreeMap::new();
        let res = reconcile_three_way(
            Some(&base),
            &local,
            None,
            "2026-11-14T11:00:00Z",
            "dev-pc",
            &remote,
            &empty,
            "2026-11-14T11:05:00Z",
            "dev-phone",
            2,
        );

        // Remote is newer (11:05 > 11:00) -> wins text & subtask text conflicts, but PC priority survives!
        assert_eq!(res.item.text, "Phone text");
        assert_eq!(res.item.priority, TodoPriority::Urgent);
        assert!(res.item.done);
        assert_eq!(res.item.done_at.as_deref(), Some("2026-11-14T11:30:00Z"));
        assert_eq!(res.item.snoozed_until.as_deref(), Some("2026-11-14T12:15:00Z"));
        assert_eq!(res.item.version, 3);
        assert_eq!(res.conflicts.len(), 2);
        assert!(res
            .conflicts
            .iter()
            .any(|c| c.id == "t-merge:text" && c.winner_side == "remote" && c.loser_text == "PC text"));
        assert!(res
            .conflicts
            .iter()
            .any(|c| c.id == "t-merge:subtasks.s1.text" && c.winner_text == "Sub 1 Phone"));
        // s2 removed, s1 done, s4 & s3 added
        let sub_ids: Vec<&str> = res.item.subtasks.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(sub_ids, vec!["s1", "s4", "s3"]);
        assert!(res.item.subtasks[0].done);

        // Explicit restore: B deleted, L unchanged (deleted), R restored -> R wins
        let mut del_base = base.clone();
        del_base.deleted_at = Some("2026-11-14T11:00:00Z".into());
        del_base.version = 3;
        let del_local = del_base.clone();
        let mut restored_remote = del_base.clone();
        restored_remote.deleted_at = None;
        restored_remote.version = 4;
        let restored = reconcile_three_way(
            Some(&del_base),
            &del_local,
            None,
            "2026-11-14T11:00:00Z",
            "dev-pc",
            &restored_remote,
            &empty,
            "2026-11-14T11:10:00Z",
            "dev-phone",
            4,
        );
        assert_eq!(restored.item.deleted_at, None);

        // Concurrent delete vs edit: B alive, L edited, R deleted -> tombstone wins
        let mut del_remote = base.clone();
        del_remote.deleted_at = Some("2026-11-14T11:20:00Z".into());
        del_remote.version = 2;
        let tomb = reconcile_three_way(
            Some(&base),
            &local,
            None,
            "2026-11-14T11:30:00Z",
            "dev-pc",
            &del_remote,
            &empty,
            "2026-11-14T11:20:00Z",
            "dev-phone",
            2,
        );
        assert!(tomb.item.is_tombstoned());
    }

    #[test]
    fn sync_status_distinguishes_saved_on_pc_waiting_phone_synced_revoked_and_error() {
        let dir = temp_dir("status-flow");
        let now = 1_794_668_400_000;
        let mut store = TaskStore::load_at(dir.clone());
        let mut sync = SyncStore::load_at(dir.clone());
        let (target, creds) = test_creds(40);
        let mailbox = MemMailbox::default();

        // Unpaired -> LocalOnly
        assert_eq!(sync.status(false).state, TaskSyncState::LocalOnly);

        let item = store
            .create(
                CreateTaskInput {
                    text: "Sincronizar relatório".into(),
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
                now,
            )
            .unwrap();
        sync.enqueue_local_mutation(&item, 0, now).unwrap();
        assert_eq!(sync.status(true).state, TaskSyncState::SavedOnPc);
        assert_eq!(sync.status(true).unsynced_item_ids, vec![item.id.clone()]);

        // Sync while phone still has pending_phone = 1 -> WaitingPhone ("Salvo neste PC · aguardando celular")
        run_local_sync(&mut store, &mut sync, &target, &creds, &mailbox, now + 1000);
        let waiting = sync.status(true);
        assert_eq!(waiting.state, TaskSyncState::WaitingPhone);
        assert_eq!(waiting.pending_outbox, 0);
        assert_eq!(waiting.pending_peer, Some(1));
        assert_eq!(waiting.unsynced_item_ids, vec![item.id.clone()]);

        // Phone drains mailbox -> stats pending_phone = 0 -> Synced!
        mailbox.to_phone.lock().unwrap().clear();
        run_local_sync(&mut store, &mut sync, &target, &creds, &mailbox, now + 2000);
        let synced = sync.status(true);
        assert_eq!(synced.state, TaskSyncState::Synced);
        assert_eq!(synced.pending_peer, Some(0));
        assert!(synced.unsynced_item_ids.is_empty());
        assert!(synced.last_synced_at.is_some());

        // Network error -> Error (never exposes task text or secrets)
        let item2 = store
            .update(crate::tasks::store::UpdateTaskInput {
                id: item.id.clone(),
                text: Some("SEGREDO-DA-TAREFA".into()),
                due: None,
                due_date: None,
                due_time: None,
                time_zone: None,
                alert_minutes: None,
                priority: None,
                recurrence: None,
                clear_snooze: false,
            })
            .unwrap();
        sync.enqueue_local_mutation(&item2, item.version, now + 3000)
            .unwrap();
        *mailbox.fail_with.lock().unwrap() = Some(MailboxHttpError::Unauthorized);
        let err_out = run_local_sync(&mut store, &mut sync, &target, &creds, &mailbox, now + 4000);
        assert_eq!(err_out.pause, Some(SyncPauseReason::Fatal));
        let err_status = sync.status(true);
        assert_eq!(err_status.state, TaskSyncState::Error);
        let err_msg = err_status.last_error.as_deref().unwrap();
        assert!(!err_msg.contains("SEGREDO-DA-TAREFA"));
        assert!(!err_msg.contains(&creds.room));
        assert!(!err_msg.contains(&creds.access));

        // 410 Revoked -> Revoked
        *mailbox.fail_with.lock().unwrap() = None;
        *mailbox.revoked.lock().unwrap() = true;
        let rev_out = run_local_sync(&mut store, &mut sync, &target, &creds, &mailbox, now + 5000);
        assert_eq!(rev_out.pause, Some(SyncPauseReason::Revoked));
        assert_eq!(sync.status(true).state, TaskSyncState::Revoked);

        // New room pairing resets revoked and queues local tasks snapshot
        let (_t2, new_creds) = test_creds(90);
        let new_mailbox = MemMailbox::default();
        run_local_sync(&mut store, &mut sync, &target, &new_creds, &new_mailbox, now + 6000);
        assert!(!sync.is_revoked());
        assert_eq!(sync.status(true).state, TaskSyncState::WaitingPhone);
        assert_eq!(new_mailbox.to_phone.lock().unwrap().len(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn end_to_end_http_loopback_with_relay_mailbox_transport() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let dir = temp_dir("http-loopback");
        let now = 1_794_668_400_000;
        let mut store = TaskStore::load_at(dir.clone());
        let mut sync = SyncStore::load_at(dir.clone());
        let (_, creds) = test_creds(55);
        let km = k_mailbox(&creds.key);

        let remote_env = SyncEnvelope {
            contract_version: 1,
            mutation_id: "mut-http-phone-1".into(),
            device_id: "dev-phone-http".into(),
            origin_role: "phone".into(),
            entity_type: "task".into(),
            entity_id: "task-http-1".into(),
            base_version: 0,
            new_version: 1,
            updated_at: "2026-11-14T12:00:00Z".into(),
            payload: serde_json::to_value(sample_item("task-http-1", "Tarefa vinda do S25", 1)).unwrap(),
        };
        let sealed_for_pc = seal_mailbox(
            &km,
            &creds.room,
            MailboxRole::Pc,
            &serde_json::to_string(&remote_env).unwrap(),
        )
        .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let target = Target {
            tls: false,
            host: "127.0.0.1".into(),
            port,
        };

        let local_item = store
            .create(
                CreateTaskInput {
                    text: "Tarefa local PC".into(),
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
                now,
            )
            .unwrap();
        sync.enqueue_local_mutation(&local_item, 0, now).unwrap();

        let server = std::thread::spawn(move || {
            let list_json = serde_json::json!({
                "messages": [{
                    "seq": 1,
                    "id": "mut-http-phone-1",
                    "createdAt": 1794668400000u64,
                    "sizeBytes": sealed_for_pc.len(),
                    "payloadB64": BASE64_STANDARD.encode(&sealed_for_pc),
                }],
                "latestSeq": 1,
                "pendingCount": 1
            })
            .to_string();
            let responses = [
                ("GET /v1/mailbox/", 200, list_json),
                (
                    "POST /v1/mailbox/",
                    200,
                    r#"{"ackedUpto":1,"deleted":1,"remaining":0}"#.to_string(),
                ),
                (
                    "POST /v1/mailbox/",
                    201,
                    r#"{"seq":1,"id":"pc-msg","createdAt":1794668401000,"duplicate":false}"#.to_string(),
                ),
                (
                    "GET /v1/mailbox/",
                    200,
                    r#"{"pendingPc":0,"pendingPhone":0,"storedBytes":0,"revoked":false}"#.to_string(),
                ),
            ];
            for (expected_prefix, code, body) in responses {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let head_end = loop {
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break pos;
                    }
                    let n = sock.read(&mut tmp).unwrap();
                    assert!(n > 0);
                    buf.extend_from_slice(&tmp[..n]);
                };
                let head = std::str::from_utf8(&buf[..head_end]).unwrap();
                let req_line = head.lines().next().unwrap();
                assert!(req_line.starts_with(expected_prefix), "{req_line}");
                let cl: usize = head
                    .lines()
                    .find_map(|l| {
                        l.split_once(':').and_then(|(k, v)| {
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                while buf.len() < head_end + 4 + cl {
                    let n = sock.read(&mut tmp).unwrap();
                    assert!(n > 0);
                    buf.extend_from_slice(&tmp[..n]);
                }
                let resp = format!(
                    "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                sock.write_all(resp.as_bytes()).unwrap();
            }
        });

        let outcome = run_local_sync(
            &mut store,
            &mut sync,
            &target,
            &creds,
            &RelayMailboxTransport,
            now + 1000,
        );
        server.join().unwrap();

        assert!(outcome.changed_tasks);
        assert!(outcome.pause.is_none());
        assert_eq!(store.active_items().len(), 2);
        assert_eq!(sync.status(true).state, TaskSyncState::Synced);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sync_module_never_logs_or_prints() {
        let src = include_str!("sync.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for forbidden in ["println!", "eprintln!", "dbg!", "log::", "tracing::"] {
            assert!(!code.contains(forbidden), "tasks/sync.rs must not use {forbidden}");
        }
    }
}
