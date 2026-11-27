//! Servidor HTTP/1.1 local (LAN) para a placa ESP32-S3 AMOLED.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
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

    HttpResponse::json(404, "Not Found", &json!({ "ok": false, "error": "not found" }))
}
