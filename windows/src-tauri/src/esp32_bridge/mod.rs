//! Ponte nativa do Lumen com a placa ESP32-S3 AMOLED (L9).
//!
//! Expõe servidor HTTP LAN (padrão `:8787`), pareamento de 6 dígitos em tempo constante,
//! armazenamento de chaves no cofre do Windows (`secrets.rs`), ponte de voz, sync de tarefas,
//! relay Cloudflare criptografado (AES-256-GCM), controle nativo de mídia e painel de cotas/agentes.

pub mod agent_status;
pub mod audio_wav;
pub mod crypto;
pub mod interactions;
pub mod max_tracker;
pub mod mqtt;
pub mod relay;
pub mod server;
pub mod tokens_agents;
pub mod usage_history;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tauri::{AppHandle, Manager, State};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::{secrets, settings};

use self::tokens_agents::ClaudeQuotaCache;

pub const DEFAULT_BRIDGE_PORT: u16 = 8787;
pub const DEFAULT_USAGE_PORT: u16 = 8737;

const SECRET_RELAY_KEY: &str = "esp32-relay-key";
pub(crate) const SECRET_MQTT_URI: &str = "esp32-mqtt-uri";
pub(crate) const SECRET_MQTT_USER: &str = "esp32-mqtt-user";
pub(crate) const SECRET_MQTT_PASS: &str = "esp32-mqtt-pass";
pub(crate) const SECRET_MQTT_ROOM: &str = "esp32-mqtt-room";
pub(crate) const SECRET_MQTT_KEY: &str = "esp32-mqtt-key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDeviceKey {
    pub device_id: String,
    pub key: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Esp32BridgeStatus {
    pub enabled: bool,
    pub port: u16,
    pub running: bool,
    pub mode: String,
    pub local_urls: Vec<String>,
    pub mqtt_configured: bool,
    pub mqtt_connected: bool,
    pub mqtt_host: String,
    pub mqtt_room: String,
    pub mqtt_board_online: bool,
    pub mqtt_error: Option<String>,
    pub relay_enabled: bool,
    pub relay_url: String,
    pub relay_key_configured: bool,
    pub agent_panel_enabled: bool,
    pub usage_port: u16,
    pub usage_running: bool,
    pub last_error: Option<String>,
}

pub struct Esp32Runtime {
    pub app: Option<AppHandle>,
    pub keys: Mutex<Vec<PairedDeviceKey>>,
    pub last_error: Mutex<Option<String>>,
    pub running: AtomicBool,
    pub usage_running: AtomicBool,
    pub agent_panel_enabled: AtomicBool,
    pub playing: AtomicBool,
    pub claude_quota: Mutex<ClaudeQuotaCache>,
    pub claude_model_week: Mutex<usage_history::ClaudeModelWeekQuota>,
    pub agy_quota: Mutex<ClaudeQuotaCache>,
    pub usage_history: Arc<usage_history::UsageHistoryStore>,
    pub max_tracker: Arc<max_tracker::MaxTrackerStore>,
    pub interactions: Arc<interactions::InteractionStore>,
    pub agent_monitor: Arc<agent_status::AgentStatusMonitor>,
    pub mqtt_state: Arc<mqtt::MqttRuntimeState>,
    pub mqtt_handle: Mutex<Option<mqtt::MqttHandle>>,
    pub mqtt_voice_bearer: Mutex<Option<String>>,
    pub mqtt_room_key: Mutex<Option<String>>,
    pub mqtt_known_synced_ids: Mutex<HashSet<String>>,
    pub mqtt_last_published: Mutex<HashMap<String, String>>,
    shutdown_tx: Mutex<Option<watch::Sender<bool>>>,
    lan_last_ok: Mutex<Option<Instant>>,
}

