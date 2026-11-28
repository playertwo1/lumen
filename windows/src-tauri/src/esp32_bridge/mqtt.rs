//! Cliente MQTT da ponte Lumen para ESP32-S3 AMOLED e Segunda Mente.
//!
//! Implementa o protocolo definido em `docs/roadmap/mqtt/PROTOCOLO.md`:
//! - TLS via rustls com ring provider existente.
//! - Presença com LWT (`sm/<sala>/presenca/lumen`, "0" retido QoS 1; "1" retido ao conectar).
//! - Monitoramento da presença da placa (`sm/<sala>/presenca/placa`).
//! - Sincronização de tarefas (`sm/<sala>/tarefas/+` retido QoS 1) com janela de settle.
//! - Controle de mídia do PC (`sm/<sala>/pc/midia` -> `sm/<sala>/pc/midia/estado`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rumqttc::{AsyncClient, Event, Incoming, LastWill, MqttOptions, QoS, SubscribeFilter};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::crypto::{open_mqtt_envelope, seal_mqtt_envelope};

pub const DEFAULT_SETTLE_DURATION: Duration = Duration::from_secs(3);
pub const REVOKED_KEY_ERROR_MSG: &str = "Chave da placa mudou — cole o código novo";

fn is_valid_envelope_shape(raw: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    v.get("v").and_then(|x| x.as_u64()) == Some(1)
        && v.get("from").and_then(|x| x.as_str()).is_some_and(|s| !s.trim().is_empty())
        && v.get("iv").and_then(|x| x.as_str()).is_some()
        && v.get("ct").and_then(|x| x.as_str()).is_some()
        && v.get("tag").and_then(|x| x.as_str()).is_some()
}

