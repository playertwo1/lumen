//! Relay para Codex e ESP32-S3 (`--esp32-codex-permission` e `--esp32-codex-mcp`).
//!
//! Usa exclusivamente `std` e `serde_json` para manter o binário enxuto.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{json, Value};

const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_TIMEOUT_SECS: u64 = 125;
const MAX_STDIN_BYTES: usize = 256 * 1024; // 256 KiB

/// Executa `--esp32-codex-permission <port>` usando stdin e stdout padrão.
pub fn run_permission(port: u16) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_permission_with_io(port, stdin.lock(), stdout.lock())
}

/// Versão testável com injeção de `Read` e `Write`.
pub fn run_permission_with_io<R: Read, W: Write>(port: u16, reader: R, mut writer: W) -> i32 {
    let mut stdin_buf = Vec::new();
    // Lê até 256 KiB + 1 para detectar estouro de tamanho
    let Ok(read_len) = reader
        .take((MAX_STDIN_BYTES + 1) as u64)
        .read_to_end(&mut stdin_buf)
    else {
        return 0;
    };

    if read_len > MAX_STDIN_BYTES {
        return 0;
    }

    if let Ok((status, resp_body)) = http_post(port, "/api/codex/permission", &stdin_buf, DEFAULT_TIMEOUT_SECS) {
        if status == 200 {
            if let Ok(body_str) = std::str::from_utf8(&resp_body) {
                if body_str.contains("\"hookSpecificOutput\"") {
                    let _ = writer.write_all(&resp_body);
                    let _ = writer.flush();
                }
            }
        }
    }

    0
}

/// Executa `--esp32-codex-mcp <port>` sobre stdin/stdout padrão.
pub fn run_mcp(port: u16) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    match run_mcp_loop(port, reader, stdout.lock()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// Loop MCP JSON-RPC 2.0 sobre canais de entrada e saída.
pub fn run_mcp_loop<R: BufRead, W: Write>(port: u16, mut reader: R, mut writer: W) -> Result<(), String> {
    while let Some(msg) = read_mcp_message(&mut reader)? {
        if let Some(resp) = handle_mcp_message(msg, port) {
            write_mcp_response(&mut writer, &resp)?;
        }
    }
    Ok(())
}

/// Lê uma mensagem MCP suportando tanto NDJSON direto quanto headers `Content-Length`.
pub fn read_mcp_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, String> {
    let mut line = String::new();
    let mut content_length: Option<usize> = None;
    let mut first_line = true;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).map_err(|e| format!("erro lendo linha: {e}"))?;
        if bytes_read == 0 {
            if content_length.is_some() {
                return Err("EOF inesperado lendo corpo com Content-Length".to_string());
            }
            return Ok(None);
        }

        let mut trimmed = line.trim();
        if first_line {
            first_line = false;
            trimmed = trimmed.strip_prefix('\u{FEFF}').unwrap_or(trimmed).trim();
        }

        if trimmed.is_empty() {
            if let Some(len) = content_length {
                let mut body = vec![0u8; len];
                reader.read_exact(&mut body).map_err(|e| format!("erro lendo corpo: {e}"))?;
                let parsed: Value = serde_json::from_slice(&body).map_err(|e| format!("JSON inválido: {e}"))?;
                return Ok(Some(parsed));
            }
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("content-length:") {
            let val_part = trimmed["content-length:".len()..].trim();
            let len: usize = val_part.parse().map_err(|_| "Content-Length inválido".to_string())?;
            content_length = Some(len);
            continue;
        }

        if trimmed.starts_with('{') {
            let parsed: Value = serde_json::from_str(trimmed).map_err(|e| format!("JSON inválido: {e}"))?;
            return Ok(Some(parsed));
        }
    }
}

pub fn write_mcp_response<W: Write>(writer: &mut W, response: &Value) -> Result<(), String> {
    let text = serde_json::to_string(response).map_err(|e| e.to_string())?;
    writeln!(writer, "{text}").map_err(|e| format!("erro gravando stdout: {e}"))?;
    writer.flush().map_err(|e| format!("erro descarregando stdout: {e}"))?;
    Ok(())
}

