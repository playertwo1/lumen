// Integração Google pessoal no Lumen (L7):
// - L7.1: Login OAuth 2.0 PKCE com credenciais próprias fora do repositório (D-055)
//   e tokens isolados no Windows Credential Manager.
// - L7.2: Backup semanal em `Segunda Mente/Lumen-PC` no Google Drive (D-054, D21)
//   e restauração segura validada por SHA-256 + snapshot pré-restauração.
// - L7.3: Leitura e criação explícita no Google Calendar (somente após confirmação,
//   sem convidados de terceiros e sem edição/remoção automática).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::Url;
use ring::digest::{self, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use subtle::ConstantTimeEq;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{timeout_at, Instant};

use crate::net;
use crate::secrets::{clear_internal, read_internal, set_internal};
use crate::tasks::model::{
    civil_from_days, days_from_civil, effective_offset_minutes, format_date, is_valid_date,
    is_valid_time, now_utc_ms, parse_date, parse_time, TodoItem, TODO_TIME_ZONE,
};
use crate::tasks::store::write_atomic;
use crate::whatsapp::gateway::iso_utc;

pub const DRIVE_ROOT_FOLDER: &str = "Segunda Mente";
pub const DRIVE_DEVICE_FOLDER: &str = "Lumen-PC";
pub const DRIVE_FOLDER_DISPLAY: &str = "Segunda Mente/Lumen-PC";
pub const DRIVE_BACKUP_FILENAME: &str = "lumen-backup-latest.json";
pub const BACKUP_SCHEMA_VERSION: u32 = 1;

const KEY_CLIENT_ID: &str = "google-oauth-client-id";
const KEY_CLIENT_SECRET: &str = "google-oauth-client-secret";
const KEY_ACCESS_TOKEN: &str = "google-oauth-access-token";
const KEY_REFRESH_TOKEN: &str = "google-oauth-refresh-token";
const KEY_ACCOUNT_META: &str = "google-oauth-account-meta";
const KEY_BACKUP_META: &str = "google-drive-backup-meta";

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const CALENDAR_EVENTS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

const OAUTH_SCOPES: &str = "openid email profile https://www.googleapis.com/auth/drive.file https://www.googleapis.com/auth/calendar.events";
const CALLBACK_PATH: &str = "/oauth2callback";
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HTTP_REQUEST: usize = 16 * 1024;
const MAX_OAUTH_RESPONSE: usize = 64 * 1024;
const MAX_BACKUP_BYTES: usize = 16 * 1024 * 1024;
const TOKEN_REFRESH_SKEW_MS: i64 = 60_000;
const WEEKLY_INTERVAL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const SCHEDULER_POLL_SECS: u64 = 60 * 60;

static REFRESH_LOCK: LazyLock<AsyncMutex<()>> = LazyLock::new(|| AsyncMutex::new(()));
static BACKUP_LOCK: LazyLock<AsyncMutex<()>> = LazyLock::new(|| AsyncMutex::new(()));

trait CredentialStore {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn clear(&self, key: &str) -> Result<(), String>;
}

struct SystemCredentialStore;

impl CredentialStore for SystemCredentialStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        read_internal(key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        set_internal(key, value)
    }

    fn clear(&self, key: &str) -> Result<(), String> {
        clear_internal(key)
    }
}