impl Esp32Runtime {
    pub fn new(app: Option<AppHandle>) -> Arc<Self> {
        let mqtt_state = Arc::new(mqtt::MqttRuntimeState::new());
        let mut initial_voice_bearer = None;
        let mut initial_room_key = None;
        let initial_keys = if let Some(cfg) = load_mqtt_config_from_vault() {
            mqtt_state.set_configured(crypto::extract_broker_host(&cfg.uri), cfg.room);
            initial_voice_bearer = Some(crypto::voice_bearer_hex(&cfg.key));
            initial_room_key = Some(cfg.key);
            // Migração limpa (Q2.6): MQTT já configurado, limpa secrets legados do cofre.
            secrets::clear_legacy_esp32_secrets();
            Vec::new()
        } else {
            load_legacy_keys_from_vault()
        };
        let history_path = if app.is_some() {
            usage_history::default_history_path()
        } else {
            static TEST_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "lumen-esp32-rt-history-{}-{n}.json",
                std::process::id()
            ))
        };
        let max_tracker_path = if app.is_some() {
            max_tracker::default_max_tracker_path()
        } else {
            static TRACKER_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = TRACKER_SEQ.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "lumen-esp32-rt-max-tracker-{}-{n}.json",
                std::process::id()
            ))
        };
        Arc::new(Self {
            app,
            keys: Mutex::new(initial_keys),
            last_error: Mutex::new(None),
            running: AtomicBool::new(false),
            usage_running: AtomicBool::new(false),
            agent_panel_enabled: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            claude_quota: Mutex::new(ClaudeQuotaCache::default()),
            claude_model_week: Mutex::new(usage_history::ClaudeModelWeekQuota::default()),
            agy_quota: Mutex::new(ClaudeQuotaCache::default()),
            usage_history: Arc::new(usage_history::UsageHistoryStore::new(history_path)),
            max_tracker: Arc::new(max_tracker::MaxTrackerStore::new(max_tracker_path)),
            interactions: Arc::new(interactions::InteractionStore::new(true)),
            agent_monitor: Arc::new(agent_status::AgentStatusMonitor::new()),
            mqtt_state,
            mqtt_handle: Mutex::new(None),
            mqtt_voice_bearer: Mutex::new(initial_voice_bearer),
            mqtt_room_key: Mutex::new(initial_room_key),
            mqtt_known_synced_ids: Mutex::new(HashSet::new()),
            mqtt_last_published: Mutex::new(HashMap::new()),
            shutdown_tx: Mutex::new(None),
            lan_last_ok: Mutex::new(None),
        })
    }

    pub fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn verify_bearer(&self, auth_header: Option<&str>) -> bool {
        let Some(raw) = auth_header else {
            return false;
        };
        let mut parts = raw.trim().splitn(2, ' ');
        let Some(scheme) = parts.next() else {
            return false;
        };
        let Some(token) = parts.next().map(str::trim).filter(|t| !t.is_empty()) else {
            return false;
        };
        if !scheme.eq_ignore_ascii_case("bearer") {
            return false;
        }
        let voice_bearer = self.mqtt_voice_bearer.lock().unwrap().clone();
        let keys: Vec<String> = self
            .keys
            .lock()
            .unwrap()
            .iter()
            .map(|k| k.key.clone())
            .collect();
        // Percorre todas as chaves sem curto-circuito para evitar vazamento por tempo.
        let mut matched = false;
        if let Some(expected) = voice_bearer.as_deref() {
            if constant_time_str_eq(token, expected) {
                matched = true;
            }
        }
        for stored in &keys {
            if constant_time_str_eq(token, stored) {
                matched = true;
            }
        }
        matched
    }

    pub fn has_paired_keys(&self) -> bool {
        self.mqtt_voice_bearer.lock().unwrap().is_some()
            || self.mqtt_room_key.lock().unwrap().is_some()
            || !self.keys.lock().unwrap().is_empty()
    }

    pub fn room_key_for_hmac(&self) -> Option<String> {
        if let Some(k) = self.mqtt_room_key.lock().unwrap().clone() {
            if !k.trim().is_empty() {
                return Some(k);
            }
        }
        self.keys
            .lock()
            .unwrap()
            .first()
            .map(|k| k.key.clone())
            .filter(|k| !k.trim().is_empty())
    }

    pub(crate) fn complete_mqtt_migration_with<F: FnOnce()>(&self, room_key: &str, clear_legacy: F) {
        *self.mqtt_voice_bearer.lock().unwrap() = Some(crypto::voice_bearer_hex(room_key));
        *self.mqtt_room_key.lock().unwrap() = Some(room_key.to_string());
        self.keys.lock().unwrap().clear();
        clear_legacy();
    }

    pub fn note_lan_activity(&self) {
        *self.lan_last_ok.lock().unwrap() = Some(Instant::now());
    }

    pub fn current_mode(&self) -> &'static str {
        let now = Instant::now();
        if self
            .lan_last_ok
            .lock()
            .unwrap()
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(90))
        {
            return "lan";
        }
        if self.mqtt_state.connected.load(Ordering::Relaxed) {
            return "mqtt";
        }
        "alone"
    }

    pub fn stop_listeners(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = self.mqtt_handle.lock().unwrap().take() {
            tauri::async_runtime::spawn(async move {
                let _ = handle.shutdown().await;
            });
        }
        self.mqtt_state.set_connected(false);
        self.running.store(false, Ordering::SeqCst);
        self.usage_running.store(false, Ordering::SeqCst);
    }

    pub fn status(&self, cfg: &settings::Settings) -> Esp32BridgeStatus {
        let mqtt_snap = self.mqtt_state.snapshot();
        let relay_key_configured = secrets::get_internal(SECRET_RELAY_KEY)
            .is_some_and(|k| !k.trim().is_empty());
        let port = normalize_port(cfg.esp32_bridge_port, DEFAULT_BRIDGE_PORT);
        let usage_port = normalize_port(cfg.esp32_usage_port, DEFAULT_USAGE_PORT);

        Esp32BridgeStatus {
            enabled: cfg.esp32_bridge_enabled,
            port,
            running: self.running.load(Ordering::SeqCst),
            mode: self.current_mode().to_string(),
            local_urls: local_urls_for_port(port),
            mqtt_configured: mqtt_snap.configured,
            mqtt_connected: mqtt_snap.connected,
            mqtt_host: mqtt_snap.host,
            mqtt_room: mqtt_snap.room,
            mqtt_board_online: mqtt_snap.board_online,
            mqtt_error: mqtt_snap.last_error,
            relay_enabled: cfg.esp32_relay_enabled,
            relay_url: cfg.esp32_relay_url.clone(),
            relay_key_configured,
            agent_panel_enabled: cfg.esp32_agent_panel_enabled,
            usage_port,
            usage_running: self.usage_running.load(Ordering::SeqCst),
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }
}

