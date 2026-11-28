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
    assert!(tok_json.get("claudeForecastState").is_some());
    assert!(tok_json.get("codexForecastState").is_some());
    assert!(tok_json.get("claudeModelWeekPct").is_some());

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
    let pipe_esp_id = rt
        .interactions
        .park_pipe_or_legacy(
            Some("pipe-99".into()),
            "Claude",
            "/repo/lumen",
            "Bash",
            "echo hi",
            super::interactions::HOLD_SECS,
            super::interactions::InteractionStore::now_ms(),
        )
        .unwrap();
    rt.interactions
        .clear_by_pipe_id("pipe-99", super::interactions::InteractionStore::now_ms());
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

#[tokio::test]
async fn loopback_agent_status_v2_endpoint() {
    let (base, _key, _rt, tx) = spawn_loopback_server(true, true).await;
    let client = reqwest::Client::new();

    let r1 = client
        .get(format!("{base}/api/agent-status"))
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);
    let j1: Value = r1.json().await.unwrap();
    assert_eq!(j1["v"], 2);
    let seq1 = j1["seq"].as_u64().unwrap();
    assert!(j1["agents"]["claude"]["jobs"].is_array());
    assert!(j1["agents"]["codex"]["jobs"].is_array());

    let r2 = client
        .get(format!("{base}/api/agent-status"))
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200);
    let j2: Value = r2.json().await.unwrap();
    assert!(j2["seq"].as_u64().unwrap() > seq1);

    let _ = tx.send(true);
}

