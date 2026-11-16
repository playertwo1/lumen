//! Ponte stdio <-> TCP loopback autenticada para `coucou-hook --mcp`.
//!
//! Usa apenas `std` + `serde_json` para não introduzir dependências no binário
//! enxuto de hook. Nunca escreve logs em `stdout`: apenas respostas JSON-RPC 2.0
//! uma por linha.

use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

const ENDPOINT_FILE_NAME: &str = "mochi-mcp-endpoint.json";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const ERR_SERVER_UNAVAILABLE: i64 = -32001;
const ERR_PARSE: i64 = -32700;

struct Endpoint {
    port: u16,
    token: String,
}

struct ClientConn {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

/// Ponto de entrada do modo `--mcp` do `coucou-hook`.
pub fn run() -> i32 {
    let endpoint_path = default_endpoint_path();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match run_with_endpoint_and_stdio(&endpoint_path, stdin.lock(), stdout.lock()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

pub fn default_endpoint_path() -> PathBuf {
    default_endpoint_path_with(cfg!(windows), &|var| std::env::var_os(var).map(PathBuf::from))
}

fn is_absolute_for_platform(is_windows: bool, p: &Path) -> bool {
    if is_windows {
        p.is_absolute()
    } else {
        p.to_str().is_some_and(|s| s.starts_with('/'))
    }
}

fn default_endpoint_path_with(
    is_windows: bool,
    env: &dyn Fn(&str) -> Option<PathBuf>,
) -> PathBuf {
    if is_windows {
        let base = env("LOCALAPPDATA").unwrap_or_else(|| PathBuf::from("."));
        return base.join("Coucou").join(ENDPOINT_FILE_NAME);
    }

    let xdg_data = env("XDG_DATA_HOME").filter(|p| is_absolute_for_platform(false, p));
    let data_home = match xdg_data {
        Some(dir) => dir,
        None => env("HOME")
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("share"),
    };
    data_home.join("coucou").join(ENDPOINT_FILE_NAME)
}

/// Lê linhas NDJSON de `reader` e ponteia cada mensagem com o servidor MCP loopback.
pub fn run_with_endpoint_and_stdio<R: BufRead, W: Write>(
    endpoint_path: &Path,
    mut reader: R,
    mut writer: W,
) -> Result<(), String> {
    let mut conn: Option<ClientConn> = None;
    let mut line = String::new();
    let mut first_line = true;

    loop {
        line.clear();
        let read_bytes = reader
            .read_line(&mut line)
            .map_err(|e| format!("falha ao ler stdin: {e}"))?;
        if read_bytes == 0 {
            break;
        }

        let mut trimmed = line.trim();
        if first_line {
            first_line = false;
            trimmed = trimmed.strip_prefix('\u{FEFF}').unwrap_or(trimmed).trim();
        }
        if trimmed.is_empty() {
            continue;
        }

        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => {
                let err = json_rpc_error(Value::Null, ERR_PARSE, "Parse error");
                write_rpc_line(&mut writer, &err)?;
                continue;
            }
        };

        let silent = is_silent_notification_value(&parsed);

        if conn.is_none() {
            conn = connect_and_auth(endpoint_path).ok();
        }

        let Some(active) = conn.as_mut() else {
            if !silent {
                let err = json_rpc_error(
                    error_reply_id(&parsed),
                    ERR_SERVER_UNAVAILABLE,
                    "Lumen MCP server unavailable",
                );
                write_rpc_line(&mut writer, &err)?;
            }
            continue;
        };

        let reply = forward_one(active, trimmed, !silent);
        match reply {
            Ok(Some(resp_line)) => {
                writeln!(writer, "{resp_line}")
                    .map_err(|e| format!("falha ao escrever stdout: {e}"))?;
                writer
                    .flush()
                    .map_err(|e| format!("falha ao descarregar stdout: {e}"))?;
            }
            Ok(None) => {}
            Err(_) => {
                conn = None;
                if !silent {
                    let err = json_rpc_error(
                        error_reply_id(&parsed),
                        ERR_SERVER_UNAVAILABLE,
                        "Lumen MCP server connection lost",
                    );
                    write_rpc_line(&mut writer, &err)?;
                }
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
fn is_silent_notification(line: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    is_silent_notification_value(&parsed)
}

fn is_silent_notification_value(parsed: &Value) -> bool {
    let Some(obj) = parsed.as_object() else {
        return false;
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return false;
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return false;
    };
    method.starts_with("notifications/") && !obj.contains_key("id")
}

fn error_reply_id(parsed: &Value) -> Value {
    parsed
        .as_object()
        .and_then(|obj| obj.get("id"))
        .filter(|id| id.is_string() || id.is_number() || id.is_null())
        .cloned()
        .unwrap_or(Value::Null)
}

fn read_endpoint(path: &Path) -> Result<Endpoint, String> {
    let bytes = fs::read(path).map_err(|e| format!("endpoint ausente: {e}"))?;
    let val: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("endpoint inválido: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "endpoint não é objeto".to_string())?;
    if obj.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("versão de endpoint incompatível".to_string());
    }
    let port_u64 = obj
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| "porta ausente".to_string())?;
    let port = u16::try_from(port_u64).map_err(|_| "porta fora do intervalo".to_string())?;
    if port == 0 {
        return Err("porta zero".to_string());
    }
    let token = obj
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "token ausente".to_string())?
        .to_string();
    Ok(Endpoint { port, token })
}

fn connect_and_auth(endpoint_path: &Path) -> Result<ClientConn, String> {
    let endpoint = read_endpoint(endpoint_path)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], endpoint.port));
    let stream =
        TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let read_stream = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(read_stream);
    let mut writer = BufWriter::new(stream);

    let auth_payload = json!({ "token": endpoint.token });
    let auth_line = serde_json::to_string(&auth_payload).map_err(|e| e.to_string())?;
    writeln!(writer, "{auth_line}").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;

    let mut ack = String::new();
    let n = reader.read_line(&mut ack).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("servidor fechou durante handshake".to_string());
    }
    let parsed_ack: Value = serde_json::from_str(ack.trim()).map_err(|e| e.to_string())?;
    if parsed_ack.get("ok") != Some(&Value::Bool(true)) {
        return Err("handshake recusado".to_string());
    }

