//! Servidor MCP local loopback autenticado do núcleo Mochi/Lumen.
//!
//! Expõe exclusivamente em `127.0.0.1:0` (nunca `0.0.0.0`) um protocolo NDJSON
//! protegido por handshake de token aleatório de 32 bytes (comparado em tempo
//! constante via `subtle`) para ser ponteado pelo `--mcp` do `coucou-hook`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tauri::AppHandle;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::approval_bridge::TauriApprovalRequester;
use super::permissions::{ApprovalRequester, DefaultDenyRequester};
use super::MochiRuntime;

pub const MCP_ENDPOINT_FILE_NAME: &str = "mochi-mcp-endpoint.json";
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

const AUTH_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_AUTH_LINE_BYTES: usize = 4 * 1024;
const MAX_RPC_LINE_BYTES: usize = 1024 * 1024;

/// Metadados de descoberta local gravados atomicamente enquanto o servidor está ativo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpEndpoint {
    pub version: u32,
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthRequest {
    token: String,
}

#[derive(Clone)]
enum McpRequesterSource {
    App(AppHandle),
    Custom(Arc<dyn ApprovalRequester>),
    #[cfg(test)]
    Factory(Arc<dyn Fn(&MochiRuntime, u64) -> Arc<dyn ApprovalRequester> + Send + Sync>),
}

struct McpActiveRunGuard<'a> {
    runtime: &'a MochiRuntime,
    run_id: u64,
}

impl Drop for McpActiveRunGuard<'_> {
    fn drop(&mut self) {
        self.runtime.finish_run(self.run_id);
    }
}

/// Handle de ciclo de vida do servidor MCP local.
#[derive(Debug)]
pub struct McpServerHandle {
    port: u16,
    endpoint_path: PathBuf,
    shutdown_tx: watch::Sender<bool>,
    accept_task: tokio::task::JoinHandle<()>,
    stopped: AtomicBool,
}

impl McpServerHandle {
    /// Caminho padrão do arquivo de descoberta em `local_dir()`.
    #[allow(dead_code)]
    pub fn default_endpoint_path() -> PathBuf {
        default_endpoint_path()
    }

    /// Inicia o listener loopback em `127.0.0.1:0`, grava o endpoint e aceita clientes autenticados.
    pub async fn start(
        runtime: MochiRuntime,
        app: Option<AppHandle>,
        endpoint_path: PathBuf,
    ) -> Result<Self, String> {
        let requester_source = match app {
            Some(app_handle) => McpRequesterSource::App(app_handle),
            None => McpRequesterSource::Custom(Arc::new(DefaultDenyRequester)),
        };
        Self::start_with_source(runtime, requester_source, endpoint_path).await
    }

    /// Inicia o servidor com um `ApprovalRequester` explícito (isolando testes unitários da UI Tauri).
    pub async fn start_with_requester(
        runtime: MochiRuntime,
        requester: Arc<dyn ApprovalRequester>,
        endpoint_path: PathBuf,
    ) -> Result<Self, String> {
        Self::start_with_source(runtime, McpRequesterSource::Custom(requester), endpoint_path).await
    }

    #[cfg(test)]
    pub(crate) async fn start_with_requester_factory(
        runtime: MochiRuntime,
        factory: Arc<dyn Fn(&MochiRuntime, u64) -> Arc<dyn ApprovalRequester> + Send + Sync>,
        endpoint_path: PathBuf,
    ) -> Result<Self, String> {
        Self::start_with_source(runtime, McpRequesterSource::Factory(factory), endpoint_path).await
    }

