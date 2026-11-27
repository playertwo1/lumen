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
    SyncNow,
    Shutdown,
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

    pub async fn sync_now(&self) -> Result<(), String> {
        self.sender
            .send(MqttCommand::SyncNow)
            .await
            .map_err(|e| format!("Failed to send SyncNow command: {e}"))
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.sender
            .send(MqttCommand::Shutdown)
            .await
            .map_err(|e| format!("Failed to send Shutdown command: {e}"))
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
                        Some(MqttCommand::SyncNow) => {
                            // Can re-trigger prune callback if needed
                            if is_connected && suback_tasks_ok {
                                callbacks.on_settle_prune(&seen_task_ids, retained_tasks_count);
                            }
                        }
                        Some(MqttCommand::Shutdown) | None => {
                            let _ = client.publish(presence_lumen_topic.clone(), QoS::AtLeastOnce, true, "0".as_bytes().to_vec()).await;
                            let _ = client.disconnect().await;
                            state.set_connected(false);
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
                                        if let Ok((from, plain_json)) = open_mqtt_envelope(&key, &env_str) {
                                            if from != "lumen" {
                                                callbacks.on_task_upsert(entity_id, &plain_json);
                                            }
                                        }
                                    }
                                }
                            } else if topic == media_action_topic {
                                if let Ok(env_str) = String::from_utf8(payload) {
                                    if let Ok((from, plain_json)) = open_mqtt_envelope(&key, &env_str) {
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
                                    if matches!(cmd, Some(MqttCommand::Shutdown) | None) {
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
}
