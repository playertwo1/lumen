//! Servidor HTTP/1.1 local (LAN) para a placa ESP32-S3 AMOLED.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::interactions::{
    self, InteractionStore, Verdict, ALIVE_POLL_DURATION, HOLD_SECS,
};
use super::Esp32Runtime;

pub(crate) fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
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

pub(crate) fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

pub(crate) fn is_valid_loopback_host(host_header: Option<&str>, expected_port: u16) -> bool {
    let Some(raw) = host_header.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    if raw.contains('@') || raw.contains('/') || raw.contains('\\') {
        return false;
    }
    let (host, port_opt): (&str, Option<&str>) = if raw.starts_with('[') {
        let Some(end) = raw.find(']') else {
            return false;
        };
        let inside = &raw[1..end];
        let rest = &raw[end + 1..];
        let port = if rest.is_empty() {
            None
        } else if let Some(p) = rest.strip_prefix(':') {
            if p.is_empty() {
                return false;
            }
            Some(p)
        } else {
            return false;
        };
        (inside, port)
    } else if let Some((h, p)) = raw.rsplit_once(':') {
        if p.is_empty() {
            return false;
        }
        (h, Some(p))
    } else {
        (raw, None)
    };

    let lower = host.to_ascii_lowercase();
    let host_ok = if lower == "localhost" || lower == "localhost." || lower == "::1" {
        true
    } else if let Ok(v4) = lower.parse::<Ipv4Addr>() {
        v4.is_loopback()
    } else {
        false
    };
    if !host_ok {
        return false;
    }

    if let Some(p_str) = port_opt {
        let Ok(supplied) = p_str.parse::<u16>() else {
            return false;
        };
        if expected_port > 0 && supplied != expected_port {
            return false;
        }
    }
    true
}

pub(crate) fn has_json_content_type(ct: Option<&str>) -> bool {
    let Some(raw) = ct.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    if let Some((media, param)) = raw.split_once(';') {
        if !media.trim().eq_ignore_ascii_case("application/json") {
            return false;
        }
        let param_trimmed = param.trim();
        let Some((k, v)) = param_trimmed.split_once('=') else {
            return false;
        };
        if !k.trim().eq_ignore_ascii_case("charset") {
            return false;
        }
        let v_clean = v.trim().trim_matches('"');
        v_clean.eq_ignore_ascii_case("utf-8")
    } else {
        raw.eq_ignore_ascii_case("application/json")
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
    let local_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
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
                let peer_ip = peer.ip();
                tokio::spawn(async move {
                    let _ = stream.set_nodelay(true);
                    handle_connection(stream, rt_conn, peer_ip, local_port).await;
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
    pub peer_ip: IpAddr,
    pub local_port: u16,
    pub host: Option<String>,
    pub host_count: usize,
    pub origins: Vec<String>,
    pub authorization: Option<String>,
    pub content_type: Option<String>,
    pub content_type_count: usize,
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

    fn empty_ok() -> Self {
        Self {
            status: 200,
            reason: "OK",
            body: Vec::new(),
        }
    }
}

fn max_body_for_path(path: &str) -> usize {
    match path {
        "/v1/voice" => MAX_VOICE_BODY_BYTES,
        p if p.starts_with("/api/") => MAX_JSON_BODY_BYTES,
        _ => MAX_SMALL_BODY_BYTES,
    }
}

async fn read_http_request(
    stream: &mut TcpStream,
    peer_ip: IpAddr,
    local_port: u16,
    rt: &Esp32Runtime,
) -> Result<HttpRequest, HttpResponse> {
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
    let mut content_type_count: usize = 0;
    let mut host: Option<String> = None;
    let mut host_count: usize = 0;
    let mut origins: Vec<String> = Vec::new();

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
                "content-type" => {
                    content_type_count += 1;
                    if content_type.is_none() {
                        content_type = Some(v.to_string());
                    }
                }
                "host" => {
                    host_count += 1;
                    if host.is_none() {
                        host = Some(v.to_string());
                    }
                }
                "origin" => origins.push(v.to_string()),
                _ => {}
            }
        }
    }

    // Rejeições antecipadas antes de estacionar ou ler body grande (drena corpo curto p/ evitar RST).
    let is_hook = is_hook_route(&method, &path);
    let is_interaction_post = method == "POST"
        && (path.starts_with("/api/interaction/") || path == "/api/panic");
    if is_hook {
        if !rt.agent_panel_enabled.load(Ordering::Relaxed) {
            drain_announced_body(stream, &buf, header_end, content_length).await;
            return Err(HttpResponse::json(
                404,
                "Not Found",
                &json!({ "error": "interactions are not enabled" }),
            ));
        }
        if !is_loopback_ip(peer_ip) {
            drain_announced_body(stream, &buf, header_end, content_length).await;
            return Err(HttpResponse::json(
                403,
                "Forbidden",
                &json!({ "error": "hooks must be local" }),
            ));
        }
        if host_count != 1 || !is_valid_loopback_host(host.as_deref(), local_port) || !origins.is_empty() {
            drain_announced_body(stream, &buf, header_end, content_length).await;
            return Err(HttpResponse::json(
                403,
                "Forbidden",
                &json!({ "error": "hook ingress rejected" }),
            ));
        }
    }
    if is_hook || is_interaction_post {
        if content_type_count != 1 || !has_json_content_type(content_type.as_deref()) {
            drain_announced_body(stream, &buf, header_end, content_length).await;
            return Err(HttpResponse::json(
                415,
                "Unsupported Media Type",
                &json!({ "error": "application/json required" }),
            ));
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
        peer_ip,
        local_port,
        host,
        host_count,
        origins,
        authorization,
        content_type,
        content_type_count,
        body,
    })
}