fn handle_mcp_message(msg: Value, port: u16) -> Option<Value> {
    let obj = msg.as_object()?;
    let method = obj.get("method").and_then(Value::as_str);
    let id = obj.get("id").cloned();

    match method {
        Some("initialize") => {
            let reply_id = id.unwrap_or(Value::Null);
            Some(json!({
                "jsonrpc": "2.0",
                "id": reply_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "lumen_placa",
                        "version": "1.0.0"
                    }
                }
            }))
        }
        Some("notifications/initialized") | Some("initialized") => None,
        Some("tools/list") => {
            let reply_id = id.unwrap_or(Value::Null);
            Some(json!({
                "jsonrpc": "2.0",
                "id": reply_id,
                "result": {
                    "tools": [
                        {
                            "name": "ask",
                            "description": "Faz uma pergunta ao usuário exibindo as opções na tela da placa ESP32-S3",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "header": { "type": "string", "description": "Cabeçalho curto da pergunta" },
                                    "question": { "type": "string", "description": "Texto da pergunta a ser respondida" },
                                    "options": {
                                        "type": "array",
                                        "description": "Lista de opções para o usuário escolher",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "label": { "type": "string" },
                                                "description": { "type": "string" },
                                                "recommended": { "type": "boolean" }
                                            },
                                            "required": ["label"]
                                        }
                                    }
                                },
                                "required": ["question", "options"]
                            }
                        }
                    ]
                }
            }))
        }
        Some("tools/call") => {
            let reply_id = id.unwrap_or(Value::Null);
            let params = obj.get("params").and_then(Value::as_object);
            let tool_name = params.and_then(|p| p.get("name")).and_then(Value::as_str);

            if tool_name == Some("ask") {
                let arguments = params.and_then(|p| p.get("arguments")).cloned().unwrap_or_else(|| json!({}));
                let outcome = call_esp32_question(port, arguments);
                let text_val = serde_json::to_string(&outcome).unwrap_or_default();
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": reply_id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": text_val
                            }
                        ],
                        "structuredContent": outcome
                    }
                }))
            } else {
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": reply_id,
                    "error": {
                        "code": -32601,
                        "message": format!("Ferramenta desconhecida: {:?}", tool_name)
                    }
                }))
            }
        }
        Some("ping") => {
            let reply_id = id.unwrap_or(Value::Null);
            Some(json!({
                "jsonrpc": "2.0",
                "id": reply_id,
                "result": {}
            }))
        }
        Some(m) if m.starts_with("notifications/") => None,
        _ => {
            id.map(|reply_id| {
                json!({
                    "jsonrpc": "2.0",
                    "id": reply_id,
                    "error": {
                        "code": -32601,
                        "message": "Method not found"
                    }
                })
            })
        }
    }
}

fn call_esp32_question(port: u16, mut arguments: Value) -> Value {
    let fallback = json!({ "status": "computer", "reason": "unavailable" });

    let Some(obj) = arguments.as_object_mut() else {
        return fallback;
    };

    if !obj.contains_key("cwd") {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        obj.insert("cwd".to_string(), json!(cwd));
    }
    if !obj.contains_key("session_id") {
        let sid = std::env::var("CODEX_SESSION_ID").unwrap_or_else(|_| "mcp-session".to_string());
        obj.insert("session_id".to_string(), json!(sid));
    }
    if !obj.contains_key("turn_id") {
        let tid = std::env::var("CODEX_TURN_ID").unwrap_or_else(|_| "turn-1".to_string());
        obj.insert("turn_id".to_string(), json!(tid));
    }

    let Ok(body_bytes) = serde_json::to_vec(obj) else {
        return fallback;
    };

    match http_post(port, "/api/codex/question", &body_bytes, DEFAULT_TIMEOUT_SECS) {
        Ok((200, resp_body)) => {
            serde_json::from_slice::<Value>(&resp_body).unwrap_or(fallback)
        }
        _ => fallback,
    }
}

/// Envia POST HTTP/1.1 via `TcpStream` loopback sem cabeçalho `Origin`.
fn http_post(port: u16, path: &str, body: &[u8], timeout_secs: u64) -> Result<(u16, Vec<u8>), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, HTTP_CONNECT_TIMEOUT)
        .map_err(|e| format!("falha ao conectar na porta {port}: {e}"))?;

    stream
        .set_read_timeout(Some(Duration::from_secs(timeout_secs)))
        .map_err(|e| format!("falha ao ajustar read_timeout: {e}"))?;
    stream
        .set_write_timeout(Some(HTTP_WRITE_TIMEOUT))
        .map_err(|e| format!("falha ao ajustar write_timeout: {e}"))?;

    let header = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );

    let mut req = Vec::with_capacity(header.len() + body.len());
    req.extend_from_slice(header.as_bytes());
    req.extend_from_slice(body);
    stream
        .write_all(&req)
        .map_err(|e| format!("falha ao escrever HTTP: {e}"))?;
    stream.flush().map_err(|e| format!("falha ao flush HTTP: {e}"))?;

    let mut resp_raw = Vec::new();
    stream
        .read_to_end(&mut resp_raw)
        .map_err(|e| format!("falha ao ler resposta HTTP: {e}"))?;

    parse_http_response(&resp_raw)
}