pub(crate) fn decrypt_incoming_envelope(
    key: &str,
    env_str: &str,
    state: &MqttRuntimeState,
) -> Option<(String, String)> {
    match open_mqtt_envelope(key, env_str) {
        Ok((from, plain_json)) => {
            if state.last_error.read().unwrap().as_deref() == Some(REVOKED_KEY_ERROR_MSG) {
                state.set_last_error(None);
            }
            Some((from, plain_json))
        }
        Err(_) => {
            if is_valid_envelope_shape(env_str) {
                state.set_last_error(Some(REVOKED_KEY_ERROR_MSG.to_string()));
            }
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttConfig {
    pub uri: String,
    pub user: String,
    pub pass: String,
    pub room: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttStatusSnapshot {
    pub configured: bool,
    pub connected: bool,
    pub board_online: bool,
    pub host: String,
    pub room: String,
    pub last_error: Option<String>,
}

pub struct MqttRuntimeState {
    pub configured: AtomicBool,
    pub connected: AtomicBool,
    pub board_online: AtomicBool,
    pub host: std::sync::RwLock<String>,
    pub room: std::sync::RwLock<String>,
    pub last_error: std::sync::RwLock<Option<String>>,
}

impl MqttRuntimeState {
    pub fn new() -> Self {
        Self {
            configured: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            board_online: AtomicBool::new(false),
            host: std::sync::RwLock::new(String::new()),
            room: std::sync::RwLock::new(String::new()),
            last_error: std::sync::RwLock::new(None),
        }
    }

    pub fn snapshot(&self) -> MqttStatusSnapshot {
        MqttStatusSnapshot {
            configured: self.configured.load(Ordering::Relaxed),
            connected: self.connected.load(Ordering::Relaxed),
            board_online: self.board_online.load(Ordering::Relaxed),
            host: self.host.read().unwrap().clone(),
            room: self.room.read().unwrap().clone(),
            last_error: self.last_error.read().unwrap().clone(),
        }
    }

    pub fn set_configured(&self, host: String, room: String) {
        *self.host.write().unwrap() = host;
        *self.room.write().unwrap() = room;
        self.configured.store(true, Ordering::Relaxed);
        *self.last_error.write().unwrap() = None;
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
        if !connected {
            self.board_online.store(false, Ordering::Relaxed);
        }
    }

    pub fn set_board_online(&self, online: bool) {
        self.board_online.store(online, Ordering::Relaxed);
    }

    pub fn set_last_error(&self, err: Option<String>) {
        *self.last_error.write().unwrap() = err;
    }
}

pub fn parse_broker_uri(uri: &str) -> Result<(String, u16, bool), String> {
    let trimmed = uri.trim();
    let (without_scheme, default_port, use_tls) = if let Some(s) = trimmed.strip_prefix("mqtts://") {
        (s, 8883, true)
    } else if let Some(s) = trimmed.strip_prefix("mqtt://") {
        (s, 1883, false)
    } else {
        return Err(format!("URI inválida: esquema deve ser mqtts:// ou mqtt:// ({trimmed})"));
    };

    let host_and_port = without_scheme.split('/').next().unwrap_or("").trim();
    if host_and_port.is_empty() {
        return Err("Host vazio na URI do broker".into());
    }

    if let Some((host, port_str)) = host_and_port.split_once(':') {
        let port: u16 = port_str
            .parse()
            .map_err(|e| format!("Porta inválida '{port_str}' na URI: {e}"))?;
        Ok((host.to_string(), port, use_tls))
    } else {
        Ok((host_and_port.to_string(), default_port, use_tls))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaActionPayload {
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaStatePayload {
    pub playing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
}

pub trait MqttCallbacks: Send + Sync + 'static {
    fn on_task_upsert(&self, entity_id: &str, plain_json: &str);
    fn on_task_delete(&self, entity_id: &str);
    fn on_settle_prune(&self, seen_task_ids: &HashSet<String>, retained_count: usize);
    fn on_media_action(&self, action: &str) -> Option<MediaStatePayload>;
    fn on_agent_decision(&self, _from: &str, _plain_json: &str) {}
}

#[derive(Debug)]
pub enum MqttCommand {
    PublishTask {
        entity_id: String,
        plain_json: String,
    },
    DeleteTask {
        entity_id: String,
    },
    PublishMediaState {
        payload: MediaStatePayload,
    },
    PublishAgentStatus {
        plain_json: String,
    },
    PublishAgentRequest {
        plain_json: Option<String>,
    },
    PublishUsage {
        plain_json: String,
    },
    PublishMaxTracker {
        plain_json: String,
    },
    SyncNow,
    Shutdown {
        done: Option<tokio::sync::oneshot::Sender<()>>,
    },
}

#[derive(Clone)]
pub struct MqttHandle {
    sender: mpsc::Sender<MqttCommand>,
}

impl MqttHandle {
    pub async fn publish_task(&self, entity_id: String, plain_json: String) -> Result<(), String> {
        self.sender
            .send(MqttCommand::PublishTask {
                entity_id,
                plain_json,
            })
            .await
            .map_err(|e| format!("Failed to send PublishTask command: {e}"))
    }

    pub async fn delete_task(&self, entity_id: String) -> Result<(), String> {
        self.sender
            .send(MqttCommand::DeleteTask { entity_id })
            .await
            .map_err(|e| format!("Failed to send DeleteTask command: {e}"))
    }

    pub async fn publish_media_state(&self, payload: MediaStatePayload) -> Result<(), String> {
        self.sender
            .send(MqttCommand::PublishMediaState { payload })
            .await
            .map_err(|e| format!("Failed to send PublishMediaState command: {e}"))
    }

    pub async fn publish_agent_status(&self, plain_json: String) -> Result<(), String> {
        self.sender
            .send(MqttCommand::PublishAgentStatus { plain_json })
            .await
            .map_err(|e| format!("Failed to send PublishAgentStatus command: {e}"))
    }

    pub async fn publish_agent_request(&self, plain_json: Option<String>) -> Result<(), String> {
        self.sender
            .send(MqttCommand::PublishAgentRequest { plain_json })
            .await
            .map_err(|e| format!("Failed to send PublishAgentRequest command: {e}"))
    }

    pub async fn publish_usage(&self, plain_json: String) -> Result<(), String> {
        self.sender
            .send(MqttCommand::PublishUsage { plain_json })
            .await
            .map_err(|e| format!("Failed to send PublishUsage command: {e}"))
    }

    pub async fn publish_max_tracker(&self, plain_json: String) -> Result<(), String> {
        self.sender
            .send(MqttCommand::PublishMaxTracker { plain_json })
            .await
            .map_err(|e| format!("Failed to send PublishMaxTracker command: {e}"))
    }

    pub async fn sync_now(&self) -> Result<(), String> {
        self.sender
            .send(MqttCommand::SyncNow)
            .await
            .map_err(|e| format!("Failed to send SyncNow command: {e}"))
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        self.sender
            .send(MqttCommand::Shutdown {
                done: Some(done_tx),
            })
            .await
            .map_err(|e| format!("Failed to send Shutdown command: {e}"))?;
        let _ = tokio::time::timeout(Duration::from_millis(1200), done_rx).await;
        Ok(())
    }
}

pub fn spawn_mqtt_loop<C: MqttCallbacks>(
    cfg: MqttConfig,
    callbacks: Arc<C>,
    state: Arc<MqttRuntimeState>,
    settle_duration: Duration,
) -> Result<MqttHandle, String> {
    let (host, port, use_tls) = parse_broker_uri(&cfg.uri)?;
    state.set_configured(host.clone(), cfg.room.clone());

    let client_id = format!("lumen-{}", cfg.room);
    let mut mqttoptions = MqttOptions::new(client_id, host.clone(), port);
    mqttoptions.set_credentials(&cfg.user, &cfg.pass);
    mqttoptions.set_keep_alive(Duration::from_secs(60));

    let lwt_topic = format!("sm/{}/presenca/lumen", cfg.room);
    mqttoptions.set_last_will(LastWill::new(lwt_topic.clone(), "0", QoS::AtLeastOnce, true));

    if use_tls {
        let mut root_store = rustls::RootCertStore::empty();
        for cert in webpki_roots::TLS_SERVER_ROOTS.iter() {
            root_store.roots.push(cert.clone());
        }
        let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("Failed to set rustls protocol versions: {e}"))?
        .with_root_certificates(root_store)
        .with_no_client_auth();

        mqttoptions.set_transport(rumqttc::Transport::tls_with_config(
            rumqttc::TlsConfiguration::Rustls(Arc::new(client_config)),
        ));
    }

    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 64);
    let (tx, mut rx) = mpsc::channel::<MqttCommand>(128);

    let handle = MqttHandle { sender: tx };

    let room = cfg.room.clone();
    let key = cfg.key.clone();

    tokio::spawn(async move {
        let presence_lumen_topic = format!("sm/{room}/presenca/lumen");
        let presence_board_topic = format!("sm/{room}/presenca/placa");
        let tasks_sub_topic = format!("sm/{room}/tarefas/+");
        let media_action_topic = format!("sm/{room}/pc/midia");
        let media_state_topic = format!("sm/{room}/pc/midia/estado");
        let agent_decision_topic = format!("sm/{room}/pc/agentes/decisao");
        let agent_status_topic = format!("sm/{room}/pc/agentes/status");
        let agent_request_topic = format!("sm/{room}/pc/agentes/pedido");
        let usage_topic = format!("sm/{room}/pc/uso");
        let max_tracker_topic = format!("sm/{room}/pc/max-tracker");
        let task_topic_prefix = format!("sm/{room}/tarefas/");

        let mut suback_tasks_ok = false;
        let mut pruned = false;
        let mut seen_task_ids = HashSet::<String>::new();
        let mut retained_tasks_count = 0usize;
        let mut connected_instant: Option<Instant> = None;

        let mut is_connected = false;

        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(MqttCommand::PublishTask { entity_id, plain_json }) => {
                            if !is_connected {
                                continue;
                            }
                            let topic = format!("{}{}", task_topic_prefix, entity_id);
                            match seal_mqtt_envelope(&key, "lumen", &plain_json) {
                                Ok(envelope) => {
                                    if let Err(e) = client.publish(topic, QoS::AtLeastOnce, true, envelope.into_bytes()).await {
                                        state.set_last_error(Some(format!("Erro ao publicar tarefa: {e}")));
                                    }
                                }
                                Err(e) => {
                                    state.set_last_error(Some(format!("Erro ao cifrar tarefa: {e}")));
                                }
                            }
                        }
                        Some(MqttCommand::DeleteTask { entity_id }) => {
                            if !is_connected {
                                continue;
                            }
                            let topic = format!("{}{}", task_topic_prefix, entity_id);
                            // Deletion via empty payload with retain true to clear broker retention
                            if let Err(e) = client.publish(topic, QoS::AtLeastOnce, true, Vec::<u8>::new()).await {
                                state.set_last_error(Some(format!("Erro ao apagar tarefa remota: {e}")));
                            }
                        }
                        Some(MqttCommand::PublishMediaState { payload }) => {
                            if !is_connected {
                                continue;
                            }
                            if let Ok(plain_json) = serde_json::to_string(&payload) {
                                if let Ok(envelope) = seal_mqtt_envelope(&key, "lumen", &plain_json) {
                                    let _ = client.publish(media_state_topic.clone(), QoS::AtLeastOnce, true, envelope.into_bytes()).await;
                                }
                            }
                        }
                        Some(MqttCommand::PublishAgentStatus { plain_json }) => {
                            if !is_connected {
                                continue;
                            }
                            match seal_mqtt_envelope(&key, "lumen", &plain_json) {
                                Ok(envelope) => {
                                    if let Err(e) = client
                                        .publish(
                                            agent_status_topic.clone(),
                                            QoS::AtLeastOnce,
                                            true,
                                            envelope.into_bytes(),
                                        )
                                        .await
                                    {
                                        state.set_last_error(Some(format!(
                                            "Erro ao publicar status dos agentes: {e}"
                                        )));
                                    }
                                }
                                Err(e) => {
                                    state.set_last_error(Some(format!(
                                        "Erro ao cifrar status dos agentes: {e}"
                                    )));
                                }
                            }
                        }
                        Some(MqttCommand::PublishAgentRequest { plain_json }) => {
                            if !is_connected {
                                continue;
                            }
                            match plain_json {
                                Some(json) => match seal_mqtt_envelope(&key, "lumen", &json) {
                                    Ok(envelope) => {
                                        if let Err(e) = client
                                            .publish(
                                                agent_request_topic.clone(),
                                                QoS::AtLeastOnce,
                                                true,
                                                envelope.into_bytes(),
                                            )
                                            .await
                                        {
                                            state.set_last_error(Some(format!(
                                                "Erro ao publicar pedido dos agentes: {e}"
                                            )));
                                        }
                                    }
                                    Err(e) => {
                                        state.set_last_error(Some(format!(
                                            "Erro ao cifrar pedido dos agentes: {e}"
                                        )));
                                    }
                                },
                                None => {
                                    if let Err(e) = client
                                        .publish(
                                            agent_request_topic.clone(),
                                            QoS::AtLeastOnce,
                                            true,
                                            Vec::<u8>::new(),
                                        )
                                        .await
                                    {
                                        state.set_last_error(Some(format!(
                                            "Erro ao limpar pedido dos agentes: {e}"
                                        )));
                                    }
                                }
                            }
                        }
                        Some(MqttCommand::PublishUsage { plain_json }) => {
                            if !is_connected {
                                continue;
                            }
                            match seal_mqtt_envelope(&key, "lumen", &plain_json) {
                                Ok(envelope) => {
                                    if let Err(e) = client
                                        .publish(
                                            usage_topic.clone(),
                                            QoS::AtLeastOnce,
                                            true,
                                            envelope.into_bytes(),
                                        )
                                        .await
                                    {
                                        state.set_last_error(Some(format!(
                                            "Erro ao publicar uso: {e}"
                                        )));
                                    }
                                }
                                Err(e) => {
                                    state.set_last_error(Some(format!(
                                        "Erro ao cifrar uso: {e}"
                                    )));
                                }
                            }
                        }
                        Some(MqttCommand::PublishMaxTracker { plain_json }) => {
                            if !is_connected {
                                continue;
                            }
                            match seal_mqtt_envelope(&key, "lumen", &plain_json) {
                                Ok(envelope) => {
                                    if let Err(e) = client
                                        .publish(
                                            max_tracker_topic.clone(),
                                            QoS::AtLeastOnce,
                                            true,
                                            envelope.into_bytes(),
                                        )
                                        .await
                                    {
                                        state.set_last_error(Some(format!(
                                            "Erro ao publicar max-tracker: {e}"
                                        )));
                                    }
                                }
                                Err(e) => {
                                    state.set_last_error(Some(format!(
                                        "Erro ao cifrar max-tracker: {e}"
                                    )));
                                }
                            }
                        }
                        Some(MqttCommand::SyncNow) => {
                            // Can re-trigger prune callback if needed
                            if is_connected && suback_tasks_ok {
                                callbacks.on_settle_prune(&seen_task_ids, retained_tasks_count);
                            }
                        }
                        cmd @ (Some(MqttCommand::Shutdown { .. }) | None) => {
                            let done = match cmd {
                                Some(MqttCommand::Shutdown { done }) => done,
                                _ => None,
                            };
                            // 1) Limpa pedido pendente com payload vazio retido
                            let _ = client
                                .publish(
                                    agent_request_topic.clone(),
                                    QoS::AtLeastOnce,
                                    true,
                                    Vec::<u8>::new(),
                                )
                                .await;
                            // 2) Publica status com active_count: 0 cifrado e retido
                            let empty_status = r#"{"v":2,"seq":0,"agents":{"claude":{"active_count":0,"jobs":[]},"codex":{"active_count":0,"jobs":[]}}}"#;
                            if let Ok(env) = seal_mqtt_envelope(&key, "lumen", empty_status) {
                                let _ = client
                                    .publish(
                                        agent_status_topic.clone(),
                                        QoS::AtLeastOnce,
                                        true,
                                        env.into_bytes(),
                                    )
                                    .await;
                            }
                            let _ = client
                                .publish(
                                    presence_lumen_topic.clone(),
                                    QoS::AtLeastOnce,
                                    true,
                                    "0".as_bytes().to_vec(),
                                )
                                .await;
                            let _ = client.disconnect().await;
                            state.set_connected(false);
                            if let Some(tx) = done {
                                let _ = tx.send(());
                            }
                            break;
                        }
                    }
                }
                event = eventloop.poll() => {
                    match event {
                        Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                            is_connected = true;
                            state.set_connected(true);
                            state.set_last_error(None);

                            // 1. Publish presence "1" (retained, QoS 1)
                            let _ = client.publish(presence_lumen_topic.clone(), QoS::AtLeastOnce, true, "1".as_bytes().to_vec()).await;

                            // 2. Subscribe to topics
                            let filters = vec![
                                SubscribeFilter::new(presence_board_topic.clone(), QoS::AtLeastOnce),
                                SubscribeFilter::new(tasks_sub_topic.clone(), QoS::AtLeastOnce),
                                SubscribeFilter::new(media_action_topic.clone(), QoS::AtLeastOnce),
                                SubscribeFilter::new(agent_decision_topic.clone(), QoS::AtLeastOnce),
                            ];
                            let _ = client.subscribe_many(filters).await;

                            connected_instant = Some(Instant::now());
                            suback_tasks_ok = false;
                            pruned = false;
                            seen_task_ids.clear();
                            retained_tasks_count = 0;
                        }
                        Ok(Event::Incoming(Incoming::SubAck(suback))) => {
                            // Check if tasks subscription succeeded
                            let failed = suback.return_codes.iter().any(|c| *c == rumqttc::SubscribeReasonCode::Failure);
                            if !failed {
                                suback_tasks_ok = true;
                            }
                        }
                        Ok(Event::Incoming(Incoming::Publish(publish))) => {
                            let topic = publish.topic.clone();
                            let payload = publish.payload.to_vec();

                            if topic == presence_board_topic {
                                let s = String::from_utf8_lossy(&payload);
                                state.set_board_online(s.trim() == "1");
                            } else if let Some(entity_id) = topic.strip_prefix(&task_topic_prefix) {
                                if payload.is_empty() {
                                    callbacks.on_task_delete(entity_id);
                                } else {
                                    seen_task_ids.insert(entity_id.to_string());
                                    if !pruned {
                                        retained_tasks_count += 1;
                                    }
                                    if let Ok(env_str) = String::from_utf8(payload) {
                                        if let Some((from, plain_json)) = decrypt_incoming_envelope(&key, &env_str, &state) {
                                            if from != "lumen" {
                                                callbacks.on_task_upsert(entity_id, &plain_json);
                                            }
                                        }
                                    }
                                }
                            } else if topic == media_action_topic {
                                if let Ok(env_str) = String::from_utf8(payload) {
                                    if let Some((from, plain_json)) = decrypt_incoming_envelope(&key, &env_str, &state) {
                                        if from != "lumen" {
                                            if let Ok(action_req) = serde_json::from_str::<MediaActionPayload>(&plain_json) {
                                                if let Some(media_resp) = callbacks.on_media_action(&action_req.action) {
                                                    if let Ok(resp_json) = serde_json::to_string(&media_resp) {
                                                        if let Ok(resp_env) = seal_mqtt_envelope(&key, "lumen", &resp_json) {
                                                            let _ = client.publish(media_state_topic.clone(), QoS::AtLeastOnce, true, resp_env.into_bytes()).await;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if topic == agent_decision_topic {
                                if let Ok(env_str) = String::from_utf8(payload) {
                                    if let Some((from, plain_json)) = decrypt_incoming_envelope(&key, &env_str, &state) {
                                        if from != "lumen" {
                                            callbacks.on_agent_decision(&from, &plain_json);
                                        }
                                    }
                                }
                            }
                        }
                        Ok(Event::Incoming(Incoming::Disconnect)) => {
                            is_connected = false;
                            state.set_connected(false);
                            suback_tasks_ok = false;
                            pruned = false;
                            connected_instant = None;
                        }
                        Err(err) => {
                            is_connected = false;
                            state.set_connected(false);
                            state.set_last_error(Some(format!("Erro MQTT: {err}")));
                            suback_tasks_ok = false;
                            pruned = false;
                            connected_instant = None;
                            // Evita loop apertado quando a rede ou o broker falham; Shutdown ainda interrompe.
                            tokio::select! {
                                cmd = rx.recv() => {
                                    if matches!(cmd, Some(MqttCommand::Shutdown { .. }) | None) {
                                        break;
                                    }
                                }
                                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                            }
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)), if is_connected && suback_tasks_ok && !pruned => {
                    if let Some(ci) = connected_instant {
                        if ci.elapsed() >= settle_duration {
                            pruned = true;
                            callbacks.on_settle_prune(&seen_task_ids, retained_tasks_count);
                        }
                    }
                }
            }
        }
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyCallbacks;
    impl MqttCallbacks for DummyCallbacks {
        fn on_task_upsert(&self, _entity_id: &str, _plain_json: &str) {}
        fn on_task_delete(&self, _entity_id: &str) {}
        fn on_settle_prune(&self, _seen_task_ids: &HashSet<String>, _retained_count: usize) {}
        fn on_media_action(&self, _action: &str) -> Option<MediaStatePayload> {
            Some(MediaStatePayload {
                playing: true,
                title: Some("Song".into()),
                artist: Some("Artist".into()),
            })
        }
    }

    #[test]
    fn test_parse_broker_uri() {
        let (host, port, tls) = parse_broker_uri("mqtts://broker.example.com:8883").unwrap();
        assert_eq!(host, "broker.example.com");
        assert_eq!(port, 8883);
        assert!(tls);

        let (host2, port2, tls2) = parse_broker_uri("mqtts://hivemq.cloud").unwrap();
        assert_eq!(host2, "hivemq.cloud");
        assert_eq!(port2, 8883);
        assert!(tls2);

        let (host3, port3, tls3) = parse_broker_uri("mqtt://127.0.0.1:1883").unwrap();
        assert_eq!(host3, "127.0.0.1");
        assert_eq!(port3, 1883);
        assert!(!tls3);
    }

    #[test]
    fn test_mqtt_runtime_state_snapshots() {
        let state = MqttRuntimeState::new();
        let snap0 = state.snapshot();
        assert!(!snap0.configured);
        assert!(!snap0.connected);
        assert!(!snap0.board_online);

        state.set_configured("broker.com".into(), "room123".into());
        state.set_connected(true);
        state.set_board_online(true);
        let snap1 = state.snapshot();
        assert!(snap1.configured);
        assert!(snap1.connected);
        assert!(snap1.board_online);
        assert_eq!(snap1.host, "broker.com");
        assert_eq!(snap1.room, "room123");

        state.set_connected(false);
        let snap2 = state.snapshot();
        assert!(!snap2.connected);
        assert!(!snap2.board_online);
    }

    fn start_embedded_broker() -> u16 {
        let sock = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = sock.local_addr().unwrap().port();
        drop(sock);

        let mut v4 = std::collections::HashMap::new();
        v4.insert(
            "v4-1".to_string(),
            rumqttd::ServerSettings {
                name: "v4-1".to_string(),
                listen: format!("127.0.0.1:{port}").parse().unwrap(),
                tls: None,
                next_connection_delay_ms: 1,
                connections: rumqttd::ConnectionSettings {
                    connection_timeout_ms: 5000,
                    max_payload_size: 65536,
                    max_inflight_count: 100,
                    auth: None,
                    external_auth: None,
                    dynamic_filters: false,
                },
            },
        );
        let config = rumqttd::Config {
            id: 0,
            router: rumqttd::RouterConfig {
                max_connections: 16,
                max_outgoing_packet_count: 200,
                max_segment_size: 1024 * 1024,
                max_segment_count: 4,
                ..Default::default()
            },
            v4: Some(v4),
            ..Default::default()
        };
        std::thread::spawn(move || {
            let mut broker = rumqttd::Broker::new(config);
            let _ = broker.start();
        });
        // Aguarda porta TCP abrir
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        port
    }

    struct RecordingCallbacks {
        upserts: std::sync::Mutex<Vec<(String, String)>>,
        deletes: std::sync::Mutex<Vec<String>>,
        media_actions: std::sync::Mutex<Vec<String>>,
        settles: std::sync::Mutex<Vec<(HashSet<String>, usize)>>,
        decisions: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl RecordingCallbacks {
        fn new() -> Self {
            Self {
                upserts: std::sync::Mutex::new(Vec::new()),
                deletes: std::sync::Mutex::new(Vec::new()),
                media_actions: std::sync::Mutex::new(Vec::new()),
                settles: std::sync::Mutex::new(Vec::new()),
                decisions: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl MqttCallbacks for RecordingCallbacks {
        fn on_task_upsert(&self, entity_id: &str, plain_json: &str) {
            self.upserts
                .lock()
                .unwrap()
                .push((entity_id.to_string(), plain_json.to_string()));
        }
        fn on_task_delete(&self, entity_id: &str) {
            self.deletes.lock().unwrap().push(entity_id.to_string());
        }
        fn on_settle_prune(&self, seen_task_ids: &HashSet<String>, retained_count: usize) {
            self.settles
                .lock()
                .unwrap()
                .push((seen_task_ids.clone(), retained_count));
        }
        fn on_media_action(&self, action: &str) -> Option<MediaStatePayload> {
            self.media_actions.lock().unwrap().push(action.to_string());
            Some(MediaStatePayload {
                playing: true,
                title: Some("Faixa Teste".into()),
                artist: Some("Artista Teste".into()),
            })
        }
        fn on_agent_decision(&self, from: &str, plain_json: &str) {
            self.decisions
                .lock()
                .unwrap()
                .push((from.to_string(), plain_json.to_string()));
        }
    }

    #[tokio::test]
    async fn mqtt_end_to_end_with_embedded_rumqttd_broker() {
        let port = start_embedded_broker();
        let room = "abc123def456";
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // 1. Peer simulando a placa ESP32
        let mut board_opts = MqttOptions::new(format!("placa-{room}"), "127.0.0.1", port);
        board_opts.set_keep_alive(Duration::from_secs(30));
        let (board_client, mut board_el) = AsyncClient::new(board_opts, 64);
        let board_rx = Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
        let board_rx_task = board_rx.clone();
        let board_loop = tokio::spawn(async move {
            loop {
                match board_el.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        board_rx_task
                            .lock()
                            .unwrap()
                            .push((p.topic, p.payload.to_vec()));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        board_client
            .subscribe(format!("sm/{room}/#"), QoS::AtLeastOnce)
            .await
            .unwrap();
        // Publica presença da placa = "1" e uma tarefa retida antes do Lumen conectar
        board_client
            .publish(
                format!("sm/{room}/presenca/placa"),
                QoS::AtLeastOnce,
                true,
                b"1".to_vec(),
            )
            .await
            .unwrap();
        let board_task_plain = r#"{"contract_version":1,"entity_type":"todo","entity_id":"esp-1","origin_role":"esp32","updated_at":1764240100,"payload":{"id":"esp-1","text":"Tarefa da placa","done":false}}"#;
        let board_task_env = seal_mqtt_envelope(key, "placa", board_task_plain).unwrap();
        board_client
            .publish(
                format!("sm/{room}/tarefas/esp-1"),
                QoS::AtLeastOnce,
                true,
                board_task_env.into_bytes(),
            )
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(150)).await;

        // 2. Sobe o loop MQTT do Lumen apontando para o broker embutido
        let state = Arc::new(MqttRuntimeState::new());
        let cb = Arc::new(RecordingCallbacks::new());
        let cfg = MqttConfig {
            uri: format!("mqtt://127.0.0.1:{port}"),
            user: String::new(),
            pass: String::new(),
            room: room.to_string(),
            key: key.to_string(),
        };
        let handle = spawn_mqtt_loop(cfg, cb.clone(), state.clone(), Duration::from_millis(300))
            .expect("spawn mqtt loop");

        // Aguarda conexão, presença da placa, recebimento da retida e janela de settle
        for _ in 0..40 {
            if state.connected.load(Ordering::Relaxed)
                && state.board_online.load(Ordering::Relaxed)
                && !cb.settles.lock().unwrap().is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert!(state.connected.load(Ordering::Relaxed));
        assert!(state.board_online.load(Ordering::Relaxed));
        assert_eq!(cb.upserts.lock().unwrap().len(), 1);
        assert_eq!(cb.upserts.lock().unwrap()[0].0, "esp-1");
        let settles = cb.settles.lock().unwrap().clone();
        assert_eq!(settles.len(), 1);
        assert!(settles[0].0.contains("esp-1"));
        assert_eq!(settles[0].1, 1);

        // 3. Tarefa criada no Lumen publicada como retida -> placa recebe e decifra
        let lumen_task_plain = r#"{"contract_version":1,"entity_type":"todo","entity_id":"lum-1","origin_role":"lumen","updated_at":1764240200,"payload":{"id":"lum-1","text":"Tarefa do PC","done":false}}"#;
        handle
            .publish_task("lum-1".into(), lumen_task_plain.into())
            .await
            .unwrap();

        // 4. Apagar nos dois sentidos: Lumen apaga lum-1, placa apaga esp-1
        tokio::time::sleep(Duration::from_millis(150)).await;
        handle.delete_task("lum-1".into()).await.unwrap();
        board_client
            .publish(
                format!("sm/{room}/tarefas/esp-1"),
                QoS::AtLeastOnce,
                true,
                Vec::<u8>::new(),
            )
            .await
            .unwrap();

        // 5. Mídia: placa envia "next" cifrado -> Lumen chama callback e publica pc/midia/estado
        let media_cmd = seal_mqtt_envelope(key, "placa", r#"{"action":"next"}"#).unwrap();
        board_client
            .publish(
                format!("sm/{room}/pc/midia"),
                QoS::AtLeastOnce,
                false,
                media_cmd.into_bytes(),
            )
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(300)).await;

        assert!(cb.deletes.lock().unwrap().contains(&"esp-1".to_string()));
        assert_eq!(cb.media_actions.lock().unwrap().as_slice(), &["next"]);

        // Verifica mensagens recebidas pelo peer da placa
        let received = board_rx.lock().unwrap().clone();
        assert!(received
            .iter()
            .any(|(t, p)| t == &format!("sm/{room}/presenca/lumen") && p == b"1"));

        let lum_pub = received
            .iter()
            .find(|(t, p)| t == &format!("sm/{room}/tarefas/lum-1") && !p.is_empty())
            .expect("placa recebeu tarefa retida do Lumen");
        let (from, plain) =
            open_mqtt_envelope(key, std::str::from_utf8(&lum_pub.1).unwrap()).unwrap();
        assert_eq!(from, "lumen");
        assert!(plain.contains("Tarefa do PC"));

        assert!(received
            .iter()
            .any(|(t, p)| t == &format!("sm/{room}/tarefas/lum-1") && p.is_empty()));

        let media_state_msg = received
            .iter()
            .find(|(t, p)| t == &format!("sm/{room}/pc/midia/estado") && !p.is_empty())
            .expect("placa recebeu estado de midia");
        let (mfrom, mplain) =
            open_mqtt_envelope(key, std::str::from_utf8(&media_state_msg.1).unwrap()).unwrap();
        assert_eq!(mfrom, "lumen");
        assert!(mplain.contains("\"playing\":true"));
        assert!(mplain.contains("Faixa Teste"));

        // Encerra cliente limpo
        let _ = handle.shutdown().await;
        board_loop.abort();
    }

    #[test]
    fn revoked_key_triggers_expected_error_message_and_recovers() {
        let state = MqttRuntimeState::new();
        let old_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let new_key = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

        let env_from_new = seal_mqtt_envelope(new_key, "placa", r#"{"action":"next"}"#).unwrap();
        assert!(decrypt_incoming_envelope(old_key, &env_from_new, &state).is_none());
        assert_eq!(
            state.snapshot().last_error.as_deref(),
            Some(REVOKED_KEY_ERROR_MSG)
        );

        // Após atualizar a chave para a nova, o erro de chave revogada limpa.
        let opened = decrypt_incoming_envelope(new_key, &env_from_new, &state);
        assert!(opened.is_some());
        assert_eq!(state.snapshot().last_error, None);
    }

    #[tokio::test]
    async fn mqtt_agent_decision_and_telemetry_e2e_with_embedded_broker() {
        use crate::esp32_bridge::interactions::{InteractionStore, Verdict};
        use crate::esp32_bridge::{spawn_telemetry_loop, Esp32Runtime, RuntimeMqttCallbacks};
        use serde_json::{json, Value};

        let port = start_embedded_broker();
        let room = "roomagent999";
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // 1. Clientes "placa" e "celular"
        let mut placa_opts = MqttOptions::new(format!("placa-{room}"), "127.0.0.1", port);
        placa_opts.set_keep_alive(Duration::from_secs(30));
        let (placa_client, mut placa_el) = AsyncClient::new(placa_opts, 64);
        let placa_msgs = Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
        let placa_msgs_task = placa_msgs.clone();
        let placa_loop = tokio::spawn(async move {
            loop {
                match placa_el.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        placa_msgs_task
                            .lock()
                            .unwrap()
                            .push((p.topic, p.payload.to_vec()));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
        placa_client
            .subscribe(format!("sm/{room}/#"), QoS::AtLeastOnce)
            .await
            .unwrap();

        let mut cel_opts = MqttOptions::new(format!("celular-{room}"), "127.0.0.1", port);
        cel_opts.set_keep_alive(Duration::from_secs(30));
        let (cel_client, mut cel_el) = AsyncClient::new(cel_opts, 64);
        let cel_loop = tokio::spawn(async move {
            loop {
                if cel_el.poll().await.is_err() {
                    break;
                }
            }
        });
        cel_client
            .subscribe(format!("sm/{room}/#"), QoS::AtLeastOnce)
            .await
            .unwrap();

        // 2. Sobe o Lumen MQTT
        let rt = Esp32Runtime::new(None);
        let state = rt.mqtt_state.clone();
        let cb = Arc::new(RuntimeMqttCallbacks { rt: rt.clone() });
        let cfg = MqttConfig {
            uri: format!("mqtt://127.0.0.1:{port}"),
            user: String::new(),
            pass: String::new(),
            room: room.to_string(),
            key: key.to_string(),
        };
        let handle = spawn_mqtt_loop(cfg, cb, state.clone(), Duration::from_millis(100)).unwrap();
        *rt.mqtt_handle.lock().unwrap() = Some(handle.clone());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        spawn_telemetry_loop(rt.clone(), shutdown_rx);

        // Aguarda Lumen conectar
        for _ in 0..50 {
            if state.connected.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        assert!(state.connected.load(Ordering::Relaxed));

        // 3. Estaciona pedido em rt.interactions
        let q_event = json!({
            "session_id": "sess-1",
            "cwd": "C:\\Users\\dev\\projeto-teste",
            "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {
                "questions": [{
                    "question": "Confirmar operação",
                    "header": "Auth",
                    "multiSelect": false,
                    "options": [
                        {"label": "Executar (recommended)"},
                        {"label": "Cancelar"}
                    ]
                }]
            }
        });
        let parked = rt
            .interactions
            .park_claude_question(
                &q_event,
                120,
                InteractionStore::now_ms(),
            )
            .expect("parked question");

        let pedido_topic = format!("sm/{room}/pc/agentes/pedido");
        let decisao_topic = format!("sm/{room}/pc/agentes/decisao");

        // Verifica que placa recebeu pedido cifrado retido
        let mut got_pedido = false;
        for _ in 0..60 {
            let msgs = placa_msgs.lock().unwrap().clone();
            for (t, p) in msgs {
                if t == pedido_topic && !p.is_empty() {
                    let env_str = String::from_utf8(p).unwrap();
                    let (from, plain) = open_mqtt_envelope(key, &env_str).unwrap();
                    assert_eq!(from, "lumen");
                    let val: Value = serde_json::from_str(&plain).unwrap();
                    assert_eq!(val["request_id"], parked.request_id);
                    assert_eq!(val["view_sha256"], parked.view_sha256);
                    got_pedido = true;
                    break;
                }
            }
            if got_pedido {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(got_pedido, "placa deve receber pc/agentes/pedido cifrado");

        // 4. Publica decisão "approve" de "placa" e logo em seguida "deny" de "celular"
        let now_sec = Esp32Runtime::now_secs();
        let approve_plain = json!({
            "request_id": parked.request_id,
            "view_sha256": parked.view_sha256,
            "verdict": "approve",
            "ts": now_sec
        })
        .to_string();
        let approve_env = seal_mqtt_envelope(key, "placa", &approve_plain).unwrap();
        placa_client
            .publish(
                decisao_topic.clone(),
                QoS::AtLeastOnce,
                false,
                approve_env.into_bytes(),
            )
            .await
            .unwrap();

        let deny_plain = json!({
            "request_id": parked.request_id,
            "view_sha256": parked.view_sha256,
            "verdict": "deny",
            "ts": now_sec
        })
        .to_string();
        let deny_env = seal_mqtt_envelope(key, "celular", &deny_plain).unwrap();
        cel_client
            .publish(
                decisao_topic.clone(),
                QoS::AtLeastOnce,
                false,
                deny_env.into_bytes(),
            )
            .await
            .unwrap();

        // 5. Verifica que placa venceu (waiter recebe Approve)
        let waiter_res = tokio::time::timeout(Duration::from_secs(3), parked.rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(waiter_res, Some(Verdict::Approve));

        // Verifica que pc/agentes/pedido recebe payload vazio retido
        let mut got_empty_pedido = false;
        for _ in 0..60 {
            let msgs = placa_msgs.lock().unwrap().clone();
            if let Some((_, p)) = msgs.iter().rev().find(|(t, _)| t == &pedido_topic) {
                if p.is_empty() {
                    got_empty_pedido = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(got_empty_pedido, "pedido deve ser limpo com payload vazio retido");

        // 6. Testa handle.shutdown() verificando pc/agentes/pedido vazio e pc/agentes/status com active_count: 0
        let status_topic = format!("sm/{room}/pc/agentes/status");
        handle.shutdown().await.unwrap();
        let _ = shutdown_tx.send(true);

        tokio::time::sleep(Duration::from_millis(200)).await;
        let msgs = placa_msgs.lock().unwrap().clone();
        let last_pedido = msgs.iter().rev().find(|(t, _)| t == &pedido_topic).unwrap();
        assert!(last_pedido.1.is_empty(), "shutdown deve publicar pedido vazio");

        let last_status = msgs.iter().rev().find(|(t, _)| t == &status_topic).unwrap();
        let env_str = String::from_utf8(last_status.1.clone()).unwrap();
        let (from, plain) = open_mqtt_envelope(key, &env_str).unwrap();
        assert_eq!(from, "lumen");
        let val: Value = serde_json::from_str(&plain).unwrap();
        assert_eq!(val["agents"]["claude"]["active_count"], 0);
        assert_eq!(val["agents"]["codex"]["active_count"], 0);

        placa_loop.abort();
        cel_loop.abort();
    }
}
