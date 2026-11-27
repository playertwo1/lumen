//! Servidor HTTP/1.1 local (LAN) para a placa ESP32-S3 AMOLED.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::Esp32Runtime;

pub(crate) fn is_local(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                v4.is_loopback() || v4.is_private() || v4.is_link_local()
            } else {
                v6.is_loopback()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        }
    }
}

const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
const BODY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_VOICE_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_BODY_BYTES: usize = 64 * 1024;
const MAX_SMALL_BODY_BYTES: usize = 4 * 1024;

pub async fn serve_listener(
    listener: TcpListener,
    rt: Arc<Esp32Runtime>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(pair) => pair,
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                if !is_local(peer.ip()) {
                    drop(stream);
                    continue;
                }
                let rt_conn = rt.clone();
                tokio::spawn(async move {
                    let _ = stream.set_nodelay(true);
                    handle_connection(stream, rt_conn).await;
                });
            }
        }
    }
}

pub(crate) struct HttpRequest {
    pub method: String,
    pub path: String,
    #[allow(dead_code)]
    pub query: String,
    pub authorization: Option<String>,
    #[allow(dead_code)]
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

pub(crate) struct HttpResponse {
    pub status: u16,
    pub reason: &'static str,
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, reason: &'static str, payload: &Value) -> Self {
        Self {
            status,
            reason,
            body: serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec()),
        }
    }
}