#[tokio::test]
async fn loopback_max_tracker_endpoint() {
    let (base, _key, _rt, tx) = spawn_loopback_server(true, true).await;
    let client = reqwest::Client::new();

    let r = client
        .get(format!("{base}/api/max-tracker"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let j: Value = r.json().await.unwrap();
    assert_eq!(j["v"], 1);
    assert_eq!(j["weeks"], 20);
    assert!(j["claude"].is_object());
    assert!(j["codex"].is_object());
    assert_eq!(j["claude"]["days"].as_array().unwrap().len(), 140);
    assert_eq!(j["claude"]["weekMaxed"].as_array().unwrap().len(), 20);

    let _ = tx.send(true);
}

#[tokio::test]
async fn legacy_secrets_kept_until_mqtt_saved_then_migrated_and_cleared() {
    let _guard = PAUSED_GUARD.lock().await;
    crate::integrations::PAUSED.store(false, Ordering::SeqCst);
    let (base, legacy_key, rt, tx) = spawn_loopback_server(false, true).await;
    let legacy_key = legacy_key.unwrap();
    let client = reqwest::Client::new();

    // Antes de salvar código MQTT: chave legada ainda autentica /v1/voice.
    let before = client
        .post(format!("{base}/v1/voice"))
        .bearer_auth(&legacy_key)
        .json(&json!({ "text": "antes da migracao", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(before.status(), 200);

    // Após salvar código MQTT: migração limpa chaves legadas e ativa bearer HMAC de voz.
    let mut cleared = false;
    let room_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    rt.complete_mqtt_migration_with(room_key, || {
        cleared = true;
    });
    assert!(cleared);
    assert!(rt.keys.lock().unwrap().is_empty());

    let legacy_after = client
        .post(format!("{base}/v1/voice"))
        .bearer_auth(&legacy_key)
        .json(&json!({ "text": "legado", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(legacy_after.status(), 401);

    let hmac_after = client
        .post(format!("{base}/v1/voice"))
        .bearer_auth(crypto::voice_bearer_hex(room_key))
        .json(&json!({ "text": "migrado", "mock": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(hmac_after.status(), 200);

    let _ = tx.send(true);
}

#[tokio::test]
async fn l11_2_loopback_hook_guards_and_end_to_end_interactions() {
    use super::interactions;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (base, _key, rt, tx) = spawn_loopback_server(true, false).await;
    let room_key = "sala-teste-0123456789abcdef";
    *rt.mqtt_room_key.lock().unwrap() = Some(room_key.to_string());
    *rt.mqtt_voice_bearer.lock().unwrap() = Some(crypto::voice_bearer_hex(room_key));

    let port: u16 = base
        .rsplit_once(':')
        .unwrap()
        .1
        .parse()
        .unwrap();
    let client = reqwest::Client::new();

    // 1. Hook guards: Host estranho -> 403; Origin presente -> 403; Content-Type text/plain -> 415
    for path in [
        "/api/hook/question",
        "/api/hook/permission",
        "/api/codex/question",
        "/api/codex/permission",
    ] {
        let bad_host = client
            .post(format!("{base}{path}"))
            .header("Host", "attacker.example")
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_host.status(), 403, "expected 403 for bad Host on {path}");

        let bad_origin = client
            .post(format!("{base}{path}"))
            .header("Origin", "https://attacker.example")
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_origin.status(), 403, "expected 403 for Origin on {path}");

        let bad_ct = client
            .post(format!("{base}{path}"))
            .header("Content-Type", "text/plain")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_ct.status(), 415, "expected 415 for text/plain on {path}");
    }

    // IP não-loopback na validação direta -> 403
    assert!(!server::is_loopback_ip("192.168.1.20".parse().unwrap()));
    assert!(server::is_loopback_ip("127.0.0.1".parse().unwrap()));

    // 2. Claude PreToolUse (AskUserQuestion) fica segurado até POST /api/interaction/<id> com HMAC v2
    let base_cl = base.clone();
    let hook_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{base_cl}/api/hook/question"))
            .json(&json!({
                "session_id": "s-claude",
                "cwd": "C:\\projetos\\lumen",
                "hook_event_name": "PreToolUse",
                "tool_name": "AskUserQuestion",
                "tool_input": {
                    "questions": [{
                        "question": "Qual abordagem usar?",
                        "header": "Auth",
                        "multiSelect": false,
                        "options": [
                            { "label": "Manter atual", "description": "Simples" },
                            { "label": "Nova camada (Recommended)", "description": "Mais limpa" }
                        ]
                    }]
                }
            }))
            .send()
            .await
            .unwrap()
    });

    // Aguarda pending aparecer no /api/agent-status
    let mut pending_val = None;
    for _ in 0..40 {
        let st: Value = client
            .get(format!("{base}/api/agent-status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(p) = st.get("pending") {
            pending_val = Some(p.clone());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let pending = pending_val.expect("pending presente em /api/agent-status");
    let req_id = pending["request_id"].as_str().unwrap().to_string();
    let view_sha = pending["view_sha256"].as_str().unwrap().to_string();
    assert_eq!(pending["provider"], "claude");
    assert_eq!(pending["kind"], "question");
    assert_eq!(pending["can_approve"], true);
    assert_eq!(pending["title"], "Nova camada");

    let ts = Esp32Runtime::now_secs() as i64;
    let mac = interactions::sign_answer_v2(
        room_key,
        "claude",
        &req_id,
        &view_sha,
        "approve",
        ts,
    )
    .unwrap();
    let ans = client
        .post(format!("{base}/api/interaction/{req_id}"))
        .json(&json!({
            "provider": "claude",
            "view_sha256": view_sha,
            "verdict": "approve",
            "ts": ts,
            "hmac": mac
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(ans.status(), 200);

    let hook_resp = hook_task.await.unwrap();
    assert_eq!(hook_resp.status(), 200);
    let hook_json: Value = hook_resp.json().await.unwrap();
    assert_eq!(
        hook_json["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        hook_json["hookSpecificOutput"]["updatedInput"]["answers"]["Qual abordagem usar?"],
        "Nova camada (Recommended)"
    );

    // 3. Codex MCP question (/api/codex/question) + panic (/api/panic) -> status computer deny
    let base_cx = base.clone();
    let codex_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{base_cx}/api/codex/question"))
            .json(&json!({
                "cwd": "/repo/lumen",
                "session_id": "s-codex",
                "turn_id": "t-1",
                "question": "Rodar agora?",
                "options": [
                    { "label": "Sim", "recommended": true },
                    { "label": "Não" }
                ]
            }))
            .send()
            .await
            .unwrap()
    });

    for _ in 0..40 {
        if rt
            .interactions
            .pending_count(interactions::InteractionStore::now_ms())
            == 1
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let ts_p = Esp32Runtime::now_secs() as i64;
    let panic_mac = interactions::sign_panic(room_key, ts_p).unwrap();
    let panic_resp = client
        .post(format!("{base}/api/panic"))
        .json(&json!({ "verdict": "deny", "ts": ts_p, "hmac": panic_mac }))
        .send()
        .await
        .unwrap();
    assert_eq!(panic_resp.status(), 200);
    let panic_json: Value = panic_resp.json().await.unwrap();
    assert_eq!(panic_json["denied"], 1);

    let codex_resp = codex_task.await.unwrap();
    let codex_json: Value = codex_resp.json().await.unwrap();
    assert_eq!(codex_json["status"], "computer");
    assert_eq!(codex_json["reason"], "deny");

    // 4. Cliente caído: conecta, envia POST /api/codex/permission e fecha socket -> vaga liberada em <= 2s
    let perm_body = serde_json::to_vec(&json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "s-ghost",
        "turn_id": "t-ghost",
        "cwd": "/repo/lumen",
        "tool_name": "Shell",
        "tool_input": { "command": "cargo test" }
    }))
    .unwrap();
    let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let raw_req = format!(
        "POST /api/codex/permission HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        perm_body.len()
    );
    sock.write_all(raw_req.as_bytes()).await.unwrap();
    sock.write_all(&perm_body).await.unwrap();
    sock.flush().await.unwrap();

    for _ in 0..40 {
        if rt
            .interactions
            .pending_count(interactions::InteractionStore::now_ms())
            == 1
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(
        rt.interactions
            .pending_count(interactions::InteractionStore::now_ms()),
        1
    );
    drop(sock);
    let mut freed = false;
    for _ in 0..60 {
        if rt
            .interactions
            .pending_count(interactions::InteractionStore::now_ms())
            == 0
        {
            freed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(freed, "cliente caído deve liberar a vaga dentro de 2s");

    // 5. Decisão via MQTT (pc/agentes/decisao) resolve o pedido estacionado
    let ev_mqtt = json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "s-mqtt",
        "turn_id": "t-mqtt",
        "cwd": "/repo/lumen",
        "tool_name": "Shell",
        "tool_input": { "command": "cargo build" }
    });
    let base_mq = base.clone();
    let mq_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{base_mq}/api/codex/permission"))
            .json(&ev_mqtt)
            .send()
            .await
            .unwrap()
    });
    let mut mq_pending = None;
    for _ in 0..40 {
        if let Some(p) = rt
            .interactions
            .pending_public(interactions::InteractionStore::now_ms())
        {
            mq_pending = Some(p);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let mq_p = mq_pending.unwrap();
    let mq_dec = json!({
        "request_id": mq_p["request_id"],
        "view_sha256": mq_p["view_sha256"],
        "verdict": "approve",
        "ts": Esp32Runtime::now_secs() as i64
    })
    .to_string();
    super::handle_mqtt_agent_decision(&rt, "placa", &mq_dec);
    let mq_resp: Value = mq_task.await.unwrap().json().await.unwrap();
    assert_eq!(
        mq_resp["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );

    // 6. Terminal ("terminal") retorna corpo vazio no permission hook (silêncio nunca aprova)
    let mut raw_sock = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let term_req = format!(
        "POST /api/hook/permission HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        perm_body.len()
    );
    raw_sock.write_all(term_req.as_bytes()).await.unwrap();
    raw_sock.write_all(&perm_body).await.unwrap();
    raw_sock.flush().await.unwrap();

    let mut term_pending = None;
    for _ in 0..40 {
        if let Some(p) = rt
            .interactions
            .pending_public(interactions::InteractionStore::now_ms())
        {
            term_pending = Some(p);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let tp = term_pending.unwrap();
    let ts_t = Esp32Runtime::now_secs() as i64;
    let mac_t = interactions::sign_answer_v2(
        room_key,
        "claude",
        tp["request_id"].as_str().unwrap(),
        tp["view_sha256"].as_str().unwrap(),
        "terminal",
        ts_t,
    )
    .unwrap();
    let resp_t = client
        .post(format!("{base}/api/interaction/{}", tp["request_id"].as_str().unwrap()))
        .json(&json!({
            "provider": "claude",
            "view_sha256": tp["view_sha256"],
            "verdict": "terminal",
            "ts": ts_t,
            "hmac": mac_t
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_t.status(), 200);

    let mut raw_out = Vec::new();
    raw_sock.read_to_end(&mut raw_out).await.unwrap();
    let raw_str = String::from_utf8_lossy(&raw_out);
    assert!(raw_str.starts_with("HTTP/1.1 200 OK\r\n"));
    let body_after_headers = raw_str.split_once("\r\n\r\n").unwrap().1;
    assert!(body_after_headers.is_empty(), "terminal deve retornar corpo vazio");

    let _ = tx.send(true);
}