async fn drain_announced_body(
    stream: &mut TcpStream,
    buf: &[u8],
    header_end: usize,
    content_length: usize,
) {
    let already = buf.len().saturating_sub(header_end + 4);
    let mut remaining = content_length.min(MAX_JSON_BODY_BYTES).saturating_sub(already);
    let mut tmp = [0u8; 2048];
    let _ = tokio::time::timeout(Duration::from_millis(50), async {
        while remaining > 0 {
            let to_read = remaining.min(tmp.len());
            match stream.read(&mut tmp[..to_read]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => remaining -= n,
            }
        }
    })
    .await;
}

fn is_hook_route(method: &str, path: &str) -> bool {
    method == "POST"
        && matches!(
            path,
            "/api/hook/question"
                | "/api/hook/permission"
                | "/api/codex/question"
                | "/api/codex/permission"
        )
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

async fn handle_connection(
    mut stream: TcpStream,
    rt: Arc<Esp32Runtime>,
    peer_ip: IpAddr,
    local_port: u16,
) {
    let req = match read_http_request(&mut stream, peer_ip, local_port, &rt).await {
        Ok(r) => r,
        Err(resp) => {
            write_http_response(&mut stream, resp).await;
            return;
        }
    };
    if is_hook_route(&req.method, &req.path) {
        let resp = handle_hook_with_stream(&mut stream, req, &rt).await;
        if let Some(r) = resp {
            write_http_response(&mut stream, r).await;
        }
        return;
    }
    let resp = dispatch_request(req, &rt).await;
    write_http_response(&mut stream, resp).await;
}

async fn wait_for_client_disconnect(stream: &TcpStream, poll_interval: Duration) {
    let mut interval = tokio::time::interval(poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Consome o primeiro tick imediato; a primeira checagem ocorre após poll_interval (2 s) ou se readable.
    interval.tick().await;
    let mut probe = [0u8; 1];
    loop {
        tokio::select! {
            _ = interval.tick() => {
                match stream.try_read(&mut probe) {
                    Ok(0) => return,
                    Ok(_) => return,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => return,
                }
            }
            ready = stream.readable() => {
                if ready.is_err() {
                    return;
                }
                match stream.try_read(&mut probe) {
                    Ok(0) => return,
                    Ok(_) => return,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => return,
                }
            }
        }
    }
}

async fn handle_hook_with_stream(
    stream: &mut TcpStream,
    req: HttpRequest,
    rt: &Arc<Esp32Runtime>,
) -> Option<HttpResponse> {
    if let Some(gate_err) = validate_hook_ingress(&req, rt) {
        return Some(gate_err);
    }
    let Ok(event) = serde_json::from_slice::<Value>(&req.body) else {
        return Some(if req.path == "/api/codex/question" {
            HttpResponse::json(200, "OK", &json!({ "status": "computer", "reason": "invalid" }))
        } else {
            HttpResponse::empty_ok()
        });
    };

    let now_ms = InteractionStore::now_ms();
    let hold_secs = HOLD_SECS;
    let handle = match req.path.as_str() {
        "/api/hook/question" => rt.interactions.park_claude_question(&event, hold_secs, now_ms),
        "/api/hook/permission" => rt.interactions.park_claude_permission(&event, hold_secs, now_ms),
        "/api/codex/permission" => rt.interactions.park_codex_permission(&event, hold_secs, now_ms),
        "/api/codex/question" => match rt.interactions.park_codex_question(&event, hold_secs, now_ms) {
            Ok(Some(h)) => Some(h),
            Ok(None) => {
                return Some(HttpResponse::json(
                    200,
                    "OK",
                    &json!({ "status": "computer", "reason": "unavailable" }),
                ));
            }
            Err(reason) => {
                return Some(HttpResponse::json(
                    200,
                    "OK",
                    &json!({ "status": "computer", "reason": reason }),
                ));
            }
        },
        _ => None,
    };

    let Some(handle) = handle else {
        return Some(HttpResponse::empty_ok());
    };

    let wait_dur = Duration::from_secs(hold_secs);
    let outcome: Option<Verdict> = tokio::select! {
        res = tokio::time::timeout(wait_dur, handle.rx) => {
            match res {
                Ok(Ok(v)) => v,
                Ok(Err(_)) | Err(_) => {
                    rt.interactions.abandon(&handle.request_id, InteractionStore::now_ms());
                    None
                }
            }
        }
        () = wait_for_client_disconnect(stream, ALIVE_POLL_DURATION) => {
            rt.interactions.abandon(&handle.request_id, InteractionStore::now_ms());
            if req.path == "/api/codex/question" {
                return Some(HttpResponse::json(
                    200,
                    "OK",
                    &json!({ "status": "computer", "reason": "disconnected" }),
                ));
            }
            return None;
        }
    };

    match req.path.as_str() {
        "/api/hook/question" => match outcome {
            Some(v) => match interactions::build_claude_question_hook_response(v, &event) {
                Some(body) => Some(HttpResponse::json(200, "OK", &body)),
                None => Some(HttpResponse::empty_ok()),
            },
            None => Some(HttpResponse::empty_ok()),
        },
        "/api/hook/permission" | "/api/codex/permission" => match outcome {
            Some(v) => match interactions::build_permission_hook_response(v) {
                Some(body) => Some(HttpResponse::json(200, "OK", &body)),
                None => Some(HttpResponse::empty_ok()),
            },
            None => Some(HttpResponse::empty_ok()),
        },
        "/api/codex/question" => match outcome {
            Some(v) => Some(HttpResponse::json(
                200,
                "OK",
                &interactions::build_codex_question_response(v, &event),
            )),
            None => Some(HttpResponse::json(
                200,
                "OK",
                &json!({ "status": "computer", "reason": "timeout" }),
            )),
        },
        _ => Some(HttpResponse::empty_ok()),
    }
}

fn validate_hook_ingress(req: &HttpRequest, rt: &Esp32Runtime) -> Option<HttpResponse> {
    if !rt.agent_panel_enabled.load(Ordering::Relaxed) {
        return Some(HttpResponse::json(
            404,
            "Not Found",
            &json!({ "error": "interactions are not enabled" }),
        ));
    }
    if !is_loopback_ip(req.peer_ip) {
        return Some(HttpResponse::json(
            403,
            "Forbidden",
            &json!({ "error": "hooks must be local" }),
        ));
    }
    if req.host_count != 1
        || !is_valid_loopback_host(req.host.as_deref(), req.local_port)
        || !req.origins.is_empty()
    {
        return Some(HttpResponse::json(
            403,
            "Forbidden",
            &json!({ "error": "hook ingress rejected" }),
        ));
    }
    if req.content_type_count != 1 || !has_json_content_type(req.content_type.as_deref()) {
        return Some(HttpResponse::json(
            415,
            "Unsupported Media Type",
            &json!({ "error": "application/json required" }),
        ));
    }
    None
}

pub async fn dispatch_request(req: HttpRequest, rt: &Arc<Esp32Runtime>) -> HttpResponse {
    if req.method == "GET" && req.path == "/v1/ping" {
        rt.note_lan_activity();
        return HttpResponse::json(200, "OK", &json!({ "ok": true, "service": "lumen-bridge" }));
    }

    if req.method == "POST" && req.path == "/v1/voice" {
        if !rt.verify_bearer(req.authorization.as_deref()) {
            return HttpResponse::json(
                401,
                "Unauthorized",
                &json!({ "ok": false, "error": "unauthorized" }),
            );
        }
        rt.note_lan_activity();
        return handle_voice(req, rt).await;
    }

    if req.path.starts_with("/api/") {
        return handle_api_route(req, rt).await;
    }

    HttpResponse::json(404, "Not Found", &json!({ "ok": false, "error": "not found" }))
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

async fn handle_api_route(req: HttpRequest, rt: &Arc<Esp32Runtime>) -> HttpResponse {
    if is_hook_route(&req.method, &req.path) {
        if let Some(err) = validate_hook_ingress(&req, rt) {
            return err;
        }
        return HttpResponse::empty_ok();
    }

    if req.method == "POST"
        && (req.path.starts_with("/api/interaction/") || req.path == "/api/panic")
    {
        if !rt.agent_panel_enabled.load(Ordering::Relaxed) {
            return HttpResponse::json(
                404,
                "Not Found",
                &json!({ "error": "interactions are not enabled" }),
            );
        }
        if req.content_type_count != 1 || !has_json_content_type(req.content_type.as_deref()) {
            return HttpResponse::json(
                415,
                "Unsupported Media Type",
                &json!({ "error": "application/json required" }),
            );
        }
        if req.path == "/api/panic" {
            return handle_panic_route(&req, rt);
        }
        let request_id = req.path.trim_start_matches("/api/interaction/");
        return handle_interaction_answer_route(request_id, &req, rt);
    }

    if !rt.agent_panel_enabled.load(Ordering::Relaxed) || !rt.has_paired_keys() {
        return HttpResponse::json(
            401,
            "Unauthorized",
            &json!({ "ok": false, "error": "panel_disabled_or_unpaired" }),
        );
    }
    if req.authorization.is_some() && !rt.verify_bearer(req.authorization.as_deref()) {
        return HttpResponse::json(
            401,
            "Unauthorized",
            &json!({ "ok": false, "error": "unauthorized" }),
        );
    }
    rt.note_lan_activity();

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/tokens") => {
            let rt_bg = rt.clone();
            let payload = tokio::task::spawn_blocking(move || {
                super::build_enriched_usage_payload(&rt_bg)
            })
            .await
            .unwrap_or_else(|_| {
                let snap = super::build_live_tokens_snapshot(rt);
                super::tokens_agents::build_clean_usage_payload(&snap)
            });
            HttpResponse::json(200, "OK", &payload)
        }
        ("GET", "/api/agent-status") => {
            let rt_bg = rt.clone();
            let snap = tokio::task::spawn_blocking(move || {
                super::build_agent_status_snapshot(&rt_bg)
            })
            .await
            .unwrap_or_else(|_| json!({ "v": 2, "seq": 0, "agents": { "claude": { "active_count": 0, "jobs": [] }, "codex": { "active_count": 0, "jobs": [] } } }));
            HttpResponse::json(200, "OK", &snap)
        }
        ("GET", "/api/agents") => {
            let now_ms = InteractionStore::now_ms();
            let now_sec = Esp32Runtime::now_secs();
            let snap = rt.interactions.legacy_agents_snapshot(now_ms, now_sec);
            HttpResponse::json(200, "OK", &snap)
        }
        ("POST", "/api/agents/decide") => {
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
            let id = val.get("id").and_then(Value::as_str).unwrap_or("");
            let decision = val.get("decision").and_then(Value::as_str).unwrap_or("");
            let now_ms = InteractionStore::now_ms();
            let res = rt.interactions.legacy_decide(id, decision, now_ms);
            match res {
                Ok((body, pipe_id)) => {
                    if let (Some(app), Some(pid)) = (rt.app.as_ref(), pipe_id) {
                        crate::resolve_approval(app, &pid, decision);
                        let _ = tauri::Emitter::emit_to(app, "island", "approval-expired", &pid);
                    }
                    HttpResponse::json(200, "OK", &body)
                }
                Err((400, msg)) => {
                    HttpResponse::json(400, "Bad Request", &json!({ "ok": false, "error": msg }))
                }
                Err((_, msg)) => {
                    HttpResponse::json(409, "Conflict", &json!({ "ok": false, "error": msg }))
                }
            }
        }
        ("GET", "/api/agents/decision") => {
            let id = query_param(&req.query, "id").unwrap_or("");
            let now_ms = InteractionStore::now_ms();
            let body = rt.interactions.legacy_get_decision(id, now_ms);
            HttpResponse::json(200, "OK", &body)
        }
        ("POST", "/api/agents/event") => {
            let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
                return HttpResponse::json(
                    400,
                    "Bad Request",
                    &json!({ "ok": false, "error": "invalid_json" }),
                );
            };
            let event = val
                .get("hook_event_name")
                .or_else(|| val.get("event"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let agent = val
                .get("agent")
                .or_else(|| val.get("coucou_agent"))
                .and_then(Value::as_str)
                .unwrap_or("Claude");
            let cwd = val.get("cwd").and_then(Value::as_str).unwrap_or("");
            let now_sec = Esp32Runtime::now_secs();
            let now_ms = InteractionStore::now_ms();
            match event {
                "StatusLine" => {
                    if let Some(rl) = val.get("rate_limits") {
                        rt.claude_quota
                            .lock()
                            .unwrap()
                            .update_from_statusline(rl, now_sec);
                    }
                    HttpResponse::json(200, "OK", &json!({ "ok": true }))
                }
                "PermissionRequest" => {
                    let tool = val.get("tool_name").and_then(Value::as_str).unwrap_or("Tool");
                    let cmd = val
                        .get("tool_input")
                        .and_then(|ti| {
                            ti.get("command")
                                .or_else(|| ti.get("file_path"))
                                .and_then(Value::as_str)
                        })
                        .unwrap_or("");
                    let id = rt
                        .interactions
                        .park_pipe_or_legacy(
                            None,
                            agent,
                            cwd,
                            tool,
                            cmd,
                            interactions::HOLD_SECS,
                            now_ms,
                        )
                        .unwrap_or_default();
                    HttpResponse::json(200, "OK", &json!({ "ok": true, "id": id }))
                }
                "Stop" | "Notification" => {
                    rt.interactions.note_turn(agent, cwd);
                    HttpResponse::json(200, "OK", &json!({ "ok": true }))
                }
                "UserPromptSubmit" | "SessionEnd" => {
                    rt.interactions.note_clear(now_ms);
                    HttpResponse::json(200, "OK", &json!({ "ok": true }))
                }
                _ => HttpResponse::json(200, "OK", &json!({ "ok": true })),
            }
        }
        ("POST", "/api/agents/turn/dismiss") => {
            let body = rt.interactions.dismiss_turn();
            HttpResponse::json(200, "OK", &body)
        }
        _ => HttpResponse::json(404, "Not Found", &json!({ "ok": false, "error": "not found" })),
    }
}

fn handle_interaction_answer_route(
    request_id: &str,
    req: &HttpRequest,
    rt: &Esp32Runtime,
) -> HttpResponse {
    let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "reason": "bad request" }),
        );
    };
    let Some(obj) = val.as_object() else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "reason": "bad request" }),
        );
    };
    let provider = obj.get("provider").and_then(Value::as_str).unwrap_or("");
    let view_sha256 = obj.get("view_sha256").and_then(Value::as_str).unwrap_or("");
    let verdict = obj.get("verdict").and_then(Value::as_str).unwrap_or("");
    let Some(ts) = obj.get("ts").and_then(Value::as_i64) else {
        return HttpResponse::json(
            409,
            "Conflict",
            &json!({ "ok": false, "reason": "signature rejected" }),
        );
    };
    let hmac = obj.get("hmac").and_then(Value::as_str).unwrap_or("");

    if verdict == "approve" && crate::integrations::PAUSED.load(Ordering::Relaxed) {
        return HttpResponse::json(
            503,
            "Service Unavailable",
            &json!({ "ok": false, "reason": "paused" }),
        );
    }

    let secret = rt.room_key_for_hmac().unwrap_or_default();
    let now_ms = InteractionStore::now_ms();
    let now_sec = Esp32Runtime::now_secs();
    match rt.interactions.resolve_http(
        &secret,
        request_id,
        provider,
        view_sha256,
        verdict,
        ts,
        hmac,
        now_ms,
        now_sec,
    ) {
        Ok((outcome, _)) => {
            if let (Some(app), Some(pid)) = (rt.app.as_ref(), outcome.pipe_request_id.as_deref()) {
                super::apply_pipe_outcome(app, pid, outcome.verdict);
            }
            HttpResponse::json(200, "OK", &json!({ "ok": true, "reason": "ok" }))
        }
        Err((status, reason)) => {
            let reason_phrase = if status == 400 { "Bad Request" } else { "Conflict" };
            HttpResponse::json(
                status,
                reason_phrase,
                &json!({ "ok": false, "reason": reason }),
            )
        }
    }
}