    async fn start_with_source(
        runtime: MochiRuntime,
        requester_source: McpRequesterSource,
        endpoint_path: PathBuf,
    ) -> Result<Self, String> {
        let token = generate_token_hex()?;
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .map_err(|e| format!("falha ao abrir listener MCP em 127.0.0.1:0: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("falha ao ler porta do listener MCP: {e}"))?
            .port();

        let endpoint = McpEndpoint {
            version: 1,
            port,
            token: token.clone(),
            pid: std::process::id(),
        };
        write_endpoint_file(&endpoint_path, &endpoint)?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let detached_runtime = runtime.detached_for_mcp();
        let accept_task = tokio::spawn(accept_loop(
            listener,
            detached_runtime,
            requester_source,
            token,
            shutdown_rx,
        ));

        Ok(Self {
            port,
            endpoint_path,
            shutdown_tx,
            accept_task,
            stopped: AtomicBool::new(false),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    #[allow(dead_code)]
    pub fn endpoint_path(&self) -> &Path {
        &self.endpoint_path
    }

    /// Encerra o servidor e remove o arquivo de descoberta.
    pub fn shutdown(&self) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.shutdown_tx.send(true);
        self.accept_task.abort();
        let _ = std::fs::remove_file(&self.endpoint_path);
    }
}

impl Drop for McpServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Caminho padrão do arquivo de descoberta do servidor MCP local.
pub fn default_endpoint_path() -> PathBuf {
    crate::platform::local_dir().join(MCP_ENDPOINT_FILE_NAME)
}

/// Aceita apenas pares em endereço de loopback canônico (`127.0.0.0/8` ou `::1`).
pub fn is_allowed_peer(addr: SocketAddr) -> bool {
    addr.ip().to_canonical().is_loopback()
}

fn generate_token_hex() -> Result<String, String> {
    let rng = SystemRandom::new();
    let mut raw = [0u8; 32];
    rng.fill(&mut raw)
        .map_err(|_| "falha ao gerar entropia para o token MCP".to_string())?;
    let mut hex = String::with_capacity(64);
    for byte in raw {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Ok(hex)
}

fn constant_time_str_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    a_bytes.len() == b_bytes.len() && bool::from(a_bytes.ct_eq(b_bytes))
}

fn write_endpoint_file(path: &Path, endpoint: &McpEndpoint) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        crate::platform::ensure_private_dir(parent)
            .map_err(|e| format!("falha ao preparar diretório de endpoint MCP: {e}"))?;
    }
    let bytes = serde_json::to_vec(endpoint)
        .map_err(|e| format!("falha ao serializar endpoint MCP: {e}"))?;
    crate::tasks::store::write_atomic(path, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("falha ao proteger permissões do endpoint MCP: {e}"))?;
    }
    Ok(())
}

async fn accept_loop(
    listener: TcpListener,
    runtime: MochiRuntime,
    requester_source: McpRequesterSource,
    expected_token: String,
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
                let (stream, peer_addr) = match accepted {
                    Ok(pair) => pair,
                    Err(err) => {
                        crate::log::line(format!("mochi mcp accept error: {err}"));
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                if !is_allowed_peer(peer_addr) {
                    drop(stream);
                    continue;
                }
                let runtime_clone = runtime.clone();
                let source_clone = requester_source.clone();
                let token_clone = expected_token.clone();
                let shutdown_clone = shutdown_rx.clone();
                tokio::spawn(async move {
                    handle_connection(
                        stream,
                        runtime_clone,
                        source_clone,
                        token_clone,
                        shutdown_clone,
                    )
                    .await;
                });
            }
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    runtime: MochiRuntime,
    requester_source: McpRequesterSource,
    expected_token: String,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let auth_line = match tokio::time::timeout(
        AUTH_TIMEOUT,
        read_bounded_line(&mut reader, MAX_AUTH_LINE_BYTES),
    )
    .await
    {
        Ok(Ok(Some(line))) => line,
        _ => return,
    };

    let Ok(auth) = serde_json::from_slice::<AuthRequest>(&auth_line) else {
        return;
    };
    if !constant_time_str_eq(&auth.token, &expected_token) {
        return;
    }

    if write_half.write_all(b"{\"ok\":true}\n").await.is_err() {
        return;
    }
    if write_half.flush().await.is_err() {
        return;
    }

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        let line_result = tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
            res = read_bounded_line(&mut reader, MAX_RPC_LINE_BYTES) => res,
        };

        let line = match line_result {
            Ok(Some(bytes)) => bytes,
            Ok(None) => break,
            Err(ReadLineError::TooLarge) => {
                let err = rpc_error(
                    Value::Null,
                    -32600,
                    "Invalid Request: linha JSON-RPC excede o limite de 1 MiB.",
                );
                let _ = write_json_line(&mut write_half, &err).await;
                break;
            }
            Err(ReadLineError::Io) => break,
        };

        if line.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }

        if let Some(reply) =
            process_rpc_bytes(&line, &runtime, &requester_source, &shutdown_rx).await
        {
            if write_json_line(&mut write_half, &reply).await.is_err() {
                break;
            }
        }
    }
}

#[derive(Debug)]
enum ReadLineError {
    TooLarge,
    Io,
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, ReadLineError> {
    let mut buf = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|_| ReadLineError::Io)?;
        if available.is_empty() {
            if buf.is_empty() {
                return Ok(None);
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Some(buf));
        }

        if let Some(nl_pos) = available.iter().position(|&b| b == b'\n') {
            let take = nl_pos + 1;
            if buf.len().saturating_add(take) > max_bytes {
                return Err(ReadLineError::TooLarge);
            }
            buf.extend_from_slice(&available[..nl_pos]);
            reader.consume(take);
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Some(buf));
        }

        let len = available.len();
        if buf.len().saturating_add(len) > max_bytes {
            return Err(ReadLineError::TooLarge);
        }
        buf.extend_from_slice(available);
        reader.consume(len);
    }
}

async fn write_json_line<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await
}

fn is_valid_rpc_id(id: &Value) -> bool {
    id.is_string() || id.is_number() || id.is_null()
}

fn rpc_ok(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

async fn process_rpc_bytes(
    bytes: &[u8],
    runtime: &MochiRuntime,
    requester_source: &McpRequesterSource,
    shutdown_rx: &watch::Receiver<bool>,
) -> Option<Value> {
    let req: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(_) => return Some(rpc_error(Value::Null, -32700, "Parse error")),
    };

    let Some(obj) = req.as_object() else {
        return Some(rpc_error(
            Value::Null,
            -32600,
            "Invalid Request: payload deve ser um objeto JSON.",
        ));
    };

    let raw_id = obj.get("id").cloned();
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(rpc_error(
            raw_id.filter(is_valid_rpc_id).unwrap_or(Value::Null),
            -32600,
            "Invalid Request: 'jsonrpc' deve ser '2.0'.",
        ));
    }

    if let Some(ref id_val) = raw_id {
        if !is_valid_rpc_id(id_val) {
            return Some(rpc_error(
                Value::Null,
                -32600,
                "Invalid Request: 'id' inválido.",
            ));
        }
    }

    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Some(rpc_error(
            raw_id.unwrap_or(Value::Null),
            -32600,
            "Invalid Request: 'method' obrigatório.",
        ));
    };

    if raw_id.is_none() && method.starts_with("notifications/") {
        return None;
    }

    let Some(reply_id) = raw_id else {
        return Some(rpc_error(
            Value::Null,
            -32600,
            "Invalid Request: 'id' obrigatório.",
        ));
    };

    Some(
        dispatch_rpc_request(
            reply_id,
            method,
            obj.get("params"),
            runtime,
            requester_source,
            shutdown_rx,
        )
        .await,
    )
}