pub fn constant_time_str_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    ab.len() == bb.len() && bool::from(ab.ct_eq(bb))
}

fn normalize_port(port: u16, fallback: u16) -> u16 {
    if port >= 1024 {
        port
    } else {
        fallback
    }
}

fn load_legacy_keys_from_vault() -> Vec<PairedDeviceKey> {
    let Some(raw) = secrets::read_legacy_esp32_secret("esp32-bridge-keys") else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<PairedDeviceKey>>(&raw).unwrap_or_default()
}

pub(crate) fn load_mqtt_config_from_vault() -> Option<mqtt::MqttConfig> {
    let uri = secrets::get_internal(SECRET_MQTT_URI)?;
    let user = secrets::get_internal(SECRET_MQTT_USER)?;
    let pass = secrets::get_internal(SECRET_MQTT_PASS)?;
    let room = secrets::get_internal(SECRET_MQTT_ROOM)?;
    let key = secrets::get_internal(SECRET_MQTT_KEY)?;
    if uri.trim().is_empty() || room.trim().is_empty() || key.trim().is_empty() {
        return None;
    }
    Some(mqtt::MqttConfig {
        uri,
        user,
        pass,
        room,
        key,
    })
}

pub(crate) fn save_mqtt_config_to_vault(code: &crypto::MqttPairCode) -> Result<(), String> {
    secrets::set_internal(SECRET_MQTT_URI, code.uri.trim())?;
    secrets::set_internal(SECRET_MQTT_USER, code.u.trim())?;
    secrets::set_internal(SECRET_MQTT_PASS, code.p.trim())?;
    secrets::set_internal(SECRET_MQTT_ROOM, code.sala.trim())?;
    secrets::set_internal(SECRET_MQTT_KEY, code.k.trim())?;
    Ok(())
}

pub(crate) fn clear_mqtt_config_from_vault() -> Result<(), String> {
    let _ = secrets::clear_internal(SECRET_MQTT_URI);
    let _ = secrets::clear_internal(SECRET_MQTT_USER);
    let _ = secrets::clear_internal(SECRET_MQTT_PASS);
    let _ = secrets::clear_internal(SECRET_MQTT_ROOM);
    let _ = secrets::clear_internal(SECRET_MQTT_KEY);
    Ok(())
}

struct RuntimeMqttCallbacks {
    rt: Arc<Esp32Runtime>,
}

impl mqtt::MqttCallbacks for RuntimeMqttCallbacks {
    fn on_task_upsert(&self, entity_id: &str, plain_json: &str) {
        self.rt
            .mqtt_known_synced_ids
            .lock()
            .unwrap()
            .insert(entity_id.to_string());
        let Ok(val) = serde_json::from_str::<serde_json::Value>(plain_json) else {
            return;
        };
        if let Some(app) = self.rt.app.as_ref() {
            let now_ms = crate::tasks::now_utc_ms_pub();
            if crate::tasks::apply_mqtt_task(app, entity_id, &val, now_ms).is_ok() {
                // Atualiza snapshot publicado para não devolver eco imediato.
                if let Some((_, env)) = crate::tasks::export_mqtt_tasks(app)
                    .into_iter()
                    .find(|(id, _)| id == entity_id)
                {
                    if let Ok(s) = serde_json::to_string(&env) {
                        self.rt
                            .mqtt_last_published
                            .lock()
                            .unwrap()
                            .insert(entity_id.to_string(), s);
                    }
                }
            }
        }
    }

    fn on_task_delete(&self, entity_id: &str) {
        self.rt
            .mqtt_known_synced_ids
            .lock()
            .unwrap()
            .remove(entity_id);
        self.rt
            .mqtt_last_published
            .lock()
            .unwrap()
            .remove(entity_id);
        if let Some(app) = self.rt.app.as_ref() {
            let now_ms = crate::tasks::now_utc_ms_pub();
            let _ = crate::tasks::delete_mqtt_task(app, entity_id, now_ms);
        }
    }

    fn on_settle_prune(&self, seen_task_ids: &HashSet<String>, retained_count: usize) {
        let Some(app) = self.rt.app.as_ref() else {
            return;
        };
        let known = self.rt.mqtt_known_synced_ids.lock().unwrap().clone();
        let now_ms = crate::tasks::now_utc_ms_pub();
        match crate::tasks::reconcile_mqtt_retained(
            app,
            &known,
            seen_task_ids,
            retained_count,
            now_ms,
        ) {
            Ok(res) => {
                if !res.deleted_ids.is_empty() {
                    let mut known_g = self.rt.mqtt_known_synced_ids.lock().unwrap();
                    let mut pub_g = self.rt.mqtt_last_published.lock().unwrap();
                    for id in &res.deleted_ids {
                        known_g.remove(id);
                        pub_g.remove(id);
                    }
                }
                self.rt.publish_tasks_diff(res.should_republish_all);
            }
            Err(_) => {
                self.rt.publish_tasks_diff(false);
            }
        }
    }

