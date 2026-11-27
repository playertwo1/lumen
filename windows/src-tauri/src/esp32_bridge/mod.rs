//! Ponte nativa do Lumen com a placa ESP32-S3 AMOLED (L9).
//!
//! Expõe servidor HTTP LAN (padrão `:8787`), pareamento de 6 dígitos em tempo constante,
//! armazenamento de chaves no cofre do Windows (`secrets.rs`), ponte de voz, sync de tarefas,
//! relay Cloudflare criptografado (AES-256-GCM), controle nativo de mídia e painel de cotas/agentes.

pub mod audio_wav;
pub mod crypto;
pub mod relay;
pub mod server;
pub mod tokens_agents;

use std::collections::HashSet;
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

use self::tokens_agents::{ClaudeQuotaCache, Esp32AgentPanelState};

pub const DEFAULT_BRIDGE_PORT: u16 = 8787;
pub const DEFAULT_USAGE_PORT: u16 = 8737;
const PAIR_CODE_TTL: Duration = Duration::from_secs(300);

const SECRET_BRIDGE_KEYS: &str = "esp32-bridge-keys";
const SECRET_RELAY_KEY: &str = "esp32-relay-key";
const SECRET_RELAY_ROOM: &str = "esp32-relay-room";

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
    pub paired_count: usize,
    pub pair_code_armed: bool,
    pub pair_code_expires_in_secs: Option<u64>,
    pub relay_enabled: bool,
    pub relay_url: String,
    pub relay_room_configured: bool,
    pub relay_key_configured: bool,
    pub agent_panel_enabled: bool,
    pub usage_port: u16,
    pub usage_running: bool,
    pub last_error: Option<String>,
}

struct ArmedPairCode {
    code: String,
    expires_at: Instant,
}

pub struct Esp32Runtime {
    pub app: Option<AppHandle>,
    pub keys: Mutex<Vec<PairedDeviceKey>>,
    pair_code: Mutex<Option<ArmedPairCode>>,
    pub last_error: Mutex<Option<String>>,
    pub running: AtomicBool,
    pub usage_running: AtomicBool,
    pub playing: AtomicBool,
    pub claude_quota: Mutex<ClaudeQuotaCache>,
    pub agent_panel: Mutex<Esp32AgentPanelState>,
    shutdown_tx: Mutex<Option<watch::Sender<bool>>>,
    relay_last_ok: Mutex<Option<Instant>>,
    lan_last_ok: Mutex<Option<Instant>>,
}

impl Esp32Runtime {
    pub fn new(app: Option<AppHandle>) -> Arc<Self> {
        let initial_keys = load_keys_from_vault();
        Arc::new(Self {
            app,
            keys: Mutex::new(initial_keys),
            pair_code: Mutex::new(None),
            last_error: Mutex::new(None),
            running: AtomicBool::new(false),
            usage_running: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            claude_quota: Mutex::new(ClaudeQuotaCache::default()),
            agent_panel: Mutex::new(Esp32AgentPanelState::default()),
            shutdown_tx: Mutex::new(None),
            relay_last_ok: Mutex::new(None),
            lan_last_ok: Mutex::new(None),
        })
    }