fn handle_panic_route(req: &HttpRequest, rt: &Esp32Runtime) -> HttpResponse {
    let Ok(val) = serde_json::from_slice::<Value>(&req.body) else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "reason": "bad request" }),
        );
    };
    let Some(obj) = val.as_object() else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &json!({ "ok": false, "reason": "bad request" }),
        );
    };
    let Some(ts) = obj.get("ts").and_then(Value::as_i64) else {
        return HttpResponse::json(
            409,
            "Conflict",
            &json!({ "ok": false, "reason": "signature rejected" }),
        );
    };
    let hmac = obj.get("hmac").and_then(Value::as_str).unwrap_or("");
    let secret = rt.room_key_for_hmac().unwrap_or_default();
    let now_ms = InteractionStore::now_ms();
    let now_sec = Esp32Runtime::now_secs();
    match rt.interactions.panic_http(&secret, ts, hmac, now_ms, now_sec) {
        Ok((denied, pipe_ids)) => {
            if let Some(app) = rt.app.as_ref() {
                for pid in pipe_ids {
                    super::apply_pipe_outcome(app, &pid, Verdict::Deny);
                }
            }
            HttpResponse::json(200, "OK", &json!({ "ok": true, "denied": denied }))
        }
        Err(reason) => HttpResponse::json(
            409,
            "Conflict",
            &json!({ "ok": false, "reason": reason }),
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
        rt.keys.lock().unwrap().clear();
        let key = super::super::crypto::generate_bridge_key_hex().unwrap();
        rt.keys.lock().unwrap().push(super::super::PairedDeviceKey {
            device_id: "test-esp".into(),
            key: key.clone(),
            created_at: 1,
        });

        let req = HttpRequest {
            method: "POST".into(),
            path: "/v1/voice".into(),
            query: String::new(),
            peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            local_port: 8787,
            host: Some("127.0.0.1:8787".into()),
            host_count: 1,
            origins: Vec::new(),
            authorization: Some(format!("Bearer {key}")),
            content_type: Some("application/json; charset=utf-8".into()),
            content_type_count: 1,
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
        rt.keys.lock().unwrap().clear();
        let key = super::super::crypto::generate_bridge_key_hex().unwrap();
        rt.keys.lock().unwrap().push(super::super::PairedDeviceKey {
            device_id: "test-esp".into(),
            key: key.clone(),
            created_at: 1,
        });

        let resp_ct = dispatch_request(
            HttpRequest {
                method: "POST".into(),
                path: "/v1/voice".into(),
                query: String::new(),
                peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                local_port: 8787,
                host: Some("127.0.0.1:8787".into()),
                host_count: 1,
                origins: Vec::new(),
                authorization: Some(format!("Bearer {key}")),
                content_type: Some("text/plain".into()),
                content_type_count: 1,
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
                peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                local_port: 8787,
                host: Some("127.0.0.1:8787".into()),
                host_count: 1,
                origins: Vec::new(),
                authorization: Some(format!("Bearer {key}")),
                content_type: Some("audio/wav".into()),
                content_type_count: 1,
                body: b"not a wav".to_vec(),
            },
            &rt,
        )
        .await;
        assert_eq!(resp_wav.status, 400);
    }
}