    fn on_media_action(&self, action: &str) -> Option<mqtt::MediaStatePayload> {
        handle_mqtt_media_action(&self.rt, action)
    }

    fn on_agent_decision(&self, plain_json: &str) {
        handle_mqtt_agent_decision(&self.rt, plain_json);
    }
}

pub(crate) fn handle_mqtt_agent_decision(rt: &Esp32Runtime, plain_json: &str) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(plain_json) else {
        return;
    };
    let Some(request_id) = val.get("request_id").and_then(serde_json::Value::as_str) else {
        return;
    };
    let Some(view_sha256) = val.get("view_sha256").and_then(serde_json::Value::as_str) else {
        return;
    };
    let Some(verdict) = val.get("verdict").and_then(serde_json::Value::as_str) else {
        return;
    };
    let Some(ts) = val.get("ts").and_then(serde_json::Value::as_i64) else {
        return;
    };
    let now_ms = interactions::InteractionStore::now_ms();
    let now_sec = Esp32Runtime::now_secs();
    if let Ok((outcome, _)) = rt
        .interactions
        .resolve_mqtt(request_id, view_sha256, verdict, ts, now_ms, now_sec)
    {
        if let (Some(app), Some(pid)) = (rt.app.as_ref(), outcome.pipe_request_id.as_deref()) {
            apply_pipe_outcome(app, pid, outcome.verdict);
        }
    }
}

pub(crate) fn apply_pipe_outcome(app: &AppHandle, pid: &str, verdict: interactions::Verdict) {
    match verdict {
        interactions::Verdict::Approve => {
            crate::resolve_approval(app, pid, "allow");
            let _ = tauri::Emitter::emit_to(app, "island", "approval-expired", pid);
        }
        interactions::Verdict::Deny => {
            crate::resolve_approval(app, pid, "deny");
            let _ = tauri::Emitter::emit_to(app, "island", "approval-expired", pid);
        }
        interactions::Verdict::Terminal => {
            crate::recap::forget_request(app, pid);
            crate::pipe::decline(app, pid);
            let _ = tauri::Emitter::emit_to(app, "island", "approval-expired", pid);
        }
    }
}

pub(crate) fn handle_mqtt_media_action(
    rt: &Esp32Runtime,
    action: &str,
) -> Option<mqtt::MediaStatePayload> {
    if crate::integrations::PAUSED.load(Ordering::Relaxed) {
        return None;
    }
    let act = action.trim();
    if !matches!(act, "play_pause" | "next" | "prev" | "vol_up" | "vol_down") {
        return None;
    }
    let playing = match act {
        "vol_up" | "vol_down" => {
            let delta = if act == "vol_up" { 6 } else { -6 };
            let _ = std::thread::spawn(move || {
                crate::mochi::tools::audio::esp32_step_volume(delta)
            })
            .join();
            rt.playing.load(Ordering::Relaxed)
        }
        _ => {
            let owned = act.to_string();
            let res = std::thread::spawn(move || {
                crate::mochi::tools::media::esp32_media_control(&owned)
            })
            .join()
            .ok()
            .and_then(|r| r.ok());
            match res {
                Some(p) => {
                    rt.playing.store(p, Ordering::Relaxed);
                    p
                }
                None => rt.playing.load(Ordering::Relaxed),
            }
        }
    };
    Some(mqtt::MediaStatePayload {
        playing,
        title: Some(String::new()),
        artist: Some(String::new()),
    })
}

impl Esp32Runtime {
    pub fn publish_tasks_diff(&self, force_all: bool) {
        let Some(app) = self.app.as_ref() else {
            return;
        };
        let Some(handle) = self.mqtt_handle.lock().unwrap().clone() else {
            return;
        };
        if !self.mqtt_state.connected.load(Ordering::Relaxed) {
            return;
        }
        let active = crate::tasks::export_mqtt_tasks(app);
        let mut active_map: HashMap<String, String> = HashMap::with_capacity(active.len());
        for (id, env) in active {
            if let Ok(s) = serde_json::to_string(&env) {
                active_map.insert(id, s);
            }
        }

        let mut to_publish: Vec<(String, String)> = Vec::new();
        let mut to_delete: Vec<String> = Vec::new();

        {
            let mut last_pub = self.mqtt_last_published.lock().unwrap();
            let mut known = self.mqtt_known_synced_ids.lock().unwrap();
            if force_all {
                last_pub.clear();
            }
            for (id, json) in &active_map {
                if last_pub.get(id).map(String::as_str) != Some(json.as_str()) {
                    to_publish.push((id.clone(), json.clone()));
                    last_pub.insert(id.clone(), json.clone());
                    known.insert(id.clone());
                }
            }
            let removed: Vec<String> = last_pub
                .keys()
                .filter(|id| !active_map.contains_key(*id))
                .cloned()
                .collect();
            for id in removed {
                last_pub.remove(&id);
                known.remove(&id);
                to_delete.push(id);
            }
        }

        if to_publish.is_empty() && to_delete.is_empty() {
            return;
        }

        tauri::async_runtime::spawn(async move {
            for (entity_id, plain_json) in to_publish {
                let _ = handle.publish_task(entity_id, plain_json).await;
            }
            for entity_id in to_delete {
                let _ = handle.delete_task(entity_id).await;
            }
        });
    }
}