#[derive(Debug, Clone)]
struct GoogleEndpoints {
    authorize_url: String,
    token_url: String,
    revoke_url: String,
    userinfo_url: String,
    drive_files_url: String,
    drive_upload_url: String,
    calendar_events_url: String,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            authorize_url: AUTHORIZE_URL.into(),
            token_url: TOKEN_URL.into(),
            revoke_url: REVOKE_URL.into(),
            userinfo_url: USERINFO_URL.into(),
            drive_files_url: DRIVE_FILES_URL.into(),
            drive_upload_url: DRIVE_UPLOAD_URL.into(),
            calendar_events_url: CALENDAR_EVENTS_URL.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ClientConfig {
    client_id: String,
    client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GoogleAccountMeta {
    email: Option<String>,
    display_name: Option<String>,
    subject: Option<String>,
    expires_at_ms: i64,
    #[serde(default)]
    scopes: Vec<String>,
    connected_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GoogleBackupMeta {
    weekly_enabled: bool,
    last_backup_at_ms: Option<i64>,
    last_backup_file_id: Option<String>,
    last_backup_sha256: Option<String>,
    last_restore_at_ms: Option<i64>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleStatus {
    pub configured: bool,
    pub client_id_preview: Option<String>,
    pub has_client_secret: bool,
    pub connected: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub scopes: Vec<String>,
    pub drive_folder: String,
    pub weekly_backup_enabled: bool,
    pub last_backup_at: Option<String>,
    pub last_backup_at_ms: Option<i64>,
    pub last_restore_at: Option<String>,
    pub last_backup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleBackupStatus {
    pub drive_folder: String,
    pub file_id: String,
    pub created_at: String,
    pub created_at_ms: i64,
    pub sha256: String,
    pub notes_count: usize,
    pub tasks_included: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleRestoreSummary {
    pub backup_created_at: String,
    pub sha256: String,
    pub tasks_restored: bool,
    pub notes_restored: usize,
    pub personas_restored: bool,
    pub settings_restored: bool,
    pub safety_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCalendarEvent {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub html_link: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCreateEventInput {
    pub task_id: String,
    pub confirmed: bool,
    pub override_date: Option<String>,
    pub override_time: Option<String>,
    pub duration_minutes: Option<i32>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BackupNoteEntry {
    id: String,
    note: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BackupPayload {
    tasks_todo: Option<Value>,
    tasks_shortcuts: Option<Value>,
    tasks_sync: Option<Value>,
    #[serde(default)]
    notes: Vec<BackupNoteEntry>,
    personas: Option<Value>,
    persona_comment_settings: Option<Value>,
    app_settings: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LumenBackupPackage {
    schema_version: u32,
    app: String,
    device: String,
    created_at_ms: i64,
    created_at: String,
    payload: BackupPayload,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CalendarDateTimePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    date_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
}

/// Payload enviado ao Google Calendar ao confirmar uma tarefa.
/// Propositalmente NÃO inclui campo `attendees`: o Lumen nunca envia convites a terceiros.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct CalendarCreateEventPayload {
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    start: CalendarDateTimePayload,
    end: CalendarDateTimePayload,
}

pub struct GoogleRuntime {
    stop_flag: Arc<AtomicBool>,
    wake_tx: Sender<()>,
}

pub fn setup(app: &AppHandle) {
    let (wake_tx, wake_rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    app.manage(GoogleRuntime {
        stop_flag: stop_flag.clone(),
        wake_tx,
    });
    let app_handle = app.clone();
    std::thread::Builder::new()
        .name("lumen-google-backup".into())
        .spawn(move || weekly_backup_loop(app_handle, wake_rx, stop_flag))
        .ok();
}

pub fn shutdown(app: &AppHandle) {
    if let Some(rt) = app.try_state::<GoogleRuntime>() {
        rt.stop_flag.store(true, Ordering::Relaxed);
        let _ = rt.wake_tx.send(());
    }
}

fn wake_scheduler(app: &AppHandle) {
    if let Some(rt) = app.try_state::<GoogleRuntime>() {
        let _ = rt.wake_tx.send(());
    }
}

fn weekly_backup_loop(
    app: AppHandle,
    wake_rx: mpsc::Receiver<()>,
    stop_flag: Arc<AtomicBool>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return,
    };
    while !stop_flag.load(Ordering::Relaxed) {
        if should_run_weekly_backup(&SystemCredentialStore, now_utc_ms()) {
            let outcome = rt.block_on(async {
                run_backup_now(
                    &SystemCredentialStore,
                    &GoogleEndpoints::default(),
                    &crate::tasks::folder(),
                    &crate::notes::folder(),
                    &crate::notes::personas::store_dir(),
                    &crate::settings::config_dir(),
                    now_utc_ms(),
                )
                .await
            });
            if let Ok(summary) = outcome {
                emit_backup_completed(&app, &summary);
            }
        }
        match wake_rx.recv_timeout(Duration::from_secs(SCHEDULER_POLL_SECS)) {
            Ok(()) => {
                while wake_rx.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn should_run_weekly_backup(store: &impl CredentialStore, now_ms: i64) -> bool {
    if resolve_client_config(store).is_none() {
        return false;
    }
    if read_account_meta(store).is_none() {
        return false;
    }
    let has_refresh = store
        .get(KEY_REFRESH_TOKEN)
        .ok()
        .flatten()
        .is_some_and(|v| !v.is_empty());
    let has_access = store
        .get(KEY_ACCESS_TOKEN)
        .ok()
        .flatten()
        .is_some_and(|v| !v.is_empty());
    if !has_refresh && !has_access {
        return false;
    }
    let meta = read_backup_meta(store);
    if !meta.weekly_enabled {
        return false;
    }
    match meta.last_backup_at_ms {
        None => true,
        Some(last_ms) => now_ms.saturating_sub(last_ms) >= WEEKLY_INTERVAL_MS,
    }
}

fn emit_backup_completed(app: &AppHandle, summary: &GoogleBackupStatus) {
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "google-backup-completed", summary);
    let _ = app.emit_to(crate::HUB_LABEL, "google-backup-completed", summary);
}

#[tauri::command]
pub fn google_status() -> Result<GoogleStatus, String> {
    build_status(&SystemCredentialStore)
}

#[tauri::command]
pub fn google_save_client_config(
    raw_input: String,
    client_secret: Option<String>,
) -> Result<GoogleStatus, String> {
    let parsed = parse_client_config_input(&raw_input, client_secret.as_deref())?;
    save_client_config(&SystemCredentialStore, &parsed)?;
    build_status(&SystemCredentialStore)
}

#[tauri::command]
pub fn google_clear_client_config() -> Result<GoogleStatus, String> {
    clear_auth_secrets(&SystemCredentialStore)?;
    SystemCredentialStore.clear(KEY_CLIENT_ID)?;
    SystemCredentialStore.clear(KEY_CLIENT_SECRET)?;
    build_status(&SystemCredentialStore)
}

#[tauri::command]
pub async fn google_sign_in(app: AppHandle) -> Result<GoogleStatus, String> {
    let status = sign_in_with(
        &SystemCredentialStore,
        &GoogleEndpoints::default(),
        open_browser_url,
    )
    .await?;
    wake_scheduler(&app);
    Ok(status)
}

#[tauri::command]
pub async fn google_sign_out() -> Result<GoogleStatus, String> {
    sign_out_with(&SystemCredentialStore, &GoogleEndpoints::default()).await
}

#[tauri::command]
pub fn google_set_weekly_backup(app: AppHandle, enabled: bool) -> Result<GoogleStatus, String> {
    set_weekly_backup_pref(&SystemCredentialStore, enabled)?;
    if enabled {
        wake_scheduler(&app);
    }
    build_status(&SystemCredentialStore)
}

#[tauri::command]
pub async fn google_backup_now(app: AppHandle) -> Result<GoogleBackupStatus, String> {
    let summary = run_backup_now(
        &SystemCredentialStore,
        &GoogleEndpoints::default(),
        &crate::tasks::folder(),
        &crate::notes::folder(),
        &crate::notes::personas::store_dir(),
        &crate::settings::config_dir(),
        now_utc_ms(),
    )
    .await?;
    emit_backup_completed(&app, &summary);
    Ok(summary)
}

#[tauri::command]
pub async fn google_restore_backup(app: AppHandle) -> Result<GoogleRestoreSummary, String> {
    crate::notes::ensure_idle_for_restore(&app)?;
    let summary = restore_backup_at(
        &SystemCredentialStore,
        &GoogleEndpoints::default(),
        &crate::tasks::folder(),
        &crate::notes::folder(),
        &crate::notes::personas::store_dir(),
        &crate::settings::config_dir(),
        now_utc_ms(),
    )
    .await?;
    crate::tasks::reload_from_disk(&app);
    crate::notes::reload_from_disk(&app)?;
    Ok(summary)
}

#[tauri::command]
pub async fn google_calendar_list_events(
    time_min: Option<String>,
    time_max: Option<String>,
    max_results: Option<u8>,
) -> Result<Vec<GoogleCalendarEvent>, String> {
    list_calendar_events_with(
        &SystemCredentialStore,
        &GoogleEndpoints::default(),
        time_min.as_deref(),
        time_max.as_deref(),
        max_results,
        now_utc_ms(),
    )
    .await
}

#[tauri::command]
pub async fn google_calendar_create_from_task(
    app: AppHandle,
    input: GoogleCreateEventInput,
) -> Result<GoogleCalendarEvent, String> {
    if !input.confirmed {
        return Err("A criação de evento no Google Calendar exige confirmação explícita.".into());
    }
    let task = crate::tasks::get_item(&app, &input.task_id)
        .ok_or_else(|| "Tarefa não encontrada.".to_string())?;
    create_event_from_task_with(
        &SystemCredentialStore,
        &GoogleEndpoints::default(),
        &task,
        &input,
    )
    .await
}

fn open_browser_url(url: &str) -> Result<(), String> {
    crate::platform::open_url(url);
    Ok(())
}

// ---------- L7.1: Credenciais & OAuth PKCE ----------

fn clean_opt(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn validate_client_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Informe o Client ID OAuth do seu projeto no Google Cloud.".into());
    }
    if trimmed.len() > 512 || trimmed.bytes().any(|b| !b.is_ascii_graphic()) {
        return Err("Client ID Google inválido.".into());
    }
    Ok(trimmed.to_string())
}

fn parse_client_config_input(
    raw_input: &str,
    explicit_secret: Option<&str>,
) -> Result<ClientConfig, String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err("Informe o Client ID ou cole o JSON OAuth do Google Cloud.".into());
    }
    let secret_override = clean_opt(explicit_secret);
    if trimmed.starts_with('{') {
        let root: Value = serde_json::from_str(trimmed)
            .map_err(|_| "JSON de credenciais OAuth do Google inválido.".to_string())?;
        let node = root
            .get("installed")
            .or_else(|| root.get("web"))
            .unwrap_or(&root);
        let client_id = node
            .get("client_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "O JSON não contém o campo client_id.".to_string())?;
        let client_id = validate_client_id(client_id)?;
        let json_secret = node.get("client_secret").and_then(|v| v.as_str());
        return Ok(ClientConfig {
            client_id,
            client_secret: secret_override.or_else(|| clean_opt(json_secret)),
        });
    }
    Ok(ClientConfig {
        client_id: validate_client_id(trimmed)?,
        client_secret: secret_override,
    })
}

fn save_client_config(store: &impl CredentialStore, cfg: &ClientConfig) -> Result<(), String> {
    store.set(KEY_CLIENT_ID, &cfg.client_id)?;
    match cfg.client_secret.as_deref() {
        Some(secret) if !secret.is_empty() => store.set(KEY_CLIENT_SECRET, secret)?,
        _ => store.clear(KEY_CLIENT_SECRET)?,
    }
    Ok(())
}

fn resolve_client_config(store: &impl CredentialStore) -> Option<ClientConfig> {
    if let Ok(Some(client_id)) = store.get(KEY_CLIENT_ID) {
        let trimmed = client_id.trim();
        if !trimmed.is_empty() {
            let client_secret = store
                .get(KEY_CLIENT_SECRET)
                .ok()
                .flatten()
                .and_then(|v| clean_opt(Some(&v)));
            return Some(ClientConfig {
                client_id: trimmed.to_string(),
                client_secret,
            });
        }
    }
    if let Ok(env_id) = std::env::var("LUMEN_GOOGLE_CLIENT_ID") {
        let trimmed = env_id.trim();
        if !trimmed.is_empty() {
            let client_secret = std::env::var("LUMEN_GOOGLE_CLIENT_SECRET")
                .ok()
                .and_then(|v| clean_opt(Some(&v)));
            return Some(ClientConfig {
                client_id: trimmed.to_string(),
                client_secret,
            });
        }
    }
    None
}

fn client_id_preview(client_id: &str) -> String {
    let chars: Vec<char> = client_id.chars().collect();
    if chars.len() <= 18 {
        return client_id.to_string();
    }
    let head: String = chars.iter().take(8).collect();
    let tail: String = chars.iter().skip(chars.len() - 8).collect();
    format!("{head}...{tail}")
}

fn read_account_meta(store: &impl CredentialStore) -> Option<GoogleAccountMeta> {
    let raw = store.get(KEY_ACCOUNT_META).ok().flatten()?;
    serde_json::from_str::<GoogleAccountMeta>(&raw).ok()
}

fn read_backup_meta(store: &impl CredentialStore) -> GoogleBackupMeta {
    store
        .get(KEY_BACKUP_META)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<GoogleBackupMeta>(&raw).ok())
        .unwrap_or_default()
}

fn write_backup_meta(store: &impl CredentialStore, meta: &GoogleBackupMeta) -> Result<(), String> {
    let json = serde_json::to_string(meta).map_err(|e| e.to_string())?;
    store.set(KEY_BACKUP_META, &json)
}

fn set_weekly_backup_pref(store: &impl CredentialStore, enabled: bool) -> Result<(), String> {
    let mut meta = read_backup_meta(store);
    meta.weekly_enabled = enabled;
    write_backup_meta(store, &meta)
}

fn clear_auth_secrets(store: &impl CredentialStore) -> Result<(), String> {
    store.clear(KEY_ACCESS_TOKEN)?;
    store.clear(KEY_REFRESH_TOKEN)?;
    store.clear(KEY_ACCOUNT_META)?;
    Ok(())
}

fn build_status(store: &impl CredentialStore) -> Result<GoogleStatus, String> {
    let cfg = resolve_client_config(store);
    let meta = read_account_meta(store);
    let has_token = store
        .get(KEY_ACCESS_TOKEN)?
        .is_some_and(|v| !v.is_empty())
        || store
            .get(KEY_REFRESH_TOKEN)?
            .is_some_and(|v| !v.is_empty());
    let connected = cfg.is_some() && meta.is_some() && has_token;
    let backup = read_backup_meta(store);

    Ok(GoogleStatus {
        configured: cfg.is_some(),
        client_id_preview: cfg.as_ref().map(|c| client_id_preview(&c.client_id)),
        has_client_secret: cfg.as_ref().is_some_and(|c| c.client_secret.is_some()),
        connected,
        email: if connected {
            meta.as_ref().and_then(|m| m.email.clone())
        } else {
            None
        },
        display_name: if connected {
            meta.as_ref().and_then(|m| m.display_name.clone())
        } else {
            None
        },
        scopes: if connected {
            meta.map(|m| m.scopes).unwrap_or_default()
        } else {
            Vec::new()
        },
        drive_folder: DRIVE_FOLDER_DISPLAY.into(),
        weekly_backup_enabled: backup.weekly_enabled,
        last_backup_at: backup.last_backup_at_ms.map(iso_utc),
        last_backup_at_ms: backup.last_backup_at_ms,
        last_restore_at: backup.last_restore_at_ms.map(iso_utc),
        last_backup_error: backup.last_error,
    })
}

fn random_base64url(len_bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; len_bytes];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| "Não foi possível gerar entropy segura para o login Google.".to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(&buf))
}

fn pkce_challenge(verifier: &str) -> String {
    let hash = digest::digest(&SHA256, verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash.as_ref())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let hash = digest::digest(&SHA256, bytes);
    hash.as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

fn build_authorize_url(
    endpoints: &GoogleEndpoints,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<String, String> {
    let mut url = Url::parse(&endpoints.authorize_url)
        .map_err(|_| "URL de autorização Google inválida.".to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", OAUTH_SCOPES)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent select_account")
            .append_pair("include_granted_scopes", "true");
    }
    Ok(url.into())
}

async fn read_callback_code(
    listener: TcpListener,
    expected_state: String,
    deadline: Instant,
) -> Result<String, String> {
    loop {
        let accept = timeout_at(deadline, listener.accept())
            .await
            .map_err(|_| "Tempo esgotado aguardando confirmação do login Google no navegador.".to_string())?;
        let (mut stream, _) =
            accept.map_err(|_| "Falha ao receber retorno local do login Google.".to_string())?;
        let Some(target) = read_http_request_target(&mut stream).await else {
            let _ = write_http_html(
                &mut stream,
                "400 Bad Request",
                "Requisição inválida no retorno OAuth.",
            )
            .await;
            continue;
        };
        let path_only = target.split('?').next().unwrap_or("");
        if path_only != CALLBACK_PATH {
            let _ = write_http_html(&mut stream, "404 Not Found", "Não encontrado.").await;
            continue;
        }
        let parsed = Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|_| "Retorno OAuth do Google malformado.".to_string())?;
        let params: HashMap<String, String> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        if let Some(err) = params.get("error") {
            let desc = params
                .get("error_description")
                .map(String::as_str)
                .unwrap_or("autorização cancelada");
            let _ = write_http_html(
                &mut stream,
                "400 Bad Request",
                "Autorização Google não concluída. Você pode fechar esta aba e voltar ao Lumen.",
            )
            .await;
            return Err(format!("Login Google recusado ({err}: {desc})."));
        }

        let got_state = params.get("state").map(String::as_str).unwrap_or("");
        let state_ok = got_state.len() == expected_state.len()
            && got_state.as_bytes().ct_eq(expected_state.as_bytes()).unwrap_u8() == 1;
        if !state_ok {
            let _ = write_http_html(
                &mut stream,
                "400 Bad Request",
                "Estado OAuth inválido. Tente iniciar o login novamente pelo Lumen.",
            )
            .await;
            return Err("Parâmetro state OAuth inválido na resposta do Google.".into());
        }

        let code = params
            .get("code")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "O Google não retornou o código de autorização.".to_string())?;

        let _ = write_http_html(
            &mut stream,
            "200 OK",
            "Conta Google conectada ao Lumen. Você pode fechar esta aba e voltar ao aplicativo.",
        )
        .await;
        return Ok(code);
    }
}

async fn read_http_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        if buf.len() + n > MAX_HTTP_REQUEST {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let first_line = text.lines().next()?;
    let mut parts = first_line.split_ascii_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    let target = parts.next()?;
    if !target.starts_with('/') {
        return None;
    }
    Some(target.to_string())
}

async fn write_http_html(stream: &mut TcpStream, status: &str, message: &str) -> Result<(), String> {
    let escaped = html_escape(message);
    let body = format!(
        "<!DOCTYPE html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><title>Lumen — Google</title><style>body{{font-family:system-ui,sans-serif;background:#111214;color:#F3F4F6;display:grid;place-items:center;min-height:100vh;margin:0}}main{{max-width:440px;padding:28px;border:1px solid #2A2C32;border-radius:14px;background:#181A1F;text-align:center}}h1{{font-size:18px;margin:0 0 10px}}p{{font-size:14px;color:#A1A1AA;line-height:1.5;margin:0}}</style></head><body><main><h1>Lumen</h1><p>{escaped}</p></main></body></html>"
    );
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
    Ok(())
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    id_token: Option<String>,
    scope: Option<String>,
}

#[derive(Deserialize, Default)]
struct UserInfoResponse {
    sub: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

fn decode_jwt_claims_unverified(jwt: &str) -> Option<UserInfoResponse> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice::<UserInfoResponse>(&decoded).ok()
}

async fn exchange_code_for_tokens(
    endpoints: &GoogleEndpoints,
    cfg: &ClientConfig,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokenResponse, String> {
    let token_url = Url::parse(&endpoints.token_url)
        .map_err(|_| "URL de token Google inválida.".to_string())?;
    let http = net::client(&token_url, REQUEST_TIMEOUT)?;
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", &cfg.client_id),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
    ];
    if let Some(secret) = cfg.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let resp = http
        .post(token_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Falha ao trocar código OAuth com o Google: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, MAX_OAUTH_RESPONSE).await?;
    if !status.is_success() {
        let detail = net::error_detail(&body);
        return Err(format!("Google rejeitou a troca de código OAuth ({status}): {detail}"));
    }
    serde_json::from_slice::<OAuthTokenResponse>(&body)
        .map_err(|_| "Resposta de token OAuth do Google inválida.".to_string())
}

async fn fetch_userinfo(
    endpoints: &GoogleEndpoints,
    access_token: &str,
) -> Option<UserInfoResponse> {
    let url = Url::parse(&endpoints.userinfo_url).ok()?;
    let http = net::client(&url, REQUEST_TIMEOUT).ok()?;
    let resp = http.get(url).bearer_auth(access_token).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = net::read_capped(resp, MAX_OAUTH_RESPONSE).await.ok()?;
    serde_json::from_slice::<UserInfoResponse>(&body).ok()
}

async fn sign_in_with<F>(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
    open_url: F,
) -> Result<GoogleStatus, String>
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let cfg = resolve_client_config(store).ok_or_else(|| {
        "Configure primeiro o Client ID OAuth do seu projeto no Google Cloud (D-055).".to_string()
    })?;
    let state = random_base64url(24)?;
    let verifier = random_base64url(48)?;
    let challenge = pkce_challenge(&verifier);

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|_| "Não foi possível abrir o retorno local do login Google.".to_string())?;
    let port = listener
        .local_addr()
        .map_err(|_| "Não foi possível obter a porta local do retorno OAuth.".to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let auth_url =
        build_authorize_url(endpoints, &cfg.client_id, &redirect_uri, &state, &challenge)?;

    open_url(&auth_url)?;

    let code = read_callback_code(listener, state, Instant::now() + AUTH_TIMEOUT).await?;
    let tokens =
        exchange_code_for_tokens(endpoints, &cfg, &code, &verifier, &redirect_uri).await?;
    if tokens.access_token.trim().is_empty() {
        return Err("O Google retornou access_token vazio.".into());
    }

    let now_ms = now_utc_ms();
    let expires_in_ms = tokens.expires_in.unwrap_or(3600).max(60) * 1000;
    let expires_at_ms = now_ms.saturating_add(expires_in_ms);

    let jwt_claims = tokens
        .id_token
        .as_deref()
        .and_then(decode_jwt_claims_unverified);
    let user_info = fetch_userinfo(endpoints, &tokens.access_token).await;

    let email = user_info
        .as_ref()
        .and_then(|u| u.email.clone())
        .or_else(|| jwt_claims.as_ref().and_then(|c| c.email.clone()));
    let display_name = user_info
        .as_ref()
        .and_then(|u| u.name.clone())
        .or_else(|| jwt_claims.as_ref().and_then(|c| c.name.clone()));
    let subject = user_info
        .as_ref()
        .and_then(|u| u.sub.clone())
        .or_else(|| jwt_claims.as_ref().and_then(|c| c.sub.clone()));
    let scopes: Vec<String> = tokens
        .scope
        .as_deref()
        .unwrap_or(OAUTH_SCOPES)
        .split_ascii_whitespace()
        .map(ToOwned::to_owned)
        .collect();

    store.set(KEY_ACCESS_TOKEN, &tokens.access_token)?;
    if let Some(refresh) = tokens.refresh_token.as_deref().filter(|s| !s.is_empty()) {
        store.set(KEY_REFRESH_TOKEN, refresh)?;
    }
    let meta = GoogleAccountMeta {
        email,
        display_name,
        subject,
        expires_at_ms,
        scopes,
        connected_at_ms: now_ms,
    };
    let meta_json = serde_json::to_string(&meta).map_err(|e| e.to_string())?;
    store.set(KEY_ACCOUNT_META, &meta_json)?;

    build_status(store)
}

async fn sign_out_with(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
) -> Result<GoogleStatus, String> {
    let token = store
        .get(KEY_REFRESH_TOKEN)?
        .filter(|v| !v.is_empty())
        .or_else(|| store.get(KEY_ACCESS_TOKEN).ok().flatten());
    if let Some(tok) = token {
        if let Ok(url) = Url::parse(&endpoints.revoke_url) {
            if let Ok(http) = net::client(&url, Duration::from_secs(5)) {
                let _ = http.post(url).form(&[("token", tok.as_str())]).send().await;
            }
        }
    }
    clear_auth_secrets(store)?;
    build_status(store)
}

async fn ensure_valid_access_token(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
) -> Result<String, String> {
    let _guard = REFRESH_LOCK.lock().await;
    let cfg = resolve_client_config(store)
        .ok_or_else(|| "Conta Google não configurada. Informe seu Client ID primeiro.".to_string())?;
    let mut meta = read_account_meta(store)
        .ok_or_else(|| "Conta Google desconectada. Faça login novamente.".to_string())?;
    let current_access = store.get(KEY_ACCESS_TOKEN)?.filter(|s| !s.is_empty());
    let now_ms = now_utc_ms();

    if let Some(access) = current_access {
        if meta.expires_at_ms.saturating_sub(now_ms) > TOKEN_REFRESH_SKEW_MS {
            return Ok(access);
        }
    }

    let refresh_token = store
        .get(KEY_REFRESH_TOKEN)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Sessão Google expirada sem refresh token. Entre novamente.".to_string())?;

    let token_url = Url::parse(&endpoints.token_url)
        .map_err(|_| "URL de token Google inválida.".to_string())?;
    let http = net::client(&token_url, REQUEST_TIMEOUT)?;
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh_token),
        ("client_id", &cfg.client_id),
    ];
    if let Some(secret) = cfg.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }

    let resp = http
        .post(token_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Falha ao renovar acesso Google: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, MAX_OAUTH_RESPONSE).await?;
    if !status.is_success() {
        let detail = net::error_detail(&body);
        if status.as_u16() == 400 || status.as_u16() == 401 {
            let _ = store.clear(KEY_ACCESS_TOKEN);
            return Err(format!(
                "Sua sessão Google expirou ou foi revogada ({detail}). Faça login novamente."
            ));
        }
        return Err(format!("Erro ao renovar sessão Google ({status}): {detail}"));
    }

    let refreshed: OAuthTokenResponse = serde_json::from_slice(&body)
        .map_err(|_| "Resposta de renovação de token Google inválida.".to_string())?;
    if refreshed.access_token.trim().is_empty() {
        return Err("O Google retornou access_token vazio ao renovar.".into());
    }
    store.set(KEY_ACCESS_TOKEN, &refreshed.access_token)?;
    if let Some(new_refresh) = refreshed
        .refresh_token
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        store.set(KEY_REFRESH_TOKEN, new_refresh)?;
    }
    meta.expires_at_ms =
        now_utc_ms().saturating_add(refreshed.expires_in.unwrap_or(3600).max(60) * 1000);
    if let Ok(meta_json) = serde_json::to_string(&meta) {
        let _ = store.set(KEY_ACCOUNT_META, &meta_json);
    }
    Ok(refreshed.access_token)
}

// ---------- L7.2: Empacotamento, Google Drive e Restauração ----------

fn read_json_value(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let clean = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    if clean.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    serde_json::from_slice::<Value>(clean).ok()
}

fn is_safe_entry_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn build_backup_package_at(
    tasks_dir: &Path,
    notes_dir: &Path,
    personas_dir: &Path,
    config_dir: &Path,
    now_ms: i64,
) -> Result<LumenBackupPackage, String> {
    let mut notes = Vec::new();
    if let Ok(entries) = fs::read_dir(notes_dir) {
        let mut dirs: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().into_owned();
                if is_safe_entry_id(&id) {
                    Some((id, e.path()))
                } else {
                    None
                }
            })
            .collect();
        dirs.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, path) in dirs {
            if let Some(note_json) = read_json_value(&path.join("note.json")) {
                notes.push(BackupNoteEntry { id, note: note_json });
            }
        }
    }

    let payload = BackupPayload {
        tasks_todo: read_json_value(&tasks_dir.join("todo.json")),
        tasks_shortcuts: read_json_value(&tasks_dir.join("shortcut_settings.json")),
        tasks_sync: read_json_value(&tasks_dir.join("sync.json")),
        notes,
        personas: read_json_value(&personas_dir.join("personas.json")),
        persona_comment_settings: read_json_value(&personas_dir.join("comment_settings.json")),
        app_settings: read_json_value(&config_dir.join("settings.json")),
    };
    let canonical = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let sha256 = sha256_hex(&canonical);

    Ok(LumenBackupPackage {
        schema_version: BACKUP_SCHEMA_VERSION,
        app: DRIVE_DEVICE_FOLDER.into(),
        device: DRIVE_DEVICE_FOLDER.into(),
        created_at_ms: now_ms,
        created_at: iso_utc(now_ms),
        payload,
        sha256,
    })
}