fn max_body_for_path(path: &str) -> usize {
    match path {
        "/v1/voice" => MAX_VOICE_BODY_BYTES,
        "/v1/tasks/sync" => MAX_JSON_BODY_BYTES,
        p if p.starts_with("/api/") => MAX_JSON_BODY_BYTES,
        _ => MAX_SMALL_BODY_BYTES,
    }
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, HttpResponse> {
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 2048];
    let header_end;

    let read_headers = async {
        loop {
            if let Some(pos) = find_double_crlf(&buf) {
                return Ok(pos);
            }
            if buf.len() >= MAX_HEADER_BYTES {
                return Err(HttpResponse::json(
                    431,
                    "Request Header Fields Too Large",
                    &json!({ "ok": false, "error": "headers too large" }),
                ));
            }
            match stream.read(&mut tmp).await {
                Ok(0) => {
                    return Err(HttpResponse::json(
                        400,
                        "Bad Request",
                        &json!({ "ok": false, "error": "empty request" }),
                    ))
                }
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => {
                    return Err(HttpResponse::json(
                        400,
                        "Bad Request",
                        &json!({ "ok": false, "error": "read error" }),
                    ))
                }
            }
        }
    };

    header_end = match tokio::time::timeout(HEADER_TIMEOUT, read_headers).await {
        Ok(Ok(pos)) => pos,
        Ok(Err(resp)) => return Err(resp),
        Err(_) => {
            return Err(HttpResponse::json(
                408,
                "Request Timeout",
                &json!({ "ok": false, "error": "header timeout" }),
            ))
        }
    };

    let head_bytes = &buf[..header_end];
    let head_str = std::str::from_utf8(head_bytes).map_err(|_| {
        HttpResponse::json(400, "Bad Request", &json!({ "ok": false, "error": "invalid utf8" }))
    })?;
    let mut lines = head_str.split("\r\n");
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_uppercase();
    let raw_target = parts.next().unwrap_or("");
    let (path, query) = match raw_target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (raw_target.to_string(), String::new()),
    };

    let mut content_length: usize = 0;
    let mut authorization: Option<String> = None;
    let mut content_type: Option<String> = None;

    for line in lines {
        if let Some((name, val)) = line.split_once(':') {
            let k = name.trim().to_ascii_lowercase();
            let v = val.trim();
            match k.as_str() {
                "content-length" => {
                    content_length = v.parse::<usize>().map_err(|_| {
                        HttpResponse::json(
                            400,
                            "Bad Request",
                            &json!({ "ok": false, "error": "invalid content-length" }),
                        )
                    })?;
                }
                "authorization" => authorization = Some(v.to_string()),
                "content-type" => content_type = Some(v.to_string()),
                _ => {}
            }
        }
    }

    let cap = max_body_for_path(&path);
    if content_length > cap {
        return Err(HttpResponse::json(
            413,
            "Payload Too Large",
            &json!({ "ok": false, "error": "payload too large" }),
        ));
    }

    let already = buf.len().saturating_sub(header_end + 4);
    let mut body = Vec::with_capacity(content_length);
    if already > 0 {
        let take = already.min(content_length);
        body.extend_from_slice(&buf[header_end + 4..header_end + 4 + take]);
    }

    if body.len() < content_length {
        let mut remaining = content_length - body.len();
        let read_body = async {
            while remaining > 0 {
                let to_read = remaining.min(tmp.len());
                match stream.read(&mut tmp[..to_read]).await {
                    Ok(0) | Err(_) => return Err(()),
                    Ok(n) => {
                        body.extend_from_slice(&tmp[..n]);
                        remaining -= n;
                    }
                }
            }
            Ok(())
        };
        match tokio::time::timeout(BODY_TIMEOUT, read_body).await {
            Ok(Ok(())) => {}
            _ => {
                return Err(HttpResponse::json(
                    400,
                    "Bad Request",
                    &json!({ "ok": false, "error": "incomplete body" }),
                ))
            }
        }
    }

    Ok(HttpRequest {
        method,
        path,
        query,
        authorization,
        content_type,
        body,
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

async fn write_http_response(stream: &mut TcpStream, resp: HttpResponse) {
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.reason,
        resp.body.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(&resp.body).await;
    let _ = stream.flush().await;
    let _ = stream.shutdown().await;
}

async fn handle_connection(mut stream: TcpStream, rt: Arc<Esp32Runtime>) {
    let req = match read_http_request(&mut stream).await {
        Ok(r) => r,
        Err(resp) => {
            write_http_response(&mut stream, resp).await;
            return;
        }
    };
    let resp = dispatch_request(req, &rt).await;
    write_http_response(&mut stream, resp).await;
}

pub async fn dispatch_request(req: HttpRequest, rt: &Arc<Esp32Runtime>) -> HttpResponse {
    if req.method == "GET" && req.path == "/v1/ping" {
        rt.note_lan_activity();
        return HttpResponse::json(200, "OK", &json!({ "ok": true, "service": "lumen-bridge" }));
    }

    if req.method == "POST" && req.path == "/v1/pair" {
        let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
            return HttpResponse::json(400, "Bad Request", &json!({ "ok": false, "error": "invalid json" }));
        };
        let code = val.get("code").and_then(Value::as_str).unwrap_or("");
        let device_id = val.get("device_id").and_then(Value::as_str);
        match rt.pair_with_code(code, device_id) {
            Ok(Some(key)) => {
                rt.note_lan_activity();
                return HttpResponse::json(200, "OK", &json!({ "ok": true, "key": key }));
            }
            Ok(None) => {
                return HttpResponse::json(
                    401,
                    "Unauthorized",
                    &json!({ "ok": false, "error": "invalid or unarmed code" }),
                );
            }
            Err(_) => {
                return HttpResponse::json(
                    500,
                    "Internal Server Error",
                    &json!({ "ok": false, "error": "pairing failed" }),
                );
            }
        }
    }

    if req.path.starts_with("/v1/") {
        if !rt.verify_bearer(req.authorization.as_deref()) {
            return HttpResponse::json(
                401,
                "Unauthorized",
                &json!({ "ok": false, "error": "unauthorized" }),
            );
        }
        rt.note_lan_activity();
    }

    if req.method == "POST" && req.path == "/v1/voice" {
        return handle_voice(req, rt).await;
    }

    if req.method == "POST" && req.path == "/v1/tasks/sync" {
        return handle_tasks_sync(req, rt).await;
    }

    if req.method == "POST" && req.path == "/v1/media" {
        return handle_media(req, rt).await;
    }

    HttpResponse::json(404, "Not Found", &json!({ "ok": false, "error": "not found" }))
}

async fn handle_media(req: HttpRequest, rt: &Arc<Esp32Runtime>) -> HttpResponse {
    if crate::integrations::PAUSED.load(Ordering::Relaxed) {
        return HttpResponse::json(
            503,
            "Service Unavailable",
            &json!({ "ok": false, "error": "paused" }),
        );
    }
    let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "error": "invalid_json" }),
        );
    };
    let action = val
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !matches!(
        action.as_str(),
        "play_pause" | "next" | "prev" | "vol_up" | "vol_down"
    ) {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "error": "invalid_action" }),
        );
    }

    let dry_run = val.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    if dry_run {
        let playing = if action == "play_pause" {
            let prev = rt.playing.load(Ordering::Relaxed);
            let next = !prev;
            rt.playing.store(next, Ordering::Relaxed);
            next
        } else {
            rt.playing.load(Ordering::Relaxed)
        };
        return HttpResponse::json(
            200,
            "OK",
            &json!({
                "ok": true,
                "action": action,
                "playing": playing,
                "dry_run": true
            }),
        );
    }

    let act = action.clone();
    match act.as_str() {
        "vol_up" | "vol_down" => {
            let delta = if act == "vol_up" { 6 } else { -6 };
            let res = tokio::task::spawn_blocking(move || {
                crate::mochi::tools::audio::esp32_step_volume(delta)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            let playing = rt.playing.load(Ordering::Relaxed);
            match res {
                Ok(volume) => HttpResponse::json(
                    200,
                    "OK",
                    &json!({
                        "ok": true,
                        "action": action,
                        "playing": playing,
                        "volume": volume
                    }),
                ),
                Err(reason) => HttpResponse::json(
                    200,
                    "OK",
                    &json!({
                        "ok": false,
                        "action": action,
                        "playing": playing,
                        "error": reason
                    }),
                ),
            }
        }
        _ => {
            let act_inner = act.clone();
            let res = tokio::task::spawn_blocking(move || {
                crate::mochi::tools::media::esp32_media_control(&act_inner)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            match res {
                Ok(playing) => {
                    rt.playing.store(playing, Ordering::Relaxed);
                    HttpResponse::json(
                        200,
                        "OK",
                        &json!({
                            "ok": true,
                            "action": action,
                            "playing": playing
                        }),
                    )
                }
                Err(reason) => HttpResponse::json(
                    200,
                    "OK",
                    &json!({
                        "ok": false,
                        "action": action,
                        "playing": false,
                        "error": reason
                    }),
                ),
            }
        }
    }
}

async fn handle_tasks_sync(req: HttpRequest, rt: &Arc<Esp32Runtime>) -> HttpResponse {
    let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "error": "invalid_json" }),
        );
    };
    let Some(incoming) = val.get("envelopes").and_then(Value::as_array) else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "error": "missing_envelopes" }),
        );
    };
    let Some(app) = rt.app.as_ref() else {
        return HttpResponse::json(
            200,
            "OK",
            &json!({ "ok": true, "envelopes": [], "count": 0 }),
        );
    };
    let now_ms = crate::tasks::now_utc_ms_pub();
    match crate::tasks::sync_esp32_envelopes(app, incoming, now_ms) {
        Ok(merged) => HttpResponse::json(
            200,
            "OK",
            &json!({
                "ok": true,
                "envelopes": merged,
                "count": merged.len()
            }),
        ),
        Err(err) => HttpResponse::json(
            500,
            "Internal Server Error",
            &json!({ "ok": false, "error": err }),
        ),
    }
}