async fn dispatch_rpc_request(
    id: Value,
    method: &str,
    params: Option<&Value>,
    runtime: &MochiRuntime,
    requester_source: &McpRequesterSource,
    shutdown_rx: &watch::Receiver<bool>,
) -> Value {
    match method {
        "initialize" => rpc_ok(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "lumen-mochi",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        ),
        "ping" => rpc_ok(id, json!({})),
        "tools/list" => {
            let tools: Vec<Value> = runtime
                .list_available(&super::host::context(crate::integrations::PAUSED.load(Ordering::Relaxed)).await)
                .into_iter()
                .map(|desc| {
                    json!({
                        "name": desc.id,
                        "description": desc.description,
                        "inputSchema": desc.input_schema,
                    })
                })
                .collect();
            rpc_ok(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let Some(params_obj) = params.and_then(Value::as_object) else {
                return rpc_error(
                    id,
                    -32602,
                    "Invalid params: 'params' deve ser um objeto JSON.",
                );
            };
            let Some(name) = params_obj
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                return rpc_error(
                    id,
                    -32602,
                    "Invalid params: 'params.name' é obrigatório.",
                );
            };
            let arguments = match params_obj.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(v @ Value::Object(_)) => v.clone(),
                Some(_) => {
                    return rpc_error(
                        id,
                        -32602,
                        "Invalid params: 'params.arguments' deve ser um objeto JSON.",
                    );
                }
            };

            let (run_id, mut run_cancel_rx) = runtime.begin_run();
            let _run_guard = McpActiveRunGuard { runtime, run_id };
            let requester_arc: Arc<dyn ApprovalRequester> = match requester_source {
                McpRequesterSource::App(app_handle) => Arc::new(TauriApprovalRequester::for_run(
                    app_handle.clone(),
                    runtime.clone(),
                    run_id,
                )),
                McpRequesterSource::Custom(req) => Arc::clone(req),
                #[cfg(test)]
                McpRequesterSource::Factory(factory) => factory(runtime, run_id),
            };

            let (combined_tx, combined_rx) =
                watch::channel(*shutdown_rx.borrow() || *run_cancel_rx.borrow());
            let mut shutdown_watch = shutdown_rx.clone();
            let cancel_relay = tokio::spawn(async move {
                tokio::select! {
                    _ = super::wait_for_cancel(&mut shutdown_watch) => {
                        let _ = combined_tx.send(true);
                    }
                    _ = super::wait_for_cancel(&mut run_cancel_rx) => {
                        let _ = combined_tx.send(true);
                    }
                }
            });

            let outcome = runtime
                .invoke_tool(
                    name,
                    arguments,
                    // A pausa rápida corta também os pedidos que chegam pelo MCP.
                    &super::host::context(crate::integrations::PAUSED.load(Ordering::Relaxed)).await,
                    requester_arc.as_ref(),
                    combined_rx,
                )
                .await;
            cancel_relay.abort();

            let result = match outcome {
                Ok(ok) => {
                    let text = match &ok.output {
                        Value::String(s) => s.clone(),
                        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
                    };
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false,
                    })
                }
                Err(err) => json!({
                    "content": [{ "type": "text", "text": err.to_string() }],
                    "isError": true,
                }),
            };
            rpc_ok(id, result)
        }
        other => rpc_error(id, -32601, format!("Method not found: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};

    use crate::mochi::audit::{AuditDecision, AuditLog, AuditResult};
    use crate::mochi::permissions::{bootstrap_default_if_first_run, PermissionEngine};
    use crate::mochi::registry::{
        Availability, ExecutionContext, RiskLevel, ToolDefinition, ToolFuture, ToolRegistry,
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "lumen-mcp-server-{prefix}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { path: dir }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    async fn start_test_server(
        runtime: MochiRuntime,
        endpoint_path: PathBuf,
    ) -> McpServerHandle {
        McpServerHandle::start_with_requester(
            runtime,
            Arc::new(DefaultDenyRequester),
            endpoint_path,
        )
        .await
        .unwrap()
    }

    fn test_runtime(dir: &Path, registry: ToolRegistry) -> MochiRuntime {
        let perms_path = dir.join("permissions.json");
        let marker_path = dir.join("permissions.init");
        bootstrap_default_if_first_run(&perms_path, &marker_path).unwrap();
        let permissions = PermissionEngine::load_from(perms_path);
        let audit = AuditLog::load_from(dir.join("audit.jsonl"));
        MochiRuntime::new_with_parts(
            Arc::new(registry),
            Arc::new(permissions),
            Arc::new(audit),
        )
    }

    async fn connect_and_auth(
        port: u16,
        token: &str,
    ) -> (
        BufReader<tokio::net::tcp::OwnedReadHalf>,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        let stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let auth_line = format!("{}\n", json!({ "token": token }));
        write_half.write_all(auth_line.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();

        let mut reply = String::new();
        reader.read_line(&mut reply).await.unwrap();
        let parsed: Value = serde_json::from_str(reply.trim()).unwrap();
        assert_eq!(parsed, json!({ "ok": true }));
        (reader, write_half)
    }

    async fn rpc_roundtrip(
        reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
        writer: &mut tokio::net::tcp::OwnedWriteHalf,
        payload: Value,
    ) -> Value {
        let line = format!("{}\n", serde_json::to_string(&payload).unwrap());
        writer.write_all(line.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        let mut reply = String::new();
        reader.read_line(&mut reply).await.unwrap();
        serde_json::from_str(reply.trim()).unwrap()
    }

    #[test]
    fn is_allowed_peer_accepts_only_loopback_addresses() {
        assert!(is_allowed_peer(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            1234
        )));
        assert!(is_allowed_peer(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 42)),
            80
        )));
        assert!(is_allowed_peer(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            443
        )));
        assert!(is_allowed_peer(SocketAddr::new(
            IpAddr::V6("::ffff:127.0.0.1".parse().unwrap()),
            9000
        )));

        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            1234
        )));
        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            1234
        )));
        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            443
        )));
        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V6("::ffff:192.168.1.10".parse().unwrap()),
            1234
        )));
        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            1234
        )));
        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            1234
        )));
        assert!(!is_allowed_peer(SocketAddr::new(
            IpAddr::V6("2001:db8::1".parse().unwrap()),
            1234
        )));
    }

    #[tokio::test]
    async fn wrong_token_closes_connection_immediately() {
        let dir = TestDir::new("bad-token");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let runtime = test_runtime(&dir.path, ToolRegistry::with_default_tools());
        let handle = start_test_server(runtime, endpoint_path).await;

        let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], handle.port())))
            .await
            .unwrap();
        let wrong = "0".repeat(64);
        let line = format!("{}\n", json!({ "token": wrong }));
        stream.write_all(line.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();

        let mut buf = Vec::new();
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());

        handle.shutdown();
    }

    #[tokio::test]
    async fn http_payload_is_rejected_and_closed() {
        let dir = TestDir::new("http");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let runtime = test_runtime(&dir.path, ToolRegistry::with_default_tools());
        let handle = start_test_server(runtime, endpoint_path).await;

        let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], handle.port())))
            .await
            .unwrap();
        stream
            .write_all(b"POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let mut buf = Vec::new();
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());

        handle.shutdown();
    }

    fn unavailable_tool() -> ToolDefinition {
        fn exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            Box::pin(async { Ok(json!({ "ok": true })) })
        }
        ToolDefinition {
            id: "missing_dep",
            description: "Ferramenta indisponível no teste",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_secs(2),
            check_availability: |_| Availability::Unavailable {
                reason: "adapter ausente".to_string(),
            },
            dry_run: |_| Ok("unavailable".to_string()),
            execute: exec,
            verify: |_, _| Ok(()),
            audit_summary: |_, _| "unavailable".to_string(),
        }
    }

    #[tokio::test]
    async fn initialize_ping_and_tools_list_omits_unavailable() {
        let dir = TestDir::new("list");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(unavailable_tool()).unwrap();

        let disabled_only = ToolDefinition {
            id: "disabled_tool",
            description: "Nunca habilitada",
            ..unavailable_tool()
        };
        registry.register(disabled_only).unwrap();

        let runtime = test_runtime(&dir.path, registry);
        let handle = start_test_server(runtime, endpoint_path.clone()).await;
        let endpoint: McpEndpoint =
            serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();

        let (mut reader, mut writer) = connect_and_auth(endpoint.port, &endpoint.token).await;

        let init = rpc_roundtrip(
            &mut reader,
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1.0" } }
            }),
        )
        .await;
        assert_eq!(init["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], "lumen-mochi");

        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();

        let ping = rpc_roundtrip(
            &mut reader,
            &mut writer,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
        )
        .await;
        assert_eq!(ping["id"], 2);
        assert_eq!(ping["result"], json!({}));

        let list = rpc_roundtrip(
            &mut reader,
            &mut writer,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        )
        .await;
        let tools = list["result"]["tools"].as_array().unwrap();
        // bloquear_tela depende só do Windows; as demais ferramentas precisam de capacidade do computador.
        let nomes: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(nomes, if cfg!(windows) { vec!["bloquear_tela", "ping"] } else { vec!["ping"] });
        let ping = tools.iter().find(|t| t["name"] == "ping").unwrap();
        assert!(ping["inputSchema"].is_object());

        handle.shutdown();
    }

    #[tokio::test]
    async fn tools_call_ping_runs_through_permission_engine_and_logs_audit() {
        let dir = TestDir::new("call-ping");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let runtime = test_runtime(&dir.path, ToolRegistry::with_default_tools());
        let handle = start_test_server(runtime.clone(), endpoint_path.clone()).await;
        let endpoint: McpEndpoint =
            serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();

        let (mut reader, mut writer) = connect_and_auth(endpoint.port, &endpoint.token).await;
        let resp = rpc_roundtrip(
            &mut reader,
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": "call-1",
                "method": "tools/call",
                "params": {
                    "name": "ping"
                }
            }),
        )
        .await;

        assert_eq!(resp["id"], "call-1");
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["pong"], "lumen-mochi-m1");

        let entries = runtime.audit.list(Some(10));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tool_id, "ping");
        assert_eq!(entries[0].decision, AuditDecision::AutoLow);
        assert_eq!(entries[0].result, AuditResult::Success);

        handle.shutdown();
    }

    #[tokio::test]
    async fn tools_call_medium_tool_without_ui_is_denied_and_never_executes() {
        static EXEC_COUNT: AtomicUsize = AtomicUsize::new(0);
        fn medium_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            EXEC_COUNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(json!({ "ok": true })) })
        }

        EXEC_COUNT.store(0, Ordering::SeqCst);
        let dir = TestDir::new("medium-deny");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "create_note",
                description: "Cria nota local de teste",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::Medium,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Criar nota de teste.".to_string()),
                execute: medium_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "nota teste".to_string(),
            })
            .unwrap();

        let runtime = test_runtime(&dir.path, registry);
        let handle = start_test_server(runtime.clone(), endpoint_path.clone()).await;
        let endpoint: McpEndpoint =
            serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();

        let (mut reader, mut writer) = connect_and_auth(endpoint.port, &endpoint.token).await;
        let resp = rpc_roundtrip(
            &mut reader,
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": {
                    "name": "create_note",
                    "arguments": {}
                }
            }),
        )
        .await;

        assert_eq!(resp["id"], 7);
        assert_eq!(resp["result"]["isError"], true);
        let err_text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(!err_text.is_empty());
        assert_eq!(EXEC_COUNT.load(Ordering::SeqCst), 0);

        let entries = runtime.audit.list(Some(10));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tool_id, "create_note");
        assert_eq!(entries[0].decision, AuditDecision::Denied);
        assert_eq!(entries[0].result, AuditResult::Blocked);

        handle.shutdown();
    }

    #[tokio::test]
    async fn endpoint_file_is_written_on_start_and_removed_on_shutdown_and_drop() {
        let dir = TestDir::new("lifecycle");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let runtime = test_runtime(&dir.path, ToolRegistry::with_default_tools());

        let handle1 = start_test_server(runtime.clone(), endpoint_path.clone()).await;
        assert!(endpoint_path.exists());
        let parsed: McpEndpoint =
            serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.port, handle1.port());
        assert_eq!(parsed.token.len(), 64);
        assert!(parsed.token.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        assert_eq!(parsed.pid, std::process::id());

        handle1.shutdown();
        assert!(!endpoint_path.exists());

        let handle2 = start_test_server(runtime, endpoint_path.clone()).await;
        assert!(endpoint_path.exists());
        drop(handle2);
        assert!(!endpoint_path.exists());
    }

    #[tokio::test]
    async fn invalid_payload_without_id_replies_before_subsequent_ping_call() {
        let dir = TestDir::new("sync-invalid");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let runtime = test_runtime(&dir.path, ToolRegistry::with_default_tools());
        let handle = start_test_server(runtime, endpoint_path.clone()).await;
        let endpoint: McpEndpoint =
            serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();

        let (mut reader, mut writer) = connect_and_auth(endpoint.port, &endpoint.token).await;

        writer.write_all(b"{\"foo\":1}\n[1]\n").await.unwrap();
        writer.flush().await.unwrap();

        let mut line1 = String::new();
        reader.read_line(&mut line1).await.unwrap();
        let err1: Value = serde_json::from_str(line1.trim()).unwrap();
        assert_eq!(err1["id"], Value::Null);
        assert_eq!(err1["error"]["code"], -32600);

        let mut line2 = String::new();
        reader.read_line(&mut line2).await.unwrap();
        let err2: Value = serde_json::from_str(line2.trim()).unwrap();
        assert_eq!(err2["id"], Value::Null);
        assert_eq!(err2["error"]["code"], -32600);

        let resp = rpc_roundtrip(
            &mut reader,
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "ping", "arguments": {} }
            }),
        )
        .await;
        assert_eq!(resp["id"], 2);
        assert_eq!(resp["result"]["isError"], false);

        handle.shutdown();
    }

    #[tokio::test]
    async fn cancel_execution_while_mcp_awaits_click_cleans_pending_and_never_persists_always_allow() {
        use crate::mochi::approval_bridge::request_via_pending_for_run;
        use crate::mochi::permissions::{ApprovalFuture, ApprovalPromptRequest};
        use crate::pipe::Pending;

        static EXEC_COUNT: AtomicUsize = AtomicUsize::new(0);
        fn medium_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            EXEC_COUNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(json!({ "ok": true })) })
        }

        EXEC_COUNT.store(0, Ordering::SeqCst);
        let dir = TestDir::new("mcp-cancel-pending");
        let endpoint_path = dir.path.join(MCP_ENDPOINT_FILE_NAME);
        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "mcp_med_tool",
                description: "Ferramenta média via MCP",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::Medium,
                timeout: Duration::from_secs(5),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Alterar item de teste.".to_string()),
                execute: medium_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "mcp médio".to_string(),
            })
            .unwrap();

        let runtime = test_runtime(&dir.path, registry);
        let pending = Arc::new(Pending::default());
        let (waiting_tx, waiting_rx) = tokio::sync::oneshot::channel::<()>();
        let waiting_tx = Arc::new(std::sync::Mutex::new(Some(waiting_tx)));

        struct McpPendingRequester {
            pending: Arc<Pending>,
            runtime: MochiRuntime,
            run_id: u64,
            waiting_tx: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
        }

        impl ApprovalRequester for McpPendingRequester {
            fn request<'a>(&'a self, prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
                Box::pin(async move {
                    if let Some(tx) = self.waiting_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    request_via_pending_for_run(
                        &self.pending,
                        "mcp-req-1".to_string(),
                        prompt,
                        Duration::from_secs(5),
                        Duration::from_secs(5),
                        Some((self.runtime.clone(), self.run_id)),
                    )
                    .await
                })
            }
        }

        let pending_for_factory = Arc::clone(&pending);
        let waiting_for_factory = Arc::clone(&waiting_tx);
        let factory: Arc<dyn Fn(&MochiRuntime, u64) -> Arc<dyn ApprovalRequester> + Send + Sync> =
            Arc::new(move |rt, run_id| {
                Arc::new(McpPendingRequester {
                    pending: Arc::clone(&pending_for_factory),
                    runtime: rt.clone(),
                    run_id,
                    waiting_tx: Arc::clone(&waiting_for_factory),
                })
            });

        let handle = McpServerHandle::start_with_requester_factory(
            runtime.clone(),
            factory,
            endpoint_path.clone(),
        )
        .await
        .unwrap();
        let endpoint: McpEndpoint =
            serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();

        let (mut reader, mut writer) = connect_and_auth(endpoint.port, &endpoint.token).await;
        let call_line = format!(
            "{}\n",
            serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 99,
                "method": "tools/call",
                "params": { "name": "mcp_med_tool", "arguments": {} }
            }))
            .unwrap()
        );
        writer.write_all(call_line.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        // Aguarda a chamada MCP entrar na espera de clique.
        tokio::time::timeout(Duration::from_secs(2), waiting_rx)
            .await
            .unwrap()
            .unwrap();
        tokio::task::yield_now().await;

        // Cancela no runtime principal (mesmo active_run compartilhado com detached_for_mcp).
        assert!(runtime.cancel_execution(None));

        let mut reply = String::new();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut reply))
            .await
            .unwrap()
            .unwrap();
        let resp: Value = serde_json::from_str(reply.trim()).unwrap();
        assert_eq!(resp["id"], 99);
        assert_eq!(resp["result"]["isError"], true);
        let err_text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(err_text.contains("cancel"), "erro inesperado: {err_text}");

        // PendingMapGuard deve ter removido o pedido ao dropar a future de autorização.
        assert!(!pending.remove_mochi("mcp-req-1"));
        assert!(!runtime.cancel_execution(None));
        assert_eq!(EXEC_COUNT.load(Ordering::SeqCst), 0);
        assert!(runtime.permissions.status().always_allow.is_empty());

        handle.shutdown();
    }
}