    // Depois do handshake, `tools/call` pode aguardar o clique humano na ilha sem timeout curto.
    reader
        .get_ref()
        .set_read_timeout(None)
        .map_err(|e| e.to_string())?;
    writer
        .get_ref()
        .set_write_timeout(None)
        .map_err(|e| e.to_string())?;

    Ok(ClientConn { reader, writer })
}

fn forward_one(
    conn: &mut ClientConn,
    raw_line: &str,
    expects_reply: bool,
) -> Result<Option<String>, String> {
    writeln!(conn.writer, "{raw_line}").map_err(|e| e.to_string())?;
    conn.writer.flush().map_err(|e| e.to_string())?;

    if !expects_reply {
        return Ok(None);
    }

    let mut reply = String::new();
    let n = conn.reader.read_line(&mut reply).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("conexão TCP encerrada".to_string());
    }
    Ok(Some(reply.trim_end_matches(['\r', '\n']).to_string()))
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

fn write_rpc_line<W: Write>(writer: &mut W, value: &Value) -> Result<(), String> {
    let encoded = serde_json::to_string(value)
        .map_err(|e| format!("falha ao serializar erro JSON-RPC: {e}"))?;
    writeln!(writer, "{encoded}").map_err(|e| format!("falha ao escrever stdout: {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("falha ao descarregar stdout: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "lumen-hook-mcp-{prefix}-{}-{seq}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self { path: dir }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn default_endpoint_path_matches_platform_conventions() {
        let win = default_endpoint_path_with(true, &|k| match k {
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\alice\AppData\Local")),
            _ => None,
        });
        assert_eq!(
            win,
            PathBuf::from(r"C:\Users\alice\AppData\Local")
                .join("Coucou")
                .join("mochi-mcp-endpoint.json")
        );

        let xdg = default_endpoint_path_with(false, &|k| match k {
            "XDG_DATA_HOME" => Some(PathBuf::from("/tmp/xdg-data")),
            "HOME" => Some(PathBuf::from("/home/alice")),
            _ => None,
        });
        assert_eq!(
            xdg,
            PathBuf::from("/tmp/xdg-data")
                .join("coucou")
                .join("mochi-mcp-endpoint.json")
        );

        let linux_fallback = default_endpoint_path_with(false, &|k| match k {
            "HOME" => Some(PathBuf::from("/home/alice")),
            _ => None,
        });
        assert_eq!(
            linux_fallback,
            PathBuf::from("/home/alice")
                .join(".local")
                .join("share")
                .join("coucou")
                .join("mochi-mcp-endpoint.json")
        );
    }

    #[test]
    fn bridge_authenticates_and_relays_initialize_tools_list_and_ping_call() {
        let dir = TempDir::new("bridge");
        let endpoint_path = dir.path.join(ENDPOINT_FILE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "ab".repeat(32);

        fs::write(
            &endpoint_path,
            serde_json::to_vec(&json!({
                "version": 1,
                "port": port,
                "token": token,
                "pid": 1234,
            }))
            .unwrap(),
        )
        .unwrap();

        let expected_token = token.clone();
        let server_thread = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = BufWriter::new(stream);

            let mut auth_line = String::new();
            reader.read_line(&mut auth_line).unwrap();
            let auth: Value = serde_json::from_str(auth_line.trim()).unwrap();
            assert_eq!(auth, json!({ "token": expected_token }));
            writeln!(writer, "{}", json!({ "ok": true })).unwrap();
            writer.flush().unwrap();

            let mut line = String::new();
            let mut seen_methods = Vec::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                let req: Value = serde_json::from_str(line.trim()).unwrap();
                let method = req["method"].as_str().unwrap().to_string();
                seen_methods.push(method.clone());
                if let Some(id) = req.get("id").cloned() {
                    let result = match method.as_str() {
                        "initialize" => json!({ "protocolVersion": "2025-06-18" }),
                        "tools/list" => json!({ "tools": [{ "name": "ping" }] }),
                        "tools/call" => {
                            assert_eq!(req["params"]["name"], "ping");
                            json!({
                                "content": [{ "type": "text", "text": "{\"status\":\"ok\"}" }],
                                "isError": false
                            })
                        }
                        other => panic!("unexpected method {other}"),
                    };
                    writeln!(
                        writer,
                        "{}",
                        json!({ "jsonrpc": "2.0", "id": id, "result": result })
                    )
                    .unwrap();
                    writer.flush().unwrap();
                }
                line.clear();
            }
            seen_methods
        });

        let stdin_data = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"ping\",\"arguments\":{}}}\n"
        );
        let mut stdout_buf = Vec::new();
        run_with_endpoint_and_stdio(
            &endpoint_path,
            Cursor::new(stdin_data.as_bytes()),
            &mut stdout_buf,
        )
        .unwrap();

        let seen = server_thread.join().unwrap();
        assert_eq!(
            seen,
            vec![
                "initialize",
                "notifications/initialized",
                "tools/list",
                "tools/call"
            ]
        );

        let out_str = String::from_utf8(stdout_buf).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 3);
        let r1: Value = serde_json::from_str(lines[0]).unwrap();
        let r2: Value = serde_json::from_str(lines[1]).unwrap();
        let r3: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(r1["id"], 1);
        assert_eq!(r2["id"], 2);
        assert_eq!(r2["result"]["tools"][0]["name"], "ping");
        assert_eq!(r3["id"], 3);
        assert_eq!(r3["result"]["isError"], false);
    }

    #[test]
    fn missing_endpoint_returns_jsonrpc_32001_and_exits_cleanly() {
        let dir = TempDir::new("missing");
        let endpoint_path = dir.path.join(ENDPOINT_FILE_NAME);

        let stdin_data = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":\"list-1\",\"method\":\"tools/list\"}\n"
        );
        let mut stdout_buf = Vec::new();
        run_with_endpoint_and_stdio(
            &endpoint_path,
            Cursor::new(stdin_data.as_bytes()),
            &mut stdout_buf,
        )
        .unwrap();

        let out_str = String::from_utf8(stdout_buf).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 2);

        let r1: Value = serde_json::from_str(lines[0]).unwrap();
        let r2: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(r1["id"], 1);
        assert_eq!(r1["error"]["code"], -32001);
        assert_eq!(r2["id"], "list-1");
        assert_eq!(r2["error"]["code"], -32001);
    }

    #[test]
    fn unreachable_server_returns_jsonrpc_32001_gracefully() {
        let dir = TempDir::new("closed");
        let endpoint_path = dir.path.join(ENDPOINT_FILE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = listener.local_addr().unwrap().port();
        drop(listener);

        fs::write(
            &endpoint_path,
            serde_json::to_vec(&json!({
                "version": 1,
                "port": closed_port,
                "token": "cd".repeat(32),
                "pid": 9999,
            }))
            .unwrap(),
        )
        .unwrap();

        let mut stdout_buf = Vec::new();
        run_with_endpoint_and_stdio(
            &endpoint_path,
            Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"ping\"}\n".as_slice()),
            &mut stdout_buf,
        )
        .unwrap();

        let out_str = String::from_utf8(stdout_buf).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 1);
        let resp: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(resp["id"], 42);
        assert_eq!(resp["error"]["code"], -32001);
    }

    #[test]
    fn invalid_requests_without_id_stay_synchronized_before_ping_call() {
        assert!(!is_silent_notification(r#"{"foo":1}"#));
        assert!(!is_silent_notification("[1]"));
        assert!(!is_silent_notification(r#"{"jsonrpc":"2.0","method":"ping"}"#));
        assert!(!is_silent_notification(
            r#"{"jsonrpc":"2.0","id":1,"method":"notifications/initialized"}"#
        ));
        assert!(is_silent_notification(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        ));
        assert!(is_silent_notification(
            r#"{"jsonrpc":"2.0","method":"notifications/custom"}"#
        ));

        let dir = TempDir::new("sync-invalid");
        let endpoint_path = dir.path.join(ENDPOINT_FILE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "ef".repeat(32);

        fs::write(
            &endpoint_path,
            serde_json::to_vec(&json!({
                "version": 1,
                "port": port,
                "token": token,
                "pid": 4321,
            }))
            .unwrap(),
        )
        .unwrap();

        let expected_token = token.clone();
        let server_thread = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = BufWriter::new(stream);

            let mut auth_line = String::new();
            reader.read_line(&mut auth_line).unwrap();
            let auth: Value = serde_json::from_str(auth_line.trim()).unwrap();
            assert_eq!(auth, json!({ "token": expected_token }));
            writeln!(writer, "{}", json!({ "ok": true })).unwrap();
            writer.flush().unwrap();

            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                let req: Value = serde_json::from_str(line.trim()).unwrap();
                let resp = if req.get("method").and_then(Value::as_str) == Some("tools/call") {
                    json!({
                        "jsonrpc": "2.0",
                        "id": req["id"],
                        "result": {
                            "content": [{ "type": "text", "text": "{\"status\":\"ok\"}" }],
                            "isError": false
                        }
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32600, "message": "Invalid Request" }
                    })
                };
                writeln!(writer, "{resp}").unwrap();
                writer.flush().unwrap();
                line.clear();
            }
        });

        let stdin_data = concat!(
            "{\"foo\":1}\n",
            "[1]\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"ping\",\"arguments\":{}}}\n"
        );
        let mut stdout_buf = Vec::new();
        run_with_endpoint_and_stdio(
            &endpoint_path,
            Cursor::new(stdin_data.as_bytes()),
            &mut stdout_buf,
        )
        .unwrap();
        server_thread.join().unwrap();

        let out_str = String::from_utf8(stdout_buf).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 3);
        let r1: Value = serde_json::from_str(lines[0]).unwrap();
        let r2: Value = serde_json::from_str(lines[1]).unwrap();
        let r3: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(r1["id"], Value::Null);
        assert_eq!(r1["error"]["code"], -32600);
        assert_eq!(r2["id"], Value::Null);
        assert_eq!(r2["error"]["code"], -32600);
        assert_eq!(r3["id"], 2);
        assert_eq!(r3["result"]["isError"], false);
    }
}