async fn handle_voice(req: HttpRequest, rt: &Arc<Esp32Runtime>) -> HttpResponse {
    if crate::integrations::PAUSED.load(Ordering::Relaxed) {
        return HttpResponse::json(
            503,
            "Service Unavailable",
            &json!({ "ok": false, "error": "paused" }),
        );
    }

    let ctype = req
        .content_type
        .as_deref()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let (heard, is_mock) = match ctype.as_str() {
        "audio/wav" | "audio/x-wav" | "audio/wave" => {
            let samples = match super::audio_wav::decode_wav_to_16k_mono(&req.body) {
                Ok(s) => s,
                Err(_) => {
                    return HttpResponse::json(
                        400,
                        "Bad Request",
                        &json!({ "ok": false, "error": "invalid_wav" }),
                    )
                }
            };
            if !crate::speech::dictation_ready() {
                return HttpResponse::json(
                    503,
                    "Service Unavailable",
                    &json!({ "ok": false, "error": "stt_unavailable" }),
                );
            }
            let text = tokio::task::spawn_blocking(move || {
                crate::speech::transcribe_dictation(&samples)
                    .map(|t| t.text.trim().to_string())
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            (text, false)
        }
        "application/json" => {
            let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
                return HttpResponse::json(
                    400,
                    "Bad Request",
                    &json!({ "ok": false, "error": "invalid_json" }),
                );
            };
            let text = val
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if text.is_empty() {
                return HttpResponse::json(
                    400,
                    "Bad Request",
                    &json!({ "ok": false, "error": "missing_text" }),
                );
            }
            let mock = val.get("mock").and_then(Value::as_bool).unwrap_or(false);
            (text, mock)
        }
        _ => {
            return HttpResponse::json(
                415,
                "Unsupported Media Type",
                &json!({ "ok": false, "error": "unsupported_content_type" }),
            )
        }
    };

    let answer = if is_mock {
        format!("Lumen: recebido {heard}")
    } else if heard.is_empty() {
        "Não ouvi nenhuma frase.".to_string()
    } else if let Some((tool, args)) = crate::voice::command::parse(&heard) {
        if let Some(app) = rt.app.as_ref() {
            if let Some(runtime) = app.try_state::<crate::mochi::MochiRuntime>() {
                let (run_id, cancel_rx) = runtime.begin_run();
                let ctx = crate::mochi::host::context(crate::integrations::PAUSED.load(Ordering::Relaxed)).await;
                let res = runtime
                    .invoke_tool(
                        tool,
                        args,
                        &ctx,
                        &crate::mochi::permissions::DefaultDenyRequester,
                        cancel_rx,
                    )
                    .await;
                runtime.finish_run(run_id);
                match res {
                    Ok(ok) => ok.audit_summary,
                    Err(err) => format!("Não consegui executar: {err}"),
                }
            } else {
                format!("Comando reconhecido: {tool}.")
            }
        } else {
            format!("Comando reconhecido: {tool}.")
        }
    } else {
        let maybe_cfg = rt
            .app
            .as_ref()
            .and_then(|a| a.try_state::<crate::Shared>())
            .map(|s| s.settings.lock().unwrap().clone());
        let mut reply_text = None;
        if let Some(cfg) = maybe_cfg {
            let system = "Você é o assistente Lumen em português do Brasil. Responda em no máximo uma frase bem curta, sem markdown.";
            if let Ok(raw) = crate::chat::complete_oneshot(
                &cfg,
                system,
                &heard,
                120,
                Duration::from_secs(9),
            )
            .await
            {
                let clean = crate::voice::brain::spoken_text(&raw);
                if !clean.trim().is_empty() {
                    reply_text = Some(clean);
                }
            }
        }
        reply_text.unwrap_or_else(|| format!("Lumen recebeu: {heard}"))
    };

    let ans_for_tts = answer.clone();
    let wav_bytes = tokio::task::spawn_blocking(move || {
        super::audio_wav::synthesize_tts_wav_22050(&ans_for_tts)
    })
    .await
    .unwrap_or_else(|_| super::audio_wav::fallback_silence_wav_22050(120));

    let wav_b64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);
    HttpResponse::json(
        200,
        "OK",
        &json!({
            "ok": true,
            "heard": heard,
            "answer": answer,
            "sample_rate": super::audio_wav::ESP32_OUT_SAMPLE_RATE,
            "wav_b64": wav_b64
        }),
    )
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    #[tokio::test]
    async fn voice_mock_json_returns_22050_wav_under_96kib() {
        let rt = Esp32Runtime::new(None);
        rt.arm_pair_code("123456").unwrap();
        let key = rt.pair_with_code("123456", Some("test-esp")).unwrap().unwrap();

        let req = HttpRequest {
            method: "POST".into(),
            path: "/v1/voice".into(),
            query: String::new(),
            authorization: Some(format!("Bearer {key}")),
            content_type: Some("application/json; charset=utf-8".into()),
            body: br#"{"text":"bom dia lumen","mock":true}"#.to_vec(),
        };
        let resp = dispatch_request(req, &rt).await;
        assert_eq!(resp.status, 200);
        assert!(resp.body.len() < 96 * 1024);
        let parsed: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["heard"], "bom dia lumen");
        assert_eq!(parsed["answer"], "Lumen: recebido bom dia lumen");
        assert_eq!(parsed["sample_rate"], 22_050);
        let wav = base64::engine::general_purpose::STANDARD
            .decode(parsed["wav_b64"].as_str().unwrap())
            .unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 22_050);
    }

    #[tokio::test]
    async fn voice_rejects_bad_content_type_and_bad_wav() {
        let rt = Esp32Runtime::new(None);
        rt.arm_pair_code("654321").unwrap();
        let key = rt.pair_with_code("654321", Some("test-esp")).unwrap().unwrap();

        let resp_ct = dispatch_request(
            HttpRequest {
                method: "POST".into(),
                path: "/v1/voice".into(),
                query: String::new(),
                authorization: Some(format!("Bearer {key}")),
                content_type: Some("text/plain".into()),
                body: b"oi".to_vec(),
            },
            &rt,
        )
        .await;
        assert_eq!(resp_ct.status, 415);

        let resp_wav = dispatch_request(
            HttpRequest {
                method: "POST".into(),
                path: "/v1/voice".into(),
                query: String::new(),
                authorization: Some(format!("Bearer {key}")),
                content_type: Some("audio/wav".into()),
                body: b"not a wav".to_vec(),
            },
            &rt,
        )
        .await;
        assert_eq!(resp_wav.status, 400);
    }
}