fn parse_http_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split_pos = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "resposta HTTP sem separador \\r\\n\\r\\n".to_string())?;

    let head_bytes = &raw[..split_pos];
    let body_bytes = &raw[split_pos + 4..];

    let head_str = std::str::from_utf8(head_bytes).map_err(|e| format!("cabeçalho não UTF-8: {e}"))?;
    let first_line = head_str.lines().next().unwrap_or_default();
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("linha de status HTTP inválida".to_string());
    }
    let status: u16 = parts[1].parse().map_err(|_| "código de status HTTP inválido".to_string())?;

    Ok((status, body_bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;

    #[test]
    fn permission_prints_stdout_only_when_200_and_has_hook_specific_output() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = client.read(&mut buf).unwrap();
            let body = r#"{"hookSpecificOutput":{"decision":"approve"}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            client.write_all(resp.as_bytes()).unwrap();
        });

        let input = Cursor::new(br#"{"session_id":"s1","tool_name":"Shell"}"#);
        let mut output = Vec::new();

        let code = run_permission_with_io(port, input, &mut output);
        handle.join().unwrap();

        assert_eq!(code, 0);
        let out_str = String::from_utf8(output).unwrap();
        assert!(out_str.contains("hookSpecificOutput"));
        assert!(out_str.contains("approve"));
    }

    #[test]
    fn permission_silent_when_response_has_no_hook_specific_output() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = client.read(&mut buf).unwrap();
            let body = r#"{"status":"computer","reason":"deny"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            client.write_all(resp.as_bytes()).unwrap();
        });

        let input = Cursor::new(br#"{"session_id":"s2"}"#);
        let mut output = Vec::new();

        let code = run_permission_with_io(port, input, &mut output);
        handle.join().unwrap();

        assert_eq!(code, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn permission_silent_when_server_unavailable() {
        let input = Cursor::new(br#"{"session_id":"s3"}"#);
        let mut output = Vec::new();
        // porta improvável de estar aberta
        let code = run_permission_with_io(64321, input, &mut output);
        assert_eq!(code, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn permission_silent_when_stdin_exceeds_256_kib() {
        let huge_input = vec![b'a'; MAX_STDIN_BYTES + 10];
        let input = Cursor::new(huge_input);
        let mut output = Vec::new();
        let code = run_permission_with_io(8787, input, &mut output);
        assert_eq!(code, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn mcp_handles_initialize_tools_list_and_ask_call() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut req_buf = Vec::new();
            let mut tmp = [0u8; 512];
            let mut content_len: Option<usize> = None;
            let mut header_len = 0;
            loop {
                let n = client.read(&mut tmp).unwrap();
                if n == 0 {
                    break;
                }
                req_buf.extend_from_slice(&tmp[..n]);
                if content_len.is_none() {
                    if let Some(pos) = req_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_len = pos + 4;
                        let head = String::from_utf8_lossy(&req_buf[..pos]);
                        for line in head.lines() {
                            if line.to_ascii_lowercase().starts_with("content-length:") {
                                if let Ok(val) = line["content-length:".len()..].trim().parse::<usize>() {
                                    content_len = Some(val);
                                }
                            }
                        }
                        if content_len.is_none() {
                            content_len = Some(0);
                        }
                    }
                }
                if let Some(cl) = content_len {
                    if req_buf.len() >= header_len + cl {
                        break;
                    }
                }
            }
            let body = r#"{"status":"answered","option_index":0,"answer":"Sim"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            client.write_all(resp.as_bytes()).unwrap();
            client.flush().unwrap();
        });

        // 1. Initialize
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let init_resp = handle_mcp_message(init_req, port).unwrap();
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "lumen_placa");

        // 2. Initialized notification (sem resposta)
        let initialized_notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        assert!(handle_mcp_message(initialized_notif, port).is_none());

        // 3. Tools list
        let list_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let list_resp = handle_mcp_message(list_req, port).unwrap();
        let tools = list_resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "ask");

        // 4. Tools call ask
        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "ask",
                "arguments": {
                    "question": "Deploy agora?",
                    "options": [
                        { "label": "Sim", "recommended": true },
                        { "label": "Não" }
                    ]
                }
            }
        });
        let call_resp = handle_mcp_message(call_req, port).unwrap();
        handle.join().unwrap();

        assert_eq!(call_resp["result"]["structuredContent"]["status"], "answered");
        assert_eq!(call_resp["result"]["structuredContent"]["answer"], "Sim");
    }

    #[test]
    fn mcp_fallback_when_http_fails() {
        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "ask",
                "arguments": {
                    "question": "Deseja continuar?",
                    "options": [{ "label": "Ok" }]
                }
            }
        });
        let resp = handle_mcp_message(call_req, 64322).unwrap();
        assert_eq!(resp["result"]["structuredContent"]["status"], "computer");
        assert_eq!(resp["result"]["structuredContent"]["reason"], "unavailable");
    }

    #[test]
    fn mcp_reader_supports_both_ndjson_and_content_length() {
        let msg2_body = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}";
        let stream = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}}\nContent-Length: {}\r\n\r\n{}",
            msg2_body.len(),
            msg2_body
        );
        let mut reader = Cursor::new(stream.into_bytes());

        let msg1 = read_mcp_message(&mut reader).unwrap().unwrap();
        assert_eq!(msg1["id"], 1);

        let msg2 = read_mcp_message(&mut reader).unwrap().unwrap();
        assert_eq!(msg2["id"], 2);

        let msg3 = read_mcp_message(&mut reader).unwrap();
        assert!(msg3.is_none());
    }
}