pub fn notify_tasks_changed(app: &AppHandle) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.publish_tasks_diff(false);
    }
}

fn local_urls_for_port(port: u16) -> Vec<String> {
    let mut ips = HashSet::new();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("192.0.2.1:80").is_ok() {
            if let Ok(SocketAddr::V4(addr)) = sock.local_addr() {
                if !addr.ip().is_loopback() && !addr.ip().is_unspecified() {
                    ips.insert(IpAddr::V4(*addr.ip()));
                }
            }
        }
    }
    let mut out: Vec<String> = ips.into_iter().map(|ip| format!("http://{ip}:{port}")).collect();
    if out.is_empty() {
        out.push(format!("http://127.0.0.1:{port}"));
    }
    out
}

pub fn setup(app: &AppHandle) {
    let rt = Esp32Runtime::new(Some(app.clone()));
    app.manage(rt.clone());
    let cfg = app
        .try_state::<crate::Shared>()
        .map(|s| s.settings.lock().unwrap().clone())
        .unwrap_or_default();
    apply_runtime_config(&rt, &cfg);
}

pub fn shutdown(app: &AppHandle) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.stop_listeners();
    }
}

pub fn apply_runtime_config(rt: &Arc<Esp32Runtime>, cfg: &settings::Settings) {
    rt.stop_listeners();
    *rt.last_error.lock().unwrap() = None;
    rt.agent_panel_enabled
        .store(cfg.esp32_agent_panel_enabled, Ordering::SeqCst);
    if !cfg.esp32_bridge_enabled {
        return;
    }

    let port = normalize_port(cfg.esp32_bridge_port, DEFAULT_BRIDGE_PORT);
    let usage_port = normalize_port(cfg.esp32_usage_port, DEFAULT_USAGE_PORT);
    let panel_enabled = cfg.esp32_agent_panel_enabled;
    let (tx, rx) = watch::channel(false);
    *rt.shutdown_tx.lock().unwrap() = Some(tx);

    let rt_main = rt.clone();
    let rx_main = rx.clone();
    tauri::async_runtime::spawn(async move {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                rt_main.running.store(true, Ordering::SeqCst);
                crate::log::line(format!("esp32_bridge: ouvindo em :{port}"));
                server::serve_listener(listener, rt_main.clone(), rx_main).await;
                rt_main.running.store(false, Ordering::SeqCst);
            }
            Err(err) => {
                rt_main.running.store(false, Ordering::SeqCst);
                *rt_main.last_error.lock().unwrap() =
                    Some(format!("Não foi possível abrir a porta {port}: {err}"));
                crate::log::line(format!("esp32_bridge: falha na porta {port}: {err}"));
            }
        }
    });

    if panel_enabled && usage_port != port {
        let rt_usage = rt.clone();
        let rx_usage = rx.clone();
        tauri::async_runtime::spawn(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], usage_port));
            if let Ok(listener) = TcpListener::bind(addr).await {
                rt_usage.usage_running.store(true, Ordering::SeqCst);
                crate::log::line(format!("esp32_bridge: painel /api/* também em :{usage_port}"));
                server::serve_listener(listener, rt_usage.clone(), rx_usage).await;
                rt_usage.usage_running.store(false, Ordering::SeqCst);
            }
        });
    }

    if panel_enabled {
        let rt_quota = rt.clone();
        let mut rx_quota = rx.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                refresh_claude_and_agy_quotas(&rt_quota).await;
                tokio::select! {
                    _ = rx_quota.changed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(240)) => {}
                }
            }
        });
    }

    if let Some(mqtt_cfg) = load_mqtt_config_from_vault() {
        let cb = Arc::new(RuntimeMqttCallbacks { rt: rt.clone() });
        match mqtt::spawn_mqtt_loop(
            mqtt_cfg,
            cb,
            rt.mqtt_state.clone(),
            mqtt::DEFAULT_SETTLE_DURATION,
        ) {
            Ok(handle) => {
                *rt.mqtt_handle.lock().unwrap() = Some(handle);
            }
            Err(err) => {
                rt.mqtt_state.set_last_error(Some(err));
            }
        }
    }

    if panel_enabled && cfg.esp32_relay_enabled && !cfg.esp32_relay_url.trim().is_empty() {
        let rt_relay = rt.clone();
        let mut rx_relay = rx;
        let relay_url = cfg.esp32_relay_url.trim().to_string();
        tauri::async_runtime::spawn(async move {
            let Ok(client) = relay::RelayClient::new() else {
                return;
            };
            let mut last_usage_at: Option<Instant> = None;
            tokio::select! {
                _ = rx_relay.changed() => return,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
            loop {
                if *rx_relay.borrow() {
                    break;
                }
                let relay_key = secrets::get_internal(SECRET_RELAY_KEY).unwrap_or_default();
                if !relay_key.trim().is_empty() {
                    let _ = run_usage_relay_tick(
                        &client,
                        &rt_relay,
                        &relay_url,
                        &relay_key,
                        &mut last_usage_at,
                    )
                    .await;
                }
                tokio::select! {
                    _ = rx_relay.changed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                }
            }
        });
    }
}