fn escape_drive_query_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[derive(Deserialize)]
struct DriveFilesListResponse {
    #[serde(default)]
    files: Vec<DriveFileEntry>,
}

#[derive(Deserialize)]
struct DriveFileEntry {
    id: String,
    #[allow(dead_code)]
    name: Option<String>,
}

async fn drive_find_item(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    token: &str,
    name: &str,
    parent_id: Option<&str>,
    is_folder: bool,
) -> Result<Option<String>, String> {
    let mut url = Url::parse(&endpoints.drive_files_url)
        .map_err(|_| "URL da API do Google Drive inválida.".to_string())?;
    let escaped_name = escape_drive_query_literal(name);
    let mime_clause = if is_folder {
        "mimeType = 'application/vnd.google-apps.folder'"
    } else {
        "mimeType != 'application/vnd.google-apps.folder'"
    };
    let parent_clause = match parent_id {
        Some(pid) => format!("'{}' in parents", escape_drive_query_literal(pid)),
        None => "'root' in parents".to_string(),
    };
    let q = format!("name = '{escaped_name}' and {mime_clause} and {parent_clause} and trashed = false");
    url.query_pairs_mut()
        .append_pair("q", &q)
        .append_pair("spaces", "drive")
        .append_pair("fields", "files(id,name)")
        .append_pair("pageSize", "10");

    let resp = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Falha ao consultar o Google Drive: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, net::MAX_BODY).await?;
    if !status.is_success() {
        return Err(format!(
            "Erro ao consultar Google Drive ({status}): {}",
            net::error_detail(&body)
        ));
    }
    let list: DriveFilesListResponse = serde_json::from_slice(&body)
        .map_err(|_| "Resposta inválida ao listar arquivos no Google Drive.".to_string())?;
    Ok(list.files.into_iter().next().map(|f| f.id))
}

