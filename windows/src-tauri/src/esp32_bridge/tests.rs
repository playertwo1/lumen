use std::sync::atomic::Ordering;
use std::sync::Arc;

use base64::Engine;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{watch, Mutex};

use super::{crypto, server, Esp32Runtime, PairedDeviceKey};

static PAUSED_GUARD: Mutex<()> = Mutex::const_new(());

async fn spawn_loopback_server(
    agent_panel: bool,
    with_key: bool,
) -> (String, Option<String>, Arc<Esp32Runtime>, watch::Sender<bool>) {
    let rt = Esp32Runtime::new(None);
    rt.keys.lock().unwrap().clear();
    rt.agent_panel_enabled.store(agent_panel, Ordering::SeqCst);

    let key = if with_key {
        let k = crypto::generate_bridge_key_hex().unwrap();
        rt.keys.lock().unwrap().push(PairedDeviceKey {
            device_id: "test-esp".into(),
            key: k.clone(),
            created_at: 1,
        });
        Some(k)
    } else {
        None
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = watch::channel(false);
    let rt_srv = rt.clone();
    tokio::spawn(async move {
        server::serve_listener(listener, rt_srv, rx).await;
    });

    (format!("http://127.0.0.1:{port}"), key, rt, tx)
}

#[tokio::test]
async fn loopback_ping_and_voice_auth_contract() {
    let _guard = PAUSED_GUARD.lock().await;
    crate::integrations::PAUSED.store(false, Ordering::SeqCst);
    let (base, key, _rt, tx) = spawn_loopback_server(false, true).await;
    let key = key.unwrap();
    let client = reqwest::Client::new();

    let ping_resp = client.get(format!("{base}/v1/ping")).send().await.unwrap();
    assert_eq!(ping_resp.status(), 200);
    let ping_json: Value = ping_resp.json().await.unwrap();
    assert_eq!(ping_json["ok"], true);
    assert_eq!(ping_json["service"], "lumen-bridge");

    let unauth = client
        .post(format!("{base}/v1/voice"))
        .json(&json!({ "text": "oi", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);

    let ok_voice = client
        .post(format!("{base}/v1/voice"))
        .bearer_auth(&key)
        .json(&json!({ "text": "oi lumen", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(ok_voice.status(), 200);
    let body_bytes = ok_voice.bytes().await.unwrap();
    assert!(body_bytes.len() < 96 * 1024);
    let v: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["sample_rate"], 22_050);
    let wav = base64::engine::general_purpose::STANDARD
        .decode(v["wav_b64"].as_str().unwrap())
        .unwrap();
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 22_050);

    let _ = tx.send(true);
}

#[tokio::test]
async fn loopback_removed_routes_return_404() {
    let (base, key, _rt, tx) = spawn_loopback_server(false, true).await;
    let key = key.unwrap();
    let client = reqwest::Client::new();

    for (path, body) in [
        ("/v1/pair", json!({ "code": "123456" })),
        ("/v1/tasks/sync", json!({ "envelopes": [] })),
        ("/v1/media", json!({ "action": "play_pause" })),
    ] {
        let r = client
            .post(format!("{base}{path}"))
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404, "expected 404 for removed route {path}");
    }

    let _ = tx.send(true);
}

#[tokio::test]
async fn loopback_api_panel_gates_tokens_agents_and_replay_409() {
    let client = reqwest::Client::new();

    // 1. Panel disabled -> 401
    let (base_off, _key_off, _rt_off, tx_off) = spawn_loopback_server(false, true).await;
    assert_eq!(
        client.get(format!("{base_off}/api/tokens")).send().await.unwrap().status(),
        401
    );
    let _ = tx_off.send(true);

    // 2. Panel enabled but unpaired -> 401
    let (base_nokey, _, _rt_nokey, tx_nokey) = spawn_loopback_server(true, false).await;
    assert_eq!(
        client.get(format!("{base_nokey}/api/agents")).send().await.unwrap().status(),
        401
    );
    let _ = tx_nokey.send(true);

    // 3. Panel enabled + paired -> 200 + full approval/replay/clear lifecycle
    let (base, _key, rt, tx) = spawn_loopback_server(true, true).await;

    let tok_resp = client.get(format!("{base}/api/tokens")).send().await.unwrap();
    assert_eq!(tok_resp.status(), 200);
    let tok_json: Value = tok_resp.json().await.unwrap();
    assert!(tok_json.get("claudeSessionPct").is_some());
    assert!(tok_json.get("codexSessionPct").is_some());
    assert!(tok_json.get("updatedAt").is_some());

    let ev_resp = client
        .post(format!("{base}/api/agents/event"))
        .json(&json!({
            "hook_event_name": "PermissionRequest",
            "agent": "Claude",
            "cwd": "C:/repo/lumen",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(ev_resp.status(), 200);
    let ev_json: Value = ev_resp.json().await.unwrap();
    let app_id = ev_json["id"].as_str().unwrap().to_string();

    let ag_resp = client.get(format!("{base}/api/agents")).send().await.unwrap();
    assert_eq!(ag_resp.status(), 200);
    let ag_json: Value = ag_resp.json().await.unwrap();
    assert_eq!(ag_json["pendingApproval"]["id"], app_id);

    let dec1 = client
        .post(format!("{base}/api/agents/decide"))
        .json(&json!({ "id": app_id, "decision": "allow" }))
        .send()
        .await
        .unwrap();
    assert_eq!(dec1.status(), 200);

    // Replay must return 409 Conflict
    let dec2 = client
        .post(format!("{base}/api/agents/decide"))
        .json(&json!({ "id": app_id, "decision": "deny" }))
        .send()
        .await
        .unwrap();
    assert_eq!(dec2.status(), 409);

    // PC-side settlement clears pending approval -> ESP32 gets 409
    let pipe_esp_id = rt.agent_panel.lock().unwrap().on_permission(
        Some("pipe-99".into()),
        "Claude",
        "/repo/lumen",
        "Bash",
        "echo hi",
        Esp32Runtime::now_secs(),
    );
    rt.agent_panel.lock().unwrap().clear_by_pipe_id("pipe-99");
    let dec_cleared = client
        .post(format!("{base}/api/agents/decide"))
        .json(&json!({ "id": pipe_esp_id, "decision": "allow" }))
        .send()
        .await
        .unwrap();
    assert_eq!(dec_cleared.status(), 409);

    let _ = tx.send(true);
}

#[tokio::test]
async fn loopback_voice_accepts_mqtt_hmac_bearer_vector() {
    let _guard = PAUSED_GUARD.lock().await;
    crate::integrations::PAUSED.store(false, Ordering::SeqCst);
    let (base, _legacy_key, rt, tx) = spawn_loopback_server(false, false).await;
    let room_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let expected_bearer = "3717157b1cbcf20a0ec4f22acbbe162bd033066c6528db618d6c7155a67f6e8b";
    assert_eq!(crypto::voice_bearer_hex(room_key), expected_bearer);
    *rt.mqtt_voice_bearer.lock().unwrap() = Some(crypto::voice_bearer_hex(room_key));

    let client = reqwest::Client::new();
    let bad = client
        .post(format!("{base}/v1/voice"))
        .bearer_auth("0000000000000000000000000000000000000000000000000000000000000000")
        .json(&json!({ "text": "oi", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 401);

    let ok = client
        .post(format!("{base}/v1/voice"))
        .bearer_auth(expected_bearer)
        .json(&json!({ "text": "oi via hmac", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let _ = tx.send(true);
}