pub(crate) async fn run_usage_relay_tick(
    client: &relay::RelayClient,
    rt: &Arc<Esp32Runtime>,
    relay_url: &str,
    relay_key: &str,
    last_usage_at: &mut Option<Instant>,
) -> Result<(), String> {
    if crate::integrations::PAUSED.load(Ordering::Relaxed) {
        return Ok(());
    }
    if last_usage_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(300)) {
        refresh_claude_and_agy_quotas(rt).await;
        let usage = build_enriched_usage_payload(rt);
        if client.put_usage(relay_url, relay_key, &usage).await.is_ok() {
            *last_usage_at = Some(Instant::now());
        }
    }
    Ok(())
}

async fn refresh_claude_and_agy_quotas(rt: &Esp32Runtime) {
    let now_sec = Esp32Runtime::now_secs();
    let claude_due = {
        let q = rt.claude_quota.lock().unwrap();
        q.updated_at == 0 || now_sec.saturating_sub(q.updated_at) >= 240
    };
    if claude_due {
        let cred = crate::platform::home_dir().join(".claude").join(".credentials.json");
        if let Some(tok) = tokens_agents::read_claude_access_token(&cred) {
            if let Ok(http) = reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
                if let Ok(resp) = http
                    .get("https://api.anthropic.com/api/oauth/usage")
                    .header("Authorization", format!("Bearer {tok}"))
                    .header("anthropic-beta", "oauth-2025-04-20")
                    .header("User-Agent", "esp32-assistente/0.1")
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        if let Ok(val) = resp.json::<serde_json::Value>().await {
                            let ts = Esp32Runtime::now_secs();
                            rt.claude_quota
                                .lock()
                                .unwrap()
                                .update_from_oauth(&val, ts);
                            *rt.claude_model_week.lock().unwrap() =
                                usage_history::ClaudeModelWeekQuota::from_oauth_payload(&val, ts);
                        }
                    }
                }
            }
        }
    }

    let agy_due = {
        let q = rt.agy_quota.lock().unwrap();
        q.updated_at == 0 || now_sec.saturating_sub(q.updated_at) >= 240
    };
    if agy_due {
        if let Ok(Some(limits)) = tokio::task::spawn_blocking(crate::agy_plan::read).await {
            rt.agy_quota
                .lock()
                .unwrap()
                .update_from_agy_limits(&limits, Esp32Runtime::now_secs());
        }
    }
}

pub(crate) fn build_agent_status_snapshot(rt: &Esp32Runtime) -> serde_json::Value {
    let claude_dir = agent_status::default_claude_projects_dir();
    let codex_dir = agent_status::default_codex_sessions_dir();
    let now_ms = crate::tasks::now_utc_ms_pub();
    let mut snap = rt.agent_monitor.snapshot(&claude_dir, &codex_dir, now_ms);
    rt.interactions
        .attach_pending_to_agent_status(&mut snap, interactions::InteractionStore::now_ms());
    snap
}