async fn drive_create_folder(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    token: &str,
    name: &str,
    parent_id: Option<&str>,
) -> Result<String, String> {
    let mut url = Url::parse(&endpoints.drive_files_url)
        .map_err(|_| "URL da API do Google Drive inválida.".to_string())?;
    url.query_pairs_mut().append_pair("fields", "id");

    let mut meta = Map::new();
    meta.insert("name".into(), Value::String(name.into()));
    meta.insert(
        "mimeType".into(),
        Value::String("application/vnd.google-apps.folder".into()),
    );
    if let Some(pid) = parent_id {
        meta.insert("parents".into(), Value::Array(vec![Value::String(pid.into())]));
    }

    let resp = http
        .post(url)
        .bearer_auth(token)
        .json(&meta)
        .send()
        .await
        .map_err(|e| format!("Falha ao criar pasta no Google Drive: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, net::MAX_BODY).await?;
    if !status.is_success() {
        return Err(format!(
            "Erro ao criar pasta '{name}' no Google Drive ({status}): {}",
            net::error_detail(&body)
        ));
    }
    let created: DriveFileEntry = serde_json::from_slice(&body)
        .map_err(|_| "Resposta inválida ao criar pasta no Google Drive.".to_string())?;
    Ok(created.id)
}

async fn ensure_device_folder(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    token: &str,
    create_if_missing: bool,
) -> Result<String, String> {
    let root_id = match drive_find_item(http, endpoints, token, DRIVE_ROOT_FOLDER, None, true).await?
    {
        Some(id) => id,
        None if create_if_missing => {
            drive_create_folder(http, endpoints, token, DRIVE_ROOT_FOLDER, None).await?
        }
        None => {
            return Err(format!(
                "Pasta '{DRIVE_ROOT_FOLDER}' ainda não existe no Google Drive."
            ))
        }
    };
    let device_id =
        match drive_find_item(http, endpoints, token, DRIVE_DEVICE_FOLDER, Some(&root_id), true)
            .await?
        {
            Some(id) => id,
            None if create_if_missing => {
                drive_create_folder(
                    http,
                    endpoints,
                    token,
                    DRIVE_DEVICE_FOLDER,
                    Some(&root_id),
                )
                .await?
            }
            None => {
                return Err(format!(
                    "Subpasta '{DRIVE_FOLDER_DISPLAY}' ainda não existe no Google Drive."
                ))
            }
        };
    Ok(device_id)
}

fn build_multipart_related(boundary: &str, metadata_json: &[u8], content_json: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(metadata_json.len() + content_json.len() + 256);
    out.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(metadata_json);
    out.extend_from_slice(
        format!(
            "\r\n--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(content_json);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

async fn run_backup_now(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
    tasks_dir: &Path,
    notes_dir: &Path,
    personas_dir: &Path,
    config_dir: &Path,
    now_ms: i64,
) -> Result<GoogleBackupStatus, String> {
    let _guard = BACKUP_LOCK.lock().await;
    let res = run_backup_inner(
        store,
        endpoints,
        tasks_dir,
        notes_dir,
        personas_dir,
        config_dir,
        now_ms,
    )
    .await;
    if let Err(err) = &res {
        let mut meta = read_backup_meta(store);
        meta.last_error = Some(err.clone());
        let _ = write_backup_meta(store, &meta);
    }
    res
}

async fn run_backup_inner(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
    tasks_dir: &Path,
    notes_dir: &Path,
    personas_dir: &Path,
    config_dir: &Path,
    now_ms: i64,
) -> Result<GoogleBackupStatus, String> {
    let token = ensure_valid_access_token(store, endpoints).await?;
    let drive_base = Url::parse(&endpoints.drive_files_url)
        .map_err(|_| "URL do Google Drive inválida.".to_string())?;
    let http = net::client(&drive_base, Duration::from_secs(60))?;

    let package =
        build_backup_package_at(tasks_dir, notes_dir, personas_dir, config_dir, now_ms)?;
    let package_bytes = serde_json::to_vec(&package).map_err(|e| e.to_string())?;
    let device_folder_id = ensure_device_folder(&http, endpoints, &token, true).await?;
    let existing_file_id = drive_find_item(
        &http,
        endpoints,
        &token,
        DRIVE_BACKUP_FILENAME,
        Some(&device_folder_id),
        false,
    )
    .await?;

    let mut meta = Map::new();
    meta.insert("name".into(), Value::String(DRIVE_BACKUP_FILENAME.into()));
    meta.insert("mimeType".into(), Value::String("application/json".into()));
    let (method, upload_url) = if let Some(file_id) = existing_file_id.as_deref() {
        let base = endpoints.drive_upload_url.trim_end_matches('/');
        (
            reqwest::Method::PATCH,
            Url::parse(&format!("{base}/{file_id}?uploadType=multipart&fields=id"))
                .map_err(|_| "URL de upload do Drive inválida.".to_string())?,
        )
    } else {
        meta.insert(
            "parents".into(),
            Value::Array(vec![Value::String(device_folder_id)]),
        );
        let base = endpoints.drive_upload_url.trim_end_matches('/');
        (
            reqwest::Method::POST,
            Url::parse(&format!("{base}?uploadType=multipart&fields=id"))
                .map_err(|_| "URL de upload do Drive inválida.".to_string())?,
        )
    };
    let meta_bytes = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    let boundary = format!("lumen_backup_{}", random_base64url(12)?);
    let multipart_body = build_multipart_related(&boundary, &meta_bytes, &package_bytes);

    let resp = http
        .request(method, upload_url)
        .bearer_auth(&token)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/related; boundary={boundary}"),
        )
        .body(multipart_body)
        .send()
        .await
        .map_err(|e| format!("Falha ao enviar backup ao Google Drive: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, net::MAX_BODY).await?;
    if !status.is_success() {
        return Err(format!(
            "Google Drive rejeitou o envio do backup ({status}): {}",
            net::error_detail(&body)
        ));
    }
    let uploaded: DriveFileEntry = serde_json::from_slice(&body)
        .map_err(|_| "Resposta inválida após enviar backup ao Google Drive.".to_string())?;

    let mut backup_meta = read_backup_meta(store);
    backup_meta.last_backup_at_ms = Some(now_ms);
    backup_meta.last_backup_file_id = Some(uploaded.id.clone());
    backup_meta.last_backup_sha256 = Some(package.sha256.clone());
    backup_meta.last_error = None;
    write_backup_meta(store, &backup_meta)?;

    Ok(GoogleBackupStatus {
        drive_folder: DRIVE_FOLDER_DISPLAY.into(),
        file_id: uploaded.id,
        created_at: package.created_at,
        created_at_ms: now_ms,
        sha256: package.sha256,
        notes_count: package.payload.notes.len(),
        tasks_included: package.payload.tasks_todo.is_some(),
    })
}

fn write_json_atomic(path: &Path, val: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(val).map_err(|e| e.to_string())?;
    write_atomic(path, &bytes)
}

async fn restore_backup_at(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
    tasks_dir: &Path,
    notes_dir: &Path,
    personas_dir: &Path,
    config_dir: &Path,
    now_ms: i64,
) -> Result<GoogleRestoreSummary, String> {
    let _guard = BACKUP_LOCK.lock().await;
    let token = ensure_valid_access_token(store, endpoints).await?;
    let drive_base = Url::parse(&endpoints.drive_files_url)
        .map_err(|_| "URL do Google Drive inválida.".to_string())?;
    let http = net::client(&drive_base, Duration::from_secs(60))?;

    let device_folder_id = ensure_device_folder(&http, endpoints, &token, false).await?;
    let file_id = drive_find_item(
        &http,
        endpoints,
        &token,
        DRIVE_BACKUP_FILENAME,
        Some(&device_folder_id),
        false,
    )
    .await?
    .ok_or_else(|| {
        format!("Nenhum backup encontrado em '{DRIVE_FOLDER_DISPLAY}/{DRIVE_BACKUP_FILENAME}'.")
    })?;

    let download_url = Url::parse(&format!(
        "{}/{}?alt=media",
        endpoints.drive_files_url.trim_end_matches('/'),
        file_id
    ))
    .map_err(|_| "URL de download do Drive inválida.".to_string())?;
    let resp = http
        .get(download_url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Falha ao baixar backup do Google Drive: {e}"))?;
    let status = resp.status();
    let bytes = net::read_capped(resp, MAX_BACKUP_BYTES).await?;
    if !status.is_success() {
        return Err(format!(
            "Erro ao baixar backup do Google Drive ({status}): {}",
            net::error_detail(&bytes)
        ));
    }

    let pkg: LumenBackupPackage = serde_json::from_slice(&bytes)
        .map_err(|_| "Arquivo de backup no Google Drive tem formato inválido.".to_string())?;
    if pkg.schema_version != BACKUP_SCHEMA_VERSION {
        return Err(format!(
            "Versão de backup não suportada: v{} (esperada v{}).",
            pkg.schema_version, BACKUP_SCHEMA_VERSION
        ));
    }
    let expected_sha = sha256_hex(&serde_json::to_vec(&pkg.payload).map_err(|e| e.to_string())?);
    if !expected_sha.eq_ignore_ascii_case(&pkg.sha256) {
        return Err("O backup no Google Drive falhou na verificação de integridade (SHA-256).".into());
    }
    for entry in &pkg.payload.notes {
        if !is_safe_entry_id(&entry.id) {
            return Err("Backup contém identificador de nota inválido.".into());
        }
    }

    let safety = build_backup_package_at(tasks_dir, notes_dir, personas_dir, config_dir, now_ms)?;
    let safety_bytes = serde_json::to_vec_pretty(&safety).map_err(|e| e.to_string())?;
    let safety_dir = crate::settings::local_dir().join("BackupRestoreSafety");
    let _ = write_atomic(&safety_dir.join("pre-restore.json"), &safety_bytes);

    let mut tasks_restored = false;
    if let Some(ref todo) = pkg.payload.tasks_todo {
        write_json_atomic(&tasks_dir.join("todo.json"), todo)?;
        tasks_restored = true;
    }
    if let Some(ref shortcuts) = pkg.payload.tasks_shortcuts {
        write_json_atomic(&tasks_dir.join("shortcut_settings.json"), shortcuts)?;
    }
    if let Some(ref sync) = pkg.payload.tasks_sync {
        write_json_atomic(&tasks_dir.join("sync.json"), sync)?;
    }

    let mut notes_restored = 0usize;
    for entry in &pkg.payload.notes {
        write_json_atomic(&notes_dir.join(&entry.id).join("note.json"), &entry.note)?;
        notes_restored += 1;
    }

    let mut personas_restored = false;
    if let Some(ref personas) = pkg.payload.personas {
        write_json_atomic(&personas_dir.join("personas.json"), personas)?;
        personas_restored = true;
    }
    if let Some(ref comment_cfg) = pkg.payload.persona_comment_settings {
        write_json_atomic(&personas_dir.join("comment_settings.json"), comment_cfg)?;
    }

    let mut settings_restored = false;
    if let Some(ref app_cfg) = pkg.payload.app_settings {
        if app_cfg.is_object() {
            write_json_atomic(&config_dir.join("settings.json"), app_cfg)?;
            settings_restored = true;
        }
    }

    let mut meta = read_backup_meta(store);
    meta.last_restore_at_ms = Some(now_ms);
    meta.last_error = None;
    write_backup_meta(store, &meta)?;

    Ok(GoogleRestoreSummary {
        backup_created_at: pkg.created_at,
        sha256: pkg.sha256,
        tasks_restored,
        notes_restored,
        personas_restored,
        settings_restored,
        safety_snapshot: "BackupRestoreSafety/pre-restore.json".into(),
    })
}

// ---------- L7.3: Google Calendar ----------

#[derive(Deserialize)]
struct CalendarEventDateTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(Deserialize)]
struct CalendarEventRaw {
    id: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    status: Option<String>,
    #[serde(rename = "htmlLink")]
    html_link: Option<String>,
    start: Option<CalendarEventDateTime>,
    end: Option<CalendarEventDateTime>,
}

#[derive(Deserialize)]
struct CalendarListRaw {
    #[serde(default)]
    items: Vec<CalendarEventRaw>,
}

fn map_calendar_event(raw: CalendarEventRaw) -> Option<GoogleCalendarEvent> {
    if raw.status.as_deref() == Some("cancelled") {
        return None;
    }
    let id = raw.id.filter(|s| !s.trim().is_empty())?;
    let start_obj = raw.start?;
    let (start, all_day) = match (&start_obj.date_time, &start_obj.date) {
        (Some(dt), _) => (dt.clone(), false),
        (None, Some(d)) => (d.clone(), true),
        (None, None) => return None,
    };
    let end = raw
        .end
        .and_then(|e| e.date_time.or(e.date))
        .unwrap_or_else(|| start.clone());
    Some(GoogleCalendarEvent {
        id,
        summary: raw
            .summary
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "(Sem título)".into()),
        description: raw.description,
        location: raw.location,
        start,
        end,
        all_day,
        html_link: raw.html_link,
        status: raw.status,
    })
}

async fn list_calendar_events_with(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
    time_min: Option<&str>,
    time_max: Option<&str>,
    max_results: Option<u8>,
    now_ms: i64,
) -> Result<Vec<GoogleCalendarEvent>, String> {
    let token = ensure_valid_access_token(store, endpoints).await?;
    let mut url = Url::parse(&endpoints.calendar_events_url)
        .map_err(|_| "URL do Google Calendar inválida.".to_string())?;
    let min_str = clean_opt(time_min)
        .unwrap_or_else(|| iso_utc(now_ms.saturating_sub(6 * 60 * 60 * 1000)));
    let max_str = clean_opt(time_max)
        .unwrap_or_else(|| iso_utc(now_ms.saturating_add(14 * 24 * 60 * 60 * 1000)));
    let limit = max_results.unwrap_or(20).clamp(1, 50).to_string();

    url.query_pairs_mut()
        .append_pair("singleEvents", "true")
        .append_pair("orderBy", "startTime")
        .append_pair("timeMin", &min_str)
        .append_pair("timeMax", &max_str)
        .append_pair("maxResults", &limit);

    let http = net::client(&url, REQUEST_TIMEOUT)?;
    let resp = http
        .get(url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Falha ao consultar Google Calendar: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, net::MAX_BODY).await?;
    if !status.is_success() {
        return Err(format!(
            "Google Calendar retornou erro ({status}): {}",
            net::error_detail(&body)
        ));
    }
    let parsed: CalendarListRaw = serde_json::from_slice(&body)
        .map_err(|_| "Resposta inválida do Google Calendar.".to_string())?;
    Ok(parsed.items.into_iter().filter_map(map_calendar_event).collect())
}

fn format_offset_rfc3339(offset_minutes: i64) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let abs = offset_minutes.abs();
    format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
}

fn build_calendar_event_payload(
    task: &TodoItem,
    input: &GoogleCreateEventInput,
) -> Result<CalendarCreateEventPayload, String> {
    if !input.confirmed {
        return Err("A criação de evento no Google Calendar exige confirmação explícita.".into());
    }
    let summary = task.text.trim();
    if summary.is_empty() {
        return Err("A tarefa precisa ter título antes de criar um evento.".into());
    }

    let date_raw = input
        .override_date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(task.due_date.as_deref())
        .ok_or_else(|| "Defina uma data na tarefa antes de criar o evento no Google Calendar.".to_string())?;
    if !is_valid_date(date_raw) {
        return Err(format!("Data inválida para evento do Calendar: {date_raw}"));
    }
    let (y, m, d) = parse_date(date_raw).ok_or_else(|| "Data inválida.".to_string())?;

    let time_opt = match input
        .override_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(task.due_time.as_deref())
    {
        Some(t) => {
            if !is_valid_time(t) {
                return Err(format!("Horário inválido para evento do Calendar: {t}"));
            }
            Some(t)
        }
        None => None,
    };

    let (start, end) = if let Some(time_str) = time_opt {
        let (hh, mm) = parse_time(time_str).ok_or_else(|| "Horário inválido.".to_string())?;
        let duration = input.duration_minutes.unwrap_or(30).clamp(5, 480) as i64;
        let tz = if task.time_zone.trim().is_empty() {
            TODO_TIME_ZONE
        } else {
            task.time_zone.trim()
        };
        let offset = effective_offset_minutes(tz);
        let offset_str = format_offset_rfc3339(offset);

        let start_day = days_from_civil(y, m, d);
        let start_total_min = hh * 60 + mm;
        let end_total_min = start_total_min + duration;
        let extra_days = end_total_min.div_euclid(1440);
        let end_tod_min = end_total_min.rem_euclid(1440);
        let (ey, em, ed) = civil_from_days(start_day + extra_days);
        let end_date_str = format_date(ey, em, ed);

        let start_iso = format!("{date_raw}T{hh:02}:{mm:02}:00{offset_str}");
        let end_iso = format!(
            "{end_date_str}T{:02}:{:02}:00{offset_str}",
            end_tod_min / 60,
            end_tod_min % 60
        );
        (
            CalendarDateTimePayload {
                date_time: Some(start_iso),
                time_zone: Some(tz.to_string()),
                date: None,
            },
            CalendarDateTimePayload {
                date_time: Some(end_iso),
                time_zone: Some(tz.to_string()),
                date: None,
            },
        )
    } else {
        let next_day = civil_from_days(days_from_civil(y, m, d) + 1);
        let end_date = format_date(next_day.0, next_day.1, next_day.2);
        (
            CalendarDateTimePayload {
                date_time: None,
                time_zone: None,
                date: Some(date_raw.to_string()),
            },
            CalendarDateTimePayload {
                date_time: None,
                time_zone: None,
                date: Some(end_date),
            },
        )
    };

    let description = clean_opt(input.description.as_deref()).or_else(|| {
        if task.subtasks.is_empty() {
            Some("Criado pelo Lumen a partir de tarefa confirmada.".into())
        } else {
            let lines: Vec<String> = task
                .subtasks
                .iter()
                .map(|s| format!("- [{}] {}", if s.done { "x" } else { " " }, s.text))
                .collect();
            Some(lines.join("\n"))
        }
    });

    Ok(CalendarCreateEventPayload {
        summary: summary.to_string(),
        description,
        start,
        end,
    })
}

async fn create_event_from_task_with(
    store: &impl CredentialStore,
    endpoints: &GoogleEndpoints,
    task: &TodoItem,
    input: &GoogleCreateEventInput,
) -> Result<GoogleCalendarEvent, String> {
    let payload = build_calendar_event_payload(task, input)?;
    let token = ensure_valid_access_token(store, endpoints).await?;
    let mut url = Url::parse(&endpoints.calendar_events_url)
        .map_err(|_| "URL do Google Calendar inválida.".to_string())?;
    // Garante que nenhuma notificação ou convite seja disparado a terceiros.
    url.query_pairs_mut().append_pair("sendUpdates", "none");

    let http = net::client(&url, REQUEST_TIMEOUT)?;
    let resp = http
        .post(url)
        .bearer_auth(&token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Falha ao criar evento no Google Calendar: {e}"))?;
    let status = resp.status();
    let body = net::read_capped(resp, net::MAX_BODY).await?;
    if !status.is_success() {
        return Err(format!(
            "Google Calendar recusou criar o evento ({status}): {}",
            net::error_detail(&body)
        ));
    }
    let created: CalendarEventRaw = serde_json::from_slice(&body)
        .map_err(|_| "Resposta inválida ao criar evento no Google Calendar.".to_string())?;
    map_calendar_event(created)
        .ok_or_else(|| "Evento criado no Calendar veio sem identificador válido.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::model::TodoPriority;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct MemoryStore {
        map: Mutex<HashMap<String, String>>,
    }

    impl CredentialStore for MemoryStore {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn clear(&self, key: &str) -> Result<(), String> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn temp_workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-google-test-{}-{}-{}",
            tag,
            std::process::id(),
            random_base64url(6).unwrap()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_jwt_with_claims(email: &str, name: &str, sub: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            format!(r#"{{"email":"{email}","name":"{name}","sub":"{sub}"}}"#).as_bytes(),
        );
        format!("{header}.{payload}.sig")
    }

    fn sample_task(id: &str, date: Option<&str>, time: Option<&str>) -> TodoItem {
        TodoItem {
            id: id.into(),
            text: "Enviar relatório mensal".into(),
            due: "".into(),
            due_date: date.map(ToOwned::to_owned),
            due_time: time.map(ToOwned::to_owned),
            time_zone: TODO_TIME_ZONE.into(),
            alert_minutes: 15,
            priority: TodoPriority::Normal,
            subtasks: Vec::new(),
            recurrence: None,
            series_id: None,
            done: false,
            created_at: "2026-11-15T10:00:00Z".into(),
            done_at: None,
            snoozed_until: None,
            source: None,
            source_stale: false,
            version: 1,
            deleted_at: None,
        }
    }

    #[test]
    fn parse_client_config_accepts_installed_json_web_json_or_raw_id() {
        let installed = r#"{"installed":{"client_id":"cid-123.apps.googleusercontent.com","client_secret":"sec-abc","redirect_uris":["http://localhost"]}}"#;
        let parsed = parse_client_config_input(installed, None).unwrap();
        assert_eq!(parsed.client_id, "cid-123.apps.googleusercontent.com");
        assert_eq!(parsed.client_secret.as_deref(), Some("sec-abc"));

        let web = r#"{"web":{"client_id":"cid-web.apps.googleusercontent.com"}}"#;
        let parsed_web = parse_client_config_input(web, Some("override-sec")).unwrap();
        assert_eq!(parsed_web.client_id, "cid-web.apps.googleusercontent.com");
        assert_eq!(parsed_web.client_secret.as_deref(), Some("override-sec"));

        let plain = parse_client_config_input("cid-plain.apps.googleusercontent.com", None).unwrap();
        assert_eq!(plain.client_id, "cid-plain.apps.googleusercontent.com");
        assert_eq!(plain.client_secret, None);

        assert!(parse_client_config_input("  ", None).is_err());
        assert!(parse_client_config_input("cid com espaco", None).is_err());
    }

    #[tokio::test]
    async fn pkce_oauth_flow_succeeds_and_rejects_mismatched_state() {
        let store = MemoryStore::default();
        save_client_config(
            &store,
            &ClientConfig {
                client_id: "synthetic-cid.apps.googleusercontent.com".into(),
                client_secret: None,
            },
        )
        .unwrap();

        let mock_server = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = mock_server.local_addr().unwrap().port();
        let token_requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let tr = token_requests.clone();
        let id_jwt = make_jwt_with_claims("usuario@exemplo.test", "Usuário Teste", "sub-123");

        let server_task = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = mock_server.accept().await.unwrap();
                let tr = tr.clone();
                let id_jwt = id_jwt.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = stream.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let path = req.split_ascii_whitespace().nth(1).unwrap_or("");
                    let (status, body) = if path == "/token" {
                        tr.lock().unwrap().push(req.clone());
                        (
                            "200 OK",
                            format!(
                                r#"{{"access_token":"acc-1","refresh_token":"ref-1","expires_in":3600,"id_token":"{id_jwt}","scope":"openid email profile https://www.googleapis.com/auth/drive.file https://www.googleapis.com/auth/calendar.events"}}"#
                            ),
                        )
                    } else if path == "/userinfo" {
                        (
                            "200 OK",
                            r#"{"sub":"sub-123","email":"usuario@exemplo.test","name":"Usuário Teste"}"#.to_string(),
                        )
                    } else {
                        ("404 Not Found", r#"{"error":"not_found"}"#.to_string())
                    };
                    let reply = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });

        let endpoints = GoogleEndpoints {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: format!("http://127.0.0.1:{port}/token"),
            revoke_url: format!("http://127.0.0.1:{port}/revoke"),
            userinfo_url: format!("http://127.0.0.1:{port}/userinfo"),
            ..Default::default()
        };

        // 1) Teste state inválido rejeitado antes de trocar código.
        let bad_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let bad_port = bad_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let _ = client
                .get(format!(
                    "http://127.0.0.1:{bad_port}{CALLBACK_PATH}?state=wrong-state&code=c1"
                ))
                .send()
                .await;
        });
        let err = read_callback_code(
            bad_listener,
            "expected-state".into(),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.contains("state"), "{err}");

        // 2) Fluxo completo PKCE válido.
        let status = sign_in_with(&store, &endpoints, |auth_url| {
            let parsed = Url::parse(auth_url).unwrap();
            let params: HashMap<String, String> = parsed
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            assert_eq!(params.get("code_challenge_method").map(String::as_str), Some("S256"));
            let redirect = params.get("redirect_uri").unwrap().clone();
            let state = params.get("state").unwrap().clone();
            tokio::spawn(async move {
                let client = reqwest::Client::builder().no_proxy().build().unwrap();
                let mut cb = Url::parse(&redirect).unwrap();
                cb.query_pairs_mut()
                    .append_pair("code", "auth-code-999")
                    .append_pair("state", &state);
                let _ = client.get(cb).send().await;
            });
            Ok(())
        })
        .await
        .unwrap();

        let _ = server_task.await;
        assert!(status.connected);
        assert_eq!(status.email.as_deref(), Some("usuario@exemplo.test"));
        assert_eq!(status.display_name.as_deref(), Some("Usuário Teste"));
        let token_body = token_requests.lock().unwrap().pop().unwrap();
        assert!(token_body.contains("code_verifier="));
        assert!(token_body.contains("code=auth-code-999"));
        assert_eq!(store.get(KEY_ACCESS_TOKEN).unwrap().as_deref(), Some("acc-1"));
        assert_eq!(store.get(KEY_REFRESH_TOKEN).unwrap().as_deref(), Some("ref-1"));
    }

    #[tokio::test]
    async fn refresh_token_is_serialized_on_single_network_call() {
        let store = MemoryStore::default();
        save_client_config(
            &store,
            &ClientConfig {
                client_id: "cid".into(),
                client_secret: None,
            },
        )
        .unwrap();
        store.set(KEY_ACCESS_TOKEN, "expired-acc").unwrap();
        store.set(KEY_REFRESH_TOKEN, "ref-keep").unwrap();
        let meta = GoogleAccountMeta {
            email: Some("a@b.test".into()),
            expires_at_ms: now_utc_ms() - 10_000,
            ..Default::default()
        };
        store
            .set(KEY_ACCOUNT_META, &serde_json::to_string(&meta).unwrap())
            .unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_srv = hits.clone();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                hits_srv.fetch_add(1, Ordering::SeqCst);
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let body = r#"{"access_token":"fresh-tok","expires_in":3600}"#;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });

        let endpoints = GoogleEndpoints {
            token_url: format!("http://127.0.0.1:{port}/token"),
            ..Default::default()
        };
        let (a, b) = tokio::join!(
            ensure_valid_access_token(&store, &endpoints),
            ensure_valid_access_token(&store, &endpoints),
        );
        assert_eq!(a.unwrap(), "fresh-tok");
        assert_eq!(b.unwrap(), "fresh-tok");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn backup_package_excludes_wav_and_secrets_and_roundtrips_sha256() {
        let root = temp_workspace("pkg");
        let tasks = root.join("Todo");
        let notes = root.join("Notes");
        let personas = root.join("Personas");
        let cfg = root.join("Config");
        fs::create_dir_all(&tasks).unwrap();
        fs::create_dir_all(notes.join("n1")).unwrap();
        fs::create_dir_all(&personas).unwrap();
        fs::create_dir_all(&cfg).unwrap();

        fs::write(tasks.join("todo.json"), br#"{"schemaVersion":1,"items":[{"id":"t1"}]}"#).unwrap();
        fs::write(notes.join("n1").join("note.json"), br#"{"id":"n1","title":"Nota 1"}"#).unwrap();
        fs::write(notes.join("n1").join("mic.wav"), b"RIFF_AUDIO_SHOULD_NOT_ENTER").unwrap();
        fs::write(personas.join("personas.json"), br#"[{"id":"p1"}]"#).unwrap();
        fs::write(cfg.join("settings.json"), br#"{"language":"pt-BR"}"#).unwrap();

        let pkg = build_backup_package_at(&tasks, &notes, &personas, &cfg, 1_763_200_000_000).unwrap();
        let json_str = serde_json::to_string(&pkg).unwrap();
        assert!(!json_str.contains("RIFF_AUDIO_SHOULD_NOT_ENTER"));
        assert!(!json_str.contains("google-oauth"));
        assert_eq!(pkg.device, DRIVE_DEVICE_FOLDER);
        assert_eq!(pkg.payload.notes.len(), 1);

        let parsed: LumenBackupPackage = serde_json::from_str(&json_str).unwrap();
        let recomputed = sha256_hex(&serde_json::to_vec(&parsed.payload).unwrap());
        assert_eq!(recomputed, parsed.sha256);

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn drive_backup_uses_segunda_mente_lumen_pc_and_restore_checks_sha256() {
        let store = MemoryStore::default();
        save_client_config(
            &store,
            &ClientConfig {
                client_id: "cid".into(),
                client_secret: None,
            },
        )
        .unwrap();
        store.set(KEY_ACCESS_TOKEN, "valid-tok").unwrap();
        store
            .set(
                KEY_ACCOUNT_META,
                &serde_json::to_string(&GoogleAccountMeta {
                    email: Some("u@exemplo.test".into()),
                    expires_at_ms: now_utc_ms() + 3_600_000,
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();

        let root_src = temp_workspace("drive-src");
        let tasks_src = root_src.join("Todo");
        let notes_src = root_src.join("Notes");
        let personas_src = root_src.join("Personas");
        let cfg_src = root_src.join("Config");
        fs::create_dir_all(&tasks_src).unwrap();
        fs::create_dir_all(notes_src.join("nota-a")).unwrap();
        fs::create_dir_all(&personas_src).unwrap();
        fs::create_dir_all(&cfg_src).unwrap();
        fs::write(tasks_src.join("todo.json"), r#"{"schemaVersion":1,"items":[{"id":"x1","text":"Comprar café"}]}"#.as_bytes()).unwrap();
        fs::write(notes_src.join("nota-a").join("note.json"), r#"{"id":"nota-a","title":"Reunião"}"#.as_bytes()).unwrap();
        fs::write(personas_src.join("personas.json"), br#"[{"id":"socio_tecnico"}]"#).unwrap();
        fs::write(cfg_src.join("settings.json"), br#"{"language":"pt-BR"}"#).unwrap();

        #[derive(Default)]
        struct DriveMockState {
            root_id: Option<String>,
            device_id: Option<String>,
            uploaded_json: Option<Vec<u8>>,
            corrupt_on_download: bool,
        }
        let drive_state = Arc::new(Mutex::new(DriveMockState::default()));
        let ds = drive_state.clone();

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let ds = ds.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let mut header_end = None;
                    let mut content_len = 0usize;
                    loop {
                        let Ok(n) = stream.read(&mut tmp).await else { return };
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&tmp[..n]);
                        if header_end.is_none() {
                            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                                let he = pos + 4;
                                header_end = Some(he);
                                let hdr = String::from_utf8_lossy(&raw[..pos]);
                                for line in hdr.lines() {
                                    if let Some((k, v)) = line.split_once(':') {
                                        if k.trim().eq_ignore_ascii_case("content-length") {
                                            content_len = v.trim().parse().unwrap_or(0);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(he) = header_end {
                            if raw.len() >= he + content_len {
                                break;
                            }
                        }
                    }
                    let he = header_end.unwrap_or(raw.len());
                    let hdr_str = String::from_utf8_lossy(&raw[..he]).into_owned();
                    let body_bytes = &raw[he..];
                    let first = hdr_str.lines().next().unwrap_or("");
                    let mut parts = first.split_ascii_whitespace();
                    let method = parts.next().unwrap_or("");
                    let target = parts.next().unwrap_or("");

                    let (status, ctype, resp_body): (&str, &str, Vec<u8>) = {
                        let mut st = ds.lock().unwrap();
                        if method == "GET" && target.starts_with("/drive/v3/files?") {
                            let parsed = Url::parse(&format!("http://127.0.0.1{target}")).unwrap();
                            let q = parsed
                                .query_pairs()
                                .find(|(k, _)| k == "q")
                                .map(|(_, v)| v.into_owned())
                                .unwrap_or_default();
                            if q.contains(DRIVE_ROOT_FOLDER) {
                                if let Some(id) = st.root_id.clone() {
                                    ("200 OK", "application/json", format!(r#"{{"files":[{{"id":"{id}","name":"{DRIVE_ROOT_FOLDER}"}}]}}"#).into_bytes())
                                } else {
                                    ("200 OK", "application/json", br#"{"files":[]}"#.to_vec())
                                }
                            } else if q.contains(DRIVE_DEVICE_FOLDER) {
                                assert!(st.root_id.is_some());
                                if let Some(id) = st.device_id.clone() {
                                    ("200 OK", "application/json", format!(r#"{{"files":[{{"id":"{id}","name":"{DRIVE_DEVICE_FOLDER}"}}]}}"#).into_bytes())
                                } else {
                                    ("200 OK", "application/json", br#"{"files":[]}"#.to_vec())
                                }
                            } else if q.contains(DRIVE_BACKUP_FILENAME) {
                                if st.uploaded_json.is_some() {
                                    ("200 OK", "application/json", format!(r#"{{"files":[{{"id":"bak-1","name":"{DRIVE_BACKUP_FILENAME}"}}]}}"#).into_bytes())
                                } else {
                                    ("200 OK", "application/json", br#"{"files":[]}"#.to_vec())
                                }
                            } else {
                                ("200 OK", "application/json", br#"{"files":[]}"#.to_vec())
                            }
                        } else if method == "POST" && target.starts_with("/drive/v3/files?") {
                            let v: Value = serde_json::from_slice(body_bytes).unwrap();
                            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
                            if name == DRIVE_ROOT_FOLDER {
                                st.root_id = Some("folder-root".into());
                                ("200 OK", "application/json", br#"{"id":"folder-root"}"#.to_vec())
                            } else if name == DRIVE_DEVICE_FOLDER {
                                let parent = v["parents"][0].as_str().unwrap_or("");
                                assert_eq!(parent, "folder-root");
                                st.device_id = Some("folder-lumen-pc".into());
                                ("200 OK", "application/json", br#"{"id":"folder-lumen-pc"}"#.to_vec())
                            } else {
                                ("400 Bad Request", "application/json", br#"{"error":"unexpected folder"}"#.to_vec())
                            }
                        } else if (method == "POST" || method == "PATCH")
                            && target.starts_with("/upload/drive/v3/files")
                        {
                            // Extrai a 2ª parte JSON do multipart/related.
                            let body_text = String::from_utf8_lossy(body_bytes);
                            let json_part = body_text
                                .split("\r\n\r\n")
                                .nth(2)
                                .unwrap_or("");
                            let trimmed = json_part.split("\r\n--").next().unwrap_or(json_part);
                            st.uploaded_json = Some(trimmed.as_bytes().to_vec());
                            ("200 OK", "application/json", br#"{"id":"bak-1"}"#.to_vec())
                        } else if method == "GET" && target.starts_with("/drive/v3/files/bak-1?alt=media") {
                            let mut payload = st.uploaded_json.clone().unwrap();
                            if st.corrupt_on_download {
                                let txt = String::from_utf8(payload).unwrap().replace("Comprar café", "Texto adulterado");
                                payload = txt.into_bytes();
                            }
                            ("200 OK", "application/json", payload)
                        } else {
                            ("404 Not Found", "application/json", br#"{"error":"unknown"}"#.to_vec())
                        }
                    };
                    let hdr = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        resp_body.len()
                    );
                    let _ = stream.write_all(hdr.as_bytes()).await;
                    let _ = stream.write_all(&resp_body).await;
                });
            }
        });

        let endpoints = GoogleEndpoints {
            drive_files_url: format!("http://127.0.0.1:{port}/drive/v3/files"),
            drive_upload_url: format!("http://127.0.0.1:{port}/upload/drive/v3/files"),
            ..Default::default()
        };

        let uploaded = run_backup_now(
            &store,
            &endpoints,
            &tasks_src,
            &notes_src,
            &personas_src,
            &cfg_src,
            1_763_200_000_000,
        )
        .await
        .unwrap();
        assert_eq!(uploaded.drive_folder, DRIVE_FOLDER_DISPLAY);
        assert_eq!(uploaded.notes_count, 1);
        assert!(uploaded.tasks_included);

        // Restaura em outro diretório e valida conteúdo.
        let root_dst = temp_workspace("drive-dst");
        let tasks_dst = root_dst.join("Todo");
        let notes_dst = root_dst.join("Notes");
        let personas_dst = root_dst.join("Personas");
        let cfg_dst = root_dst.join("Config");
        fs::create_dir_all(&tasks_dst).unwrap();
        fs::write(tasks_dst.join("todo.json"), br#"{"schemaVersion":1,"items":[{"id":"old"}]}"#).unwrap();

        let restored = restore_backup_at(
            &store,
            &endpoints,
            &tasks_dst,
            &notes_dst,
            &personas_dst,
            &cfg_dst,
            1_763_200_100_000,
        )
        .await
        .unwrap();
        assert!(restored.tasks_restored);
        assert_eq!(restored.notes_restored, 1);
        assert!(restored.personas_restored);
        assert!(restored.settings_restored);
        let todo_after = fs::read_to_string(tasks_dst.join("todo.json")).unwrap();
        assert!(todo_after.contains("Comprar café"));
        let note_after = fs::read_to_string(notes_dst.join("nota-a").join("note.json")).unwrap();
        assert!(note_after.contains("Reunião"));

        // Simula corrupção no Drive -> SHA-256 precisa bloquear sem sobrescrever!
        drive_state.lock().unwrap().corrupt_on_download = true;
        fs::write(tasks_dst.join("todo.json"), br#"{"keep":"local"}"#).unwrap();
        let err = restore_backup_at(
            &store,
            &endpoints,
            &tasks_dst,
            &notes_dst,
            &personas_dst,
            &cfg_dst,
            1_763_200_200_000,
        )
        .await
        .unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        let kept = fs::read_to_string(tasks_dst.join("todo.json")).unwrap();
        assert!(kept.contains("local"));

        let _ = fs::remove_dir_all(&root_src);
        let _ = fs::remove_dir_all(&root_dst);
    }

    #[tokio::test]
    async fn calendar_requires_confirmation_and_date_and_never_sends_attendees() {
        let store = MemoryStore::default();
        save_client_config(
            &store,
            &ClientConfig {
                client_id: "cid".into(),
                client_secret: None,
            },
        )
        .unwrap();
        store.set(KEY_ACCESS_TOKEN, "valid-tok").unwrap();
        store
            .set(
                KEY_ACCOUNT_META,
                &serde_json::to_string(&GoogleAccountMeta {
                    email: Some("u@exemplo.test".into()),
                    expires_at_ms: now_utc_ms() + 3_600_000,
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();

        let task_ok = sample_task("t1", Some("2026-11-20"), Some("23:45"));
        let task_no_date = sample_task("t2", None, None);

        // 1) Sem confirmed=true deve falhar localmente antes de qualquer HTTP.
        let err_unconfirmed = build_calendar_event_payload(
            &task_ok,
            &GoogleCreateEventInput {
                task_id: "t1".into(),
                confirmed: false,
                override_date: None,
                override_time: None,
                duration_minutes: Some(30),
                description: None,
            },
        )
        .unwrap_err();
        assert!(err_unconfirmed.contains("confirmação"));

        // 2) Sem data deve falhar localmente.
        let err_nodate = build_calendar_event_payload(
            &task_no_date,
            &GoogleCreateEventInput {
                task_id: "t2".into(),
                confirmed: true,
                override_date: None,
                override_time: None,
                duration_minutes: Some(30),
                description: None,
            },
        )
        .unwrap_err();
        assert!(err_nodate.contains("data"));

        // 3) Com confirmação + virada de meia-noite valida payload + sendUpdates=none + sem attendees.
        let captured_post = Arc::new(Mutex::new((String::new(), String::new())));
        let cp = captured_post.clone();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let cp = cp.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = stream.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let first = req.lines().next().unwrap_or("");
                    let mut parts = first.split_ascii_whitespace();
                    let method = parts.next().unwrap_or("");
                    let target = parts.next().unwrap_or("");
                    let body_in = req.split("\r\n\r\n").nth(1).unwrap_or("");
                    let (status, resp) = if method == "GET" {
                        (
                            "200 OK",
                            r#"{"items":[{"id":"ev-1","summary":"Call trimestral","start":{"dateTime":"2026-11-16T10:00:00-03:00"},"end":{"dateTime":"2026-11-16T10:30:00-03:00"},"status":"confirmed"}]}"#,
                        )
                    } else {
                        *cp.lock().unwrap() = (target.to_string(), body_in.to_string());
                        (
                            "200 OK",
                            r#"{"id":"ev-created","summary":"Enviar relatório mensal","start":{"dateTime":"2026-11-20T23:45:00-03:00"},"end":{"dateTime":"2026-11-21T00:15:00-03:00"},"status":"confirmed"}"#,
                        )
                    };
                    let reply = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                        resp.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });

        let endpoints = GoogleEndpoints {
            calendar_events_url: format!("http://127.0.0.1:{port}/calendar/v3/calendars/primary/events"),
            ..Default::default()
        };

        let events = list_calendar_events_with(&store, &endpoints, None, None, None, 1_763_200_000_000)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Call trimestral");

        let created = create_event_from_task_with(
            &store,
            &endpoints,
            &task_ok,
            &GoogleCreateEventInput {
                task_id: "t1".into(),
                confirmed: true,
                override_date: None,
                override_time: None,
                duration_minutes: Some(30),
                description: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(created.id, "ev-created");

        let (post_target, post_body) = captured_post.lock().unwrap().clone();
        assert!(post_target.contains("sendUpdates=none"), "{post_target}");
        assert!(!post_body.contains("attendees"), "{post_body}");
        assert!(post_body.contains("2026-11-21T00:15:00"), "{post_body}");
    }

    #[test]
    fn no_secrets_logged_and_payload_struct_has_no_attendees() {
        let src = include_str!("google.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!prod.contains("crate::log::line"));
        assert!(!prod.contains("println!"));
        assert!(!prod.contains("eprintln!"));
        let payload = CalendarCreateEventPayload {
            summary: "Teste".into(),
            description: None,
            start: CalendarDateTimePayload {
                date_time: None,
                time_zone: None,
                date: Some("2026-11-20".into()),
            },
            end: CalendarDateTimePayload {
                date_time: None,
                time_zone: None,
                date: Some("2026-11-21".into()),
            },
        };
        let encoded = serde_json::to_string(&payload).unwrap();
        assert!(!encoded.contains("attendees"));
    }
}