    pub fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn arm_pair_code(&self, code: &str) -> Result<(), String> {
        let trimmed = code.trim();
        if trimmed.len() != 6 || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
            return Err("O código de pareamento deve ter exatamente 6 dígitos numéricos.".into());
        }
        let mut guard = self.pair_code.lock().unwrap();
        *guard = Some(ArmedPairCode {
            code: trimmed.to_string(),
            expires_at: Instant::now() + PAIR_CODE_TTL,
        });
        Ok(())
    }

    pub fn pair_with_code(&self, code: &str, device_id: Option<&str>) -> Result<Option<String>, String> {
        let mut guard = self.pair_code.lock().unwrap();
        let Some(armed) = guard.as_ref() else {
            return Ok(None);
        };
        if Instant::now() > armed.expires_at {
            *guard = None;
            return Ok(None);
        }
        let matched = constant_time_str_eq(code.trim(), &armed.code);
        if !matched {
            return Ok(None);
        }
        // Uso único: limpa imediatamente após acerto.
        *guard = None;
        drop(guard);

        let key = crypto::generate_bridge_key_hex()?;
        let dev = device_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("esp32-s3")
            .chars()
            .take(48)
            .collect::<String>();
        let entry = PairedDeviceKey {
            device_id: dev,
            key: key.clone(),
            created_at: Self::now_secs(),
        };
        let mut keys = self.keys.lock().unwrap();
        keys.push(entry);
        save_keys_to_vault(&keys)?;
        Ok(Some(key))
    }

    pub fn revoke_all(&self) -> Result<(), String> {
        {
            let mut keys = self.keys.lock().unwrap();
            keys.clear();
            save_keys_to_vault(&keys)?;
        }
        *self.pair_code.lock().unwrap() = None;
        Ok(())
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
        let keys: Vec<String> = self
            .keys
            .lock()
            .unwrap()
            .iter()
            .map(|k| k.key.clone())
            .collect();
        // Percorre todas as chaves sem curto-circuito para evitar vazamento por tempo.
        let mut matched = false;
        for stored in &keys {
            if constant_time_str_eq(token, stored) {
                matched = true;
            }
        }
        matched
    }

    pub fn has_paired_keys(&self) -> bool {
        !self.keys.lock().unwrap().is_empty()
    }

    pub fn key_strings(&self) -> Vec<String> {
        self.keys
            .lock()
            .unwrap()
            .iter()
            .map(|k| k.key.clone())
            .collect()
    }

    pub fn note_lan_activity(&self) {
        *self.lan_last_ok.lock().unwrap() = Some(Instant::now());
    }

    #[allow(dead_code)]
    pub fn note_relay_activity(&self) {
        *self.relay_last_ok.lock().unwrap() = Some(Instant::now());
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
        if self
            .relay_last_ok
            .lock()
            .unwrap()
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(90))
        {
            return "relay";
        }
        "alone"
    }

    pub fn stop_listeners(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(true);
        }
        self.running.store(false, Ordering::SeqCst);
        self.usage_running.store(false, Ordering::SeqCst);
    }

    pub fn status(&self, cfg: &settings::Settings) -> Esp32BridgeStatus {
        let mut pair_armed = false;
        let mut pair_secs = None;
        {
            let mut guard = self.pair_code.lock().unwrap();
            if let Some(armed) = guard.as_ref() {
                if let Some(rem) = armed.expires_at.checked_duration_since(Instant::now()) {
                    pair_armed = true;
                    pair_secs = Some(rem.as_secs().max(1));
                } else {
                    *guard = None;
                }
            }
        }
        let paired_count = self.keys.lock().unwrap().len();
        let relay_room_configured = secrets::get_internal(SECRET_RELAY_ROOM)
            .is_some_and(|r| crypto::valid_room(&r));
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
            paired_count,
            pair_code_armed: pair_armed,
            pair_code_expires_in_secs: pair_secs,
            relay_enabled: cfg.esp32_relay_enabled,
            relay_url: cfg.esp32_relay_url.clone(),
            relay_room_configured,
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

fn load_keys_from_vault() -> Vec<PairedDeviceKey> {
    let Some(raw) = secrets::get_internal(SECRET_BRIDGE_KEYS) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<PairedDeviceKey>>(&raw).unwrap_or_default()
}

fn save_keys_to_vault(keys: &[PairedDeviceKey]) -> Result<(), String> {
    if keys.is_empty() {
        return secrets::clear_internal(SECRET_BRIDGE_KEYS);
    }
    let json = serde_json::to_string(keys).map_err(|e| e.to_string())?;
    secrets::set_internal(SECRET_BRIDGE_KEYS, &json)
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
    if !cfg.esp32_bridge_enabled {
        return;
    }

    let port = normalize_port(cfg.esp32_bridge_port, DEFAULT_BRIDGE_PORT);
    let usage_port = normalize_port(cfg.esp32_usage_port, DEFAULT_USAGE_PORT);
    let panel_enabled = cfg.esp32_agent_panel_enabled;
    let (tx, rx) = watch::channel(false);
    *rt.shutdown_tx.lock().unwrap() = Some(tx);

    let rt_main = rt.clone();
    tauri::async_runtime::spawn(async move {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                rt_main.running.store(true, Ordering::SeqCst);
                crate::log::line(format!("esp32_bridge: ouvindo em :{port}"));
                server::serve_listener(listener, rt_main.clone(), rx.clone()).await;
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
        let rx_usage = rt.shutdown_tx.lock().unwrap().as_ref().map(|s| s.subscribe());
        if let Some(rx_usage) = rx_usage {
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
pub fn esp32_arm_pair_code(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
    code: String,
) -> Result<Esp32BridgeStatus, String> {
    rt.arm_pair_code(&code)?;
    let cfg = current_settings(&app);
    Ok(rt.status(&cfg))
}

#[tauri::command]
pub fn esp32_revoke_all(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
) -> Result<Esp32BridgeStatus, String> {
    rt.revoke_all()?;
    let cfg = current_settings(&app);
    Ok(rt.status(&cfg))
}

#[tauri::command]
pub fn esp32_save_relay(
    app: AppHandle,
    rt: State<'_, Arc<Esp32Runtime>>,
    enabled: bool,
    url: String,
    room: String,
    key: String,
) -> Result<Esp32BridgeStatus, String> {
    let trimmed_url = url.trim().trim_end_matches('/').to_string();
    let trimmed_room = room.trim().to_string();
    let trimmed_key = key.trim().to_string();

    if enabled {
        let valid_url = trimmed_url.starts_with("https://")
            || trimmed_url.starts_with("http://127.0.0.1")
            || trimmed_url.starts_with("http://localhost");
        if !valid_url {
            return Err("URL do Relay inválida (use https://...).".into());
        }
        if !trimmed_room.is_empty() && !crypto::valid_room(&trimmed_room) {
            return Err("Sala do Relay deve ter 8 a 64 caracteres [A-Za-z0-9_-].".into());
        }
    }

    if !trimmed_room.is_empty() {
        secrets::set_internal(SECRET_RELAY_ROOM, &trimmed_room)?;
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