pub(crate) fn build_enriched_usage_payload(rt: &Esp32Runtime) -> serde_json::Value {
    let snap = build_live_tokens_snapshot(rt);
    let now_sec = snap.updated_at;
    let mut samples: Vec<(&str, &str, f64, u64)> = Vec::new();

    let claude_week_reset = snap
        .claude_week_reset_min
        .filter(|&m| m > 0)
        .map(|m| now_sec + (m as u64) * 60);
    if let (Some(pct), Some(reset_at)) = (snap.claude_week_pct, claude_week_reset) {
        samples.push(("claude", "week", pct, reset_at));
    }
    if let (Some(pct), Some(m)) = (snap.claude_session_pct, snap.claude_session_reset_min.filter(|&m| m > 0)) {
        samples.push(("claude", "session", pct, now_sec + (m as u64) * 60));
    }

    let codex_week_reset = snap
        .codex_week_reset_min
        .filter(|&m| m > 0)
        .map(|m| now_sec + (m as u64) * 60);
    if let (Some(pct), Some(reset_at)) = (snap.codex_week_pct, codex_week_reset) {
        samples.push(("codex", "week", pct, reset_at));
    }
    if let (Some(pct), Some(m)) = (snap.codex_session_pct, snap.codex_session_reset_min.filter(|&m| m > 0)) {
        samples.push(("codex", "session", pct, now_sec + (m as u64) * 60));
    }

    if !samples.is_empty() {
        let _ = rt.usage_history.record_many(&samples, Some(now_sec));
    }

    if !snap.claude_stale {
        let _ = rt.max_tracker.observe_quota(
            "claude",
            snap.claude_session_pct,
            snap.claude_week_pct,
            now_sec,
        );
    }
    if !snap.codex_stale {
        let _ = rt.max_tracker.observe_quota(
            "codex",
            snap.codex_session_pct,
            snap.codex_week_pct,
            now_sec,
        );
    }

    let claude_fc = match claude_week_reset {
        Some(reset_at) => rt.usage_history.forecast("claude", "week", reset_at, now_sec),
        None => usage_history::ProviderForecast {
            state: "unavailable".to_string(),
            ..Default::default()
        },
    };
    let codex_fc = match codex_week_reset {
        Some(reset_at) => rt.usage_history.forecast("codex", "week", reset_at, now_sec),
        None => usage_history::ProviderForecast {
            state: "unavailable".to_string(),
            ..Default::default()
        },
    };

    let mut payload = tokens_agents::build_clean_usage_payload(&snap);
    usage_history::merge_forecasts_into_usage(&mut payload, &claude_fc, &codex_fc);
    let model_week = rt.claude_model_week.lock().unwrap().clone();
    usage_history::merge_model_week_into_usage(&mut payload, &model_week);
    payload
}

pub(crate) fn build_max_tracker_payload(rt: &Esp32Runtime) -> serde_json::Value {
    let claude_dir = agent_status::default_claude_projects_dir();
    let codex_dir = agent_status::default_codex_sessions_dir();
    rt.max_tracker.backfill_step(&claude_dir, &codex_dir);
    rt.max_tracker.snapshot(Esp32Runtime::now_secs())
}

pub(crate) fn build_live_tokens_snapshot(rt: &Esp32Runtime) -> tokens_agents::TokensSnapshot {
    let now_sec = Esp32Runtime::now_secs();
    let (c5, c5r, c7, c7r, c_stale) = rt.claude_quota.lock().unwrap().to_fields(now_sec);
    let (a5, a5r, a7, a7r, a_stale) = rt.agy_quota.lock().unwrap().to_fields(now_sec);
    let codex_dir = crate::platform::home_dir().join(".codex").join("sessions");
    let codex = tokens_agents::read_codex_rollout_quota(&codex_dir, now_sec);
    tokens_agents::TokensSnapshot {
        claude_session_pct: c5,
        claude_session_reset_min: c5r,
        claude_week_pct: c7,
        claude_week_reset_min: c7r,
        claude_stale: c_stale,
        codex_session_pct: codex.session_pct,
        codex_session_reset_min: codex.session_reset_min,
        codex_week_pct: codex.week_pct,
        codex_week_reset_min: codex.week_reset_min,
        codex_stale: codex.stale,
        antigravity_session_pct: a5,
        antigravity_session_reset_min: a5r,
        antigravity_week_pct: a7,
        antigravity_week_reset_min: a7r,
        antigravity_stale: a_stale,
        updated_at: now_sec,
    }
}

pub fn note_claude_statusline(app: &AppHandle, rate_limits: &serde_json::Value) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.claude_quota
            .lock()
            .unwrap()
            .update_from_statusline(rate_limits, Esp32Runtime::now_secs());
    }
}

pub fn note_claude_plan_usage(
    app: &AppHandle,
    five_hour: Option<(f64, u64)>,
    seven_day: Option<(f64, u64)>,
) {
    if five_hour.is_none() && seven_day.is_none() {
        return;
    }
    let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() else { return };
    let mut guard = rt.claude_quota.lock().unwrap();
    if let Some((used_pct, resets_at)) = five_hour {
        if used_pct.is_finite() {
            guard.session_pct = Some(used_pct.clamp(0.0, 100.0));
        }
        if resets_at > 0 {
            guard.session_resets_at = Some(if resets_at > 10_000_000_000 {
                resets_at / 1000
            } else {
                resets_at
            });
        }
    }
    if let Some((used_pct, resets_at)) = seven_day {
        if used_pct.is_finite() {
            guard.week_pct = Some(used_pct.clamp(0.0, 100.0));
        }
        if resets_at > 0 {
            guard.week_resets_at = Some(if resets_at > 10_000_000_000 {
                resets_at / 1000
            } else {
                resets_at
            });
        }
    }
    guard.updated_at = Esp32Runtime::now_secs();
}

pub fn note_agent_turn(app: &AppHandle, agent: &str, cwd: &str) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.interactions.note_turn(agent, cwd);
    }
}

pub fn note_agent_clear(app: &AppHandle) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.interactions
            .note_clear(interactions::InteractionStore::now_ms());
    }
}

pub fn note_permission_request(
    app: &AppHandle,
    pipe_request_id: &str,
    agent: &str,
    cwd: &str,
    tool: &str,
    command: &str,
) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.interactions.park_pipe_or_legacy(
            Some(pipe_request_id.to_string()),
            agent,
            cwd,
            tool,
            command,
            interactions::HOLD_SECS,
            interactions::InteractionStore::now_ms(),
        );
    }
}

pub fn clear_permission_request(app: &AppHandle, pipe_request_id: &str) {
    if let Some(rt) = app.try_state::<Arc<Esp32Runtime>>() {
        rt.interactions
            .clear_by_pipe_id(pipe_request_id, interactions::InteractionStore::now_ms());
    }
}

fn current_settings(app: &AppHandle) -> settings::Settings {
    app.try_state::<crate::Shared>()
        .map(|s| s.settings.lock().unwrap().clone())
        .unwrap_or_default()
}

fn update_esp32_settings<F>(app: &AppHandle, mutate: F) -> Result<settings::Settings, String>
where
    F: FnOnce(&mut settings::Settings),
{
    let shared = app
        .try_state::<crate::Shared>()
        .ok_or_else(|| "Estado de configurações indisponível.".to_string())?;
    let next = {
        let mut guard = shared.settings.lock().unwrap();
        mutate(&mut guard);
        guard.clone()
    };
    settings::save(&next).map_err(|e| format!("Falha ao salvar ajustes: {e}"))?;
    Ok(next)
}

#[tauri::command]
pub fn esp32_status(app: AppHandle, rt: State<'_, Arc<Esp32Runtime>>) -> Esp32BridgeStatus {
    let cfg = current_settings(&app);
    rt.status(&cfg)
}

#[tauri::command]
pub fn esp32_set_enabled(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
    enabled: bool,
    port: Option<u16>,
) -> Result<Esp32BridgeStatus, String> {
    let next = update_esp32_settings(&app, |s| {
        s.esp32_bridge_enabled = enabled;
        if let Some(p) = port {
            if p >= 1024 {
                s.esp32_bridge_port = p;
            }
        }
    })?;
    apply_runtime_config(&rt, &next);
    Ok(rt.status(&next))
}

#[tauri::command]
pub fn esp32_save_mqtt_code(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
    code_json: String,
) -> Result<Esp32BridgeStatus, String> {
    let parsed = crypto::parse_mqtt_pair_code(&code_json)?;
    save_mqtt_config_to_vault(&parsed)?;
    // Migração limpa (Q2.6): remove secrets legados depois de configurar o MQTT.
    rt.complete_mqtt_migration_with(&parsed.k, secrets::clear_legacy_esp32_secrets);
    rt.mqtt_state
        .set_configured(crypto::extract_broker_host(&parsed.uri), parsed.sala.clone());
    let next = update_esp32_settings(&app, |s| {
        s.esp32_bridge_enabled = true;
    })?;
    apply_runtime_config(&rt, &next);
    Ok(rt.status(&next))
}

#[tauri::command]
pub fn esp32_clear_mqtt(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
) -> Result<Esp32BridgeStatus, String> {
    clear_mqtt_config_from_vault()?;
    *rt.mqtt_voice_bearer.lock().unwrap() = None;
    *rt.mqtt_room_key.lock().unwrap() = None;
    rt.mqtt_state.configured.store(false, Ordering::Relaxed);
    rt.mqtt_state.set_connected(false);
    *rt.mqtt_state.host.write().unwrap() = String::new();
    *rt.mqtt_state.room.write().unwrap() = String::new();
    rt.mqtt_state.set_last_error(None);
    let cfg = current_settings(&app);
    apply_runtime_config(&rt, &cfg);
    Ok(rt.status(&cfg))
}

#[tauri::command]
pub fn esp32_save_relay(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
    enabled: bool,
    url: String,
    key: String,
) -> Result<Esp32BridgeStatus, String> {
    let trimmed_url = url.trim().trim_end_matches('/').to_string();
    let trimmed_key = key.trim().to_string();

    if enabled {
        let valid_url = trimmed_url.starts_with("https://")
            || trimmed_url.starts_with("http://127.0.0.1")
            || trimmed_url.starts_with("http://localhost");
        if !valid_url {
            return Err("URL do Relay inválida (use https://...).".into());
        }
    }

    if !trimmed_key.is_empty() {
        secrets::set_internal(SECRET_RELAY_KEY, &trimmed_key)?;
    }

    let next = update_esp32_settings(&app, |s| {
        s.esp32_relay_enabled = enabled;
        s.esp32_relay_url = trimmed_url;
    })?;
    apply_runtime_config(&rt, &next);
    Ok(rt.status(&next))
}

#[tauri::command]
pub fn esp32_set_agent_panel(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
    enabled: bool,
    usage_port: Option<u16>,
) -> Result<Esp32BridgeStatus, String> {
    let next = update_esp32_settings(&app, |s| {
        s.esp32_agent_panel_enabled = enabled;
        if let Some(p) = usage_port {
            if p >= 1024 {
                s.esp32_usage_port = p;
            }
        }
    })?;
    apply_runtime_config(&rt, &next);
    Ok(rt.status(&next))
}
