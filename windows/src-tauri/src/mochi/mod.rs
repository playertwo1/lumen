//! Núcleo de execução do assistente Mochi no Lumen.
//!
//! Orquestra pausa rápida, registro deny-by-default, validação estrita de entrada,
//! simulação (`dry_run`), autorização fail-closed (`PermissionEngine` + `PermitToken`),
//! execução sob timeout + cancelamento cooperativo (`tokio::select!`), verificação
//! obrigatória pós-execução (`verify`) e resumo para auditoria.

#![cfg_attr(not(test), allow(dead_code))]

pub mod approval_bridge;
pub mod audit;
pub mod chat_bridge;
pub mod mcp_config;
pub mod mcp_server;
pub mod permissions;
pub mod registry;
pub mod sanitize;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

pub const MOCHI_RUN_STATE_EVENT: &str = "mochi-run-state";
pub const MOCHI_PAUSED_EVENT: &str = "mochi-paused";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MochiRunState {
    Thinking,
    AwaitingClick,
    Executing,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MochiRunStatePayload {
    pub state: MochiRunState,
    pub tool: Option<String>,
    pub detail: Option<String>,
}

pub(crate) trait RunProgressObserver: Send + Sync {
    fn on_awaiting_click(&self, tool_id: &str);
    fn on_executing(&self, tool_id: &str);
}

struct ObservedRequester<'a> {
    inner: &'a dyn ApprovalRequester,
    tool_id: &'a str,
    observer: Option<&'a dyn RunProgressObserver>,
}

impl ApprovalRequester for ObservedRequester<'_> {
    fn request<'a>(&'a self, prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
        if let Some(obs) = self.observer {
            obs.on_awaiting_click(self.tool_id);
        }
        self.inner.request(prompt)
    }
}

#[derive(Debug)]
struct ActiveRun {
    run_id: u64,
    cancel_tx: watch::Sender<bool>,
    pending_request_id: Option<String>,
}

#[allow(unused_imports)]
pub use approval_bridge::TauriApprovalRequester;
#[allow(unused_imports)]
pub use audit::{
    redact_summary, AuditDecision, AuditEntry, AuditLog, AuditResult, AUDIT_FILE_NAME,
    MAX_AUDIT_ENTRIES,
};
#[allow(unused_imports)]
pub use permissions::{
    bootstrap_default_if_first_run, load_config_from, parse_config, ApprovalFuture,
    ApprovalPromptRequest, ApprovalRequester, DefaultDenyRequester, PermissionConfigFile,
    PermissionDecision, PermissionEngine, PermissionStatus, PermitToken, UserApprovalReply,
};
#[allow(unused_imports)]
pub use registry::{
    is_valid_tool_id, ping_tool, validate_schema, Availability, ExecutionContext, RiskLevel,
    ToolContext, ToolDefinition, ToolDescriptor, ToolError, ToolFuture, ToolRegistry,
};

/// Resultado completo de uma execução de ferramenta bem-sucedida e verificada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionOutcome {
    pub tool_id: String,
    pub risk: RiskLevel,
    pub decision: PermissionDecision,
    pub dry_run_summary: String,
    pub output: Value,
    pub audit_summary: String,
}

/// Runtime central do Mochi na camada Rust.
#[derive(Debug, Clone)]
pub struct MochiRuntime {
    pub registry: Arc<ToolRegistry>,
    pub permissions: Arc<PermissionEngine>,
    pub audit: Arc<AuditLog>,
    mcp: Arc<Mutex<Option<mcp_server::McpServerHandle>>>,
    next_run_id: Arc<AtomicU64>,
    active_run: Arc<Mutex<Option<ActiveRun>>>,
}

impl Default for MochiRuntime {
    fn default() -> Self {
        Self::new_default()
    }
}

impl MochiRuntime {
    /// Cria o runtime padrão (`ping` habilitada, permissões em `%APPDATA%\Coucou\mochi-permissions.json`
    /// e auditoria em `%LOCALAPPDATA%\Coucou\mochi-audit.json`).
    pub fn new_default() -> Self {
        Self::new_with_parts(
            Arc::new(ToolRegistry::with_default_tools()),
            Arc::new(PermissionEngine::new_default()),
            Arc::new(AuditLog::new_default()),
        )
    }

    /// Cria o runtime usando `ToolRegistry` e `PermissionEngine` explícitos.
    pub fn new(registry: Arc<ToolRegistry>, permissions: Arc<PermissionEngine>) -> Self {
        #[cfg(test)]
        let audit = Arc::new(AuditLog::new_in_memory());
        #[cfg(not(test))]
        let audit = Arc::new(AuditLog::new_default());
        Self::new_with_parts(registry, permissions, audit)
    }

    /// Cria o runtime usando `ToolRegistry`, `PermissionEngine` e `AuditLog` explícitos.
    pub fn new_with_parts(
        registry: Arc<ToolRegistry>,
        permissions: Arc<PermissionEngine>,
        audit: Arc<AuditLog>,
    ) -> Self {
        Self {
            registry,
            permissions,
            audit,
            mcp: Arc::new(Mutex::new(None)),
            next_run_id: Arc::new(AtomicU64::new(1)),
            active_run: Arc::new(Mutex::new(None)),
        }
    }

    /// Clona o runtime sem reter referência a `McpServerHandle` (evita ciclo no loop MCP).
    pub(crate) fn detached_for_mcp(&self) -> Self {
        Self {
            registry: Arc::clone(&self.registry),
            permissions: Arc::clone(&self.permissions),
            audit: Arc::clone(&self.audit),
            mcp: Arc::new(Mutex::new(None)),
            next_run_id: Arc::new(AtomicU64::new(1)),
            active_run: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn set_mcp_handle(&self, handle: mcp_server::McpServerHandle) {
        let mut guard = self.mcp.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = guard.take() {
            previous.shutdown();
        }
        *guard = Some(handle);
    }

    pub(crate) fn shutdown_mcp(&self) {
        let handle = {
            let mut guard = self.mcp.lock().unwrap_or_else(PoisonError::into_inner);
            guard.take()
        };
        if let Some(handle) = handle {
            handle.shutdown();
        }
    }

    /// Cria o runtime usando um `ToolRegistry` explícito e o `PermissionEngine` padrão.
    #[allow(dead_code)]
    pub fn new_with_registry(registry: Arc<ToolRegistry>) -> Self {
        Self::new(registry, Arc::new(PermissionEngine::new_default()))
    }

    /// Retorna referência ao registro de ferramentas.
    #[allow(dead_code)]
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// Retorna referência ao motor de permissões fail-closed.
    pub fn permissions(&self) -> &Arc<PermissionEngine> {
        &self.permissions
    }

    /// Retorna referência ao registro local redigido de auditoria.
    #[allow(dead_code)]
    pub fn audit(&self) -> &Arc<AuditLog> {
        &self.audit
    }

    /// Informa se a execução está bloqueada pela pausa rápida do contexto ou do aplicativo.
    pub fn is_paused(&self, ctx: &ToolContext) -> bool {
        ctx.paused || crate::integrations::PAUSED.load(Ordering::Relaxed)
    }

    /// Inicia o rastreamento de uma execução cancelável e devolve `(run_id, cancel_rx)`.
    pub fn begin_run(&self) -> (u64, watch::Receiver<bool>) {
        let run_id = self.next_run_id.fetch_add(1, Ordering::Relaxed);
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let previous = {
            let mut guard = self.active_run.lock().unwrap_or_else(PoisonError::into_inner);
            guard.replace(ActiveRun {
                run_id,
                cancel_tx,
                pending_request_id: None,
            })
        };
        if let Some(prev) = previous {
            let _ = prev.cancel_tx.send(true);
        }
        (run_id, cancel_rx)
    }

    /// Registra ou limpa o pedido de aprovação em aberto da execução ativa.
    pub fn set_pending_request(&self, run_id: u64, request_id: Option<String>) {
        let mut guard = self.active_run.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(active) = guard.as_mut() {
            if active.run_id == run_id {
                active.pending_request_id = request_id;
            }
        }
    }

    /// Encerra o rastreamento quando a execução termina normalmente ou falha.
    pub fn finish_run(&self, run_id: u64) {
        let mut guard = self.active_run.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_some_and(|active| active.run_id == run_id) {
            *guard = None;
        }
    }

    /// Cancela imediatamente a execução ativa (espera de modelo, espera de clique ou ferramenta).
    pub fn cancel_execution(&self, app: Option<&AppHandle>) -> bool {
        let taken = {
            let mut guard = self.active_run.lock().unwrap_or_else(PoisonError::into_inner);
            guard.take()
        };
        let Some(mut active) = taken else {
            return false;
        };
        let _ = active.cancel_tx.send(true);
        let pending_id = active.pending_request_id.take();
        if let Some(app) = app {
            if let Some(req_id) = pending_id {
                crate::pipe::expire_mochi_request(app, &req_id);
            }
            emit_run_state(
                app,
                MochiRunState::Failed,
                None,
                Some("Execução cancelada pelo usuário.".into()),
            );
        }
        true
    }

    /// Lista apenas ferramentas habilitadas e disponíveis para o contexto atual.
    pub fn list_available(&self, ctx: &ToolContext) -> Vec<ToolDescriptor> {
        self.registry.list_available(ctx)
    }

    fn known_risk_or_low(&self, tool_id: &str, tool_ctx: &ToolContext) -> RiskLevel {
        self.registry
            .get_if_available(tool_id, tool_ctx)
            .map(|def| def.risk)
            .unwrap_or(RiskLevel::Low)
    }

    fn record_audit(
        &self,
        tool_id: &str,
        risk: RiskLevel,
        decision: AuditDecision,
        result: AuditResult,
        summary: &str,
    ) -> Option<AuditEntry> {
        match self.audit.record(tool_id, risk, decision, result, summary) {
            Ok(entry) => Some(entry),
            Err(err) => {
                crate::log::line(format!("mochi audit write failed: {err}"));
                None
            }
        }
    }

    /// Pipeline único e obrigatório de execução de ferramentas:
    /// 1. pausa -> 2. registro/disponibilidade -> 3. schema estrito -> 4. dry_run ->
    /// 5. autorização (`PermissionEngine::authorize` -> `PermitToken`) ->
    /// 6. execução autenticada (`execute_with_permit`) -> 7. `verify` -> 8. `audit_summary` -> 9. `AuditLog`.
    pub async fn invoke_tool(
        &self,
        tool_id: &str,
        arguments: Value,
        tool_ctx: &ToolContext,
        requester: &dyn ApprovalRequester,
        cancel_rx: watch::Receiver<bool>,
    ) -> Result<ToolExecutionOutcome, ToolError> {
        self.invoke_tool_observed(tool_id, arguments, tool_ctx, requester, cancel_rx, None)
            .await
    }

    pub(crate) async fn invoke_tool_observed(
        &self,
        tool_id: &str,
        arguments: Value,
        tool_ctx: &ToolContext,
        requester: &dyn ApprovalRequester,
        cancel_rx: watch::Receiver<bool>,
        observer: Option<&dyn RunProgressObserver>,
    ) -> Result<ToolExecutionOutcome, ToolError> {
        if self.is_paused(tool_ctx) {
            let risk = self.known_risk_or_low(tool_id, tool_ctx);
            self.record_audit(
                tool_id,
                risk,
                AuditDecision::BlockedPaused,
                AuditResult::Blocked,
                &ToolError::Paused.to_string(),
            );
            return Err(ToolError::Paused);
        }
        if *cancel_rx.borrow() {
            let risk = self.known_risk_or_low(tool_id, tool_ctx);
            self.record_audit(
                tool_id,
                risk,
                AuditDecision::Cancelled,
                AuditResult::Cancelled,
                &ToolError::Cancelled.to_string(),
            );
            return Err(ToolError::Cancelled);
        }

        let tool = match self.registry.get_if_available(tool_id, tool_ctx) {
            Ok(tool) => tool,
            Err(err) => {
                self.record_audit(
                    tool_id,
                    RiskLevel::Low,
                    AuditDecision::Rejected,
                    AuditResult::Blocked,
                    &err.to_string(),
                );
                return Err(err);
            }
        };

        if let Err(err) = validate_schema(&tool.input_schema, &arguments) {
            self.record_audit(
                tool.id,
                tool.risk,
                AuditDecision::Rejected,
                AuditResult::Blocked,
                &err.to_string(),
            );
            return Err(err);
        }

        let dry_run_summary = match (tool.dry_run)(&arguments) {
            Ok(summary) => summary,
            Err(err) => {
                let result = match &err {
                    ToolError::InvalidInput(_) => AuditResult::Blocked,
                    _ => AuditResult::Failed,
                };
                self.record_audit(
                    tool.id,
                    tool.risk,
                    AuditDecision::Rejected,
                    result,
                    &err.to_string(),
                );
                return Err(err);
            }
        };

        let observed_requester = ObservedRequester {
            inner: requester,
            tool_id: tool.id,
            observer,
        };
        let mut cancel_watch = cancel_rx.clone();
        let permit = tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel_watch) => {
                self.record_audit(
                    tool.id,
                    tool.risk,
                    AuditDecision::Cancelled,
                    AuditResult::Cancelled,
                    &ToolError::Cancelled.to_string(),
                );
                return Err(ToolError::Cancelled);
            }
            res = self.permissions.authorize(tool.id, tool.risk, &dry_run_summary, &observed_requester) => {
                match res {
                    Ok(permit) => permit,
                    Err(err) => {
                        let decision = match &err {
                            ToolError::Timeout => AuditDecision::TimedOut,
                            ToolError::PermissionDenied(_) if !self.permissions.status().healthy => {
                                AuditDecision::BlockedFailClosed
                            }
                            _ => AuditDecision::Denied,
                        };
                        self.record_audit(
                            tool.id,
                            tool.risk,
                            decision,
                            AuditResult::Blocked,
                            &err.to_string(),
                        );
                        return Err(err);
                    }
                }
            }
        };

        if let Some(obs) = observer {
            obs.on_executing(tool.id);
        }

        let output = match self
            .execute_with_permit(&tool, arguments.clone(), tool_ctx, &permit, cancel_rx)
            .await
        {
            Ok(output) => output,
            Err(err) => {
                let (decision, result, summary) = match &err {
                    ToolError::Paused => (
                        AuditDecision::BlockedPaused,
                        AuditResult::Blocked,
                        err.to_string(),
                    ),
                    ToolError::Cancelled => (
                        AuditDecision::Cancelled,
                        AuditResult::Cancelled,
                        err.to_string(),
                    ),
                    ToolError::PermissionDenied(_) => (
                        AuditDecision::from(permit.decision()),
                        AuditResult::Blocked,
                        err.to_string(),
                    ),
                    _ => (
                        AuditDecision::from(permit.decision()),
                        AuditResult::Failed,
                        safe_tool_audit_summary(&tool, &arguments, &Err(err.clone())),
                    ),
                };
                self.record_audit(tool.id, tool.risk, decision, result, &summary);
                return Err(err);
            }
        };

        if let Err(err) = (tool.verify)(&arguments, &output) {
            let summary = safe_tool_audit_summary(&tool, &arguments, &Err(err.clone()));
            self.record_audit(
                tool.id,
                tool.risk,
                AuditDecision::from(permit.decision()),
                AuditResult::Failed,
                &summary,
            );
            return Err(err);
        }

        let ok_result = Ok(output.clone());
        let raw_summary = (tool.audit_summary)(&arguments, &ok_result);
        let audit_summary = self
            .record_audit(
                tool.id,
                tool.risk,
                AuditDecision::from(permit.decision()),
                AuditResult::Success,
                &raw_summary,
            )
            .map(|entry| entry.summary)
            .unwrap_or_else(|| audit::redact_summary(&raw_summary));

        Ok(ToolExecutionOutcome {
            tool_id: tool.id.to_string(),
            risk: tool.risk,
            decision: permit.decision(),
            dry_run_summary,
            output,
            audit_summary,
        })
    }

    /// Executa a ferramenta usando `TauriApprovalRequester` conectado ao `AppHandle` para confirmações.
    #[allow(dead_code)]
    pub async fn invoke_tool_with_app(
        &self,
        app: &tauri::AppHandle,
        tool_id: &str,
        arguments: Value,
        tool_ctx: &ToolContext,
        cancel_rx: watch::Receiver<bool>,
    ) -> Result<ToolExecutionOutcome, ToolError> {
        let requester = approval_requester(app);
        self.invoke_tool(tool_id, arguments, tool_ctx, &requester, cancel_rx)
            .await
    }

    async fn execute_with_permit(
        &self,
        tool: &ToolDefinition,
        arguments: Value,
        tool_ctx: &ToolContext,
        permit: &PermitToken,
        cancel_rx: watch::Receiver<bool>,
    ) -> Result<Value, ToolError> {
        if self.is_paused(tool_ctx) {
            return Err(ToolError::Paused);
        }
        if *cancel_rx.borrow() {
            return Err(ToolError::Cancelled);
        }

        let exec_ctx = ExecutionContext::for_tool(cancel_rx.clone(), permit, tool.id)?;
        let mut cancel_watch = cancel_rx;

        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel_watch) => Err(ToolError::Cancelled),
            timed = tokio::time::timeout(tool.timeout, (tool.execute)(arguments, exec_ctx)) => {
                match timed {
                    Ok(res) => res,
                    Err(_) => Err(ToolError::Timeout),
                }
            }
        }
    }
}

/// Cria um solicitante de aprovação conectado à ilha e às notificações do `AppHandle`.
#[allow(dead_code)]
pub fn approval_requester(app: &tauri::AppHandle) -> TauriApprovalRequester {
    TauriApprovalRequester::new(app.clone())
}

/// Inicializa permissões do Mochi e o servidor MCP local na partida do Tauri.
pub fn setup(app: &tauri::AppHandle) {
    let config_path = permissions::default_config_path();
    let marker_path = permissions::default_init_marker_path();
    if let Err(err) = permissions::bootstrap_default_if_first_run(&config_path, &marker_path) {
        crate::log::line(format!("mochi permissions bootstrap failed: {err}"));
    }

    if let Some(runtime) = app.try_state::<MochiRuntime>() {
        let status = runtime.permissions.status();
        if !status.healthy {
            if let Some(reason) = status.fail_closed_reason {
                crate::log::line(format!("mochi permissions fail-closed: {reason}"));
            }
        }

        let endpoint_path = mcp_server::default_endpoint_path();
        let runtime_clone = runtime.inner().clone();
        match tauri::async_runtime::block_on(mcp_server::McpServerHandle::start(
            runtime_clone,
            Some(app.clone()),
            endpoint_path,
        )) {
            Ok(handle) => runtime.set_mcp_handle(handle),
            Err(err) => crate::log::line(format!("mochi mcp server start failed: {err}")),
        }
    }
}

/// Encerra o servidor MCP local do Mochi e remove o arquivo de descoberta.
pub fn shutdown(app: &tauri::AppHandle) {
    if let Some(runtime) = app.try_state::<MochiRuntime>() {
        runtime.shutdown_mcp();
    }
}

#[tauri::command]
pub fn mochi_permissions_status(runtime: tauri::State<'_, MochiRuntime>) -> PermissionStatus {
    runtime.permissions.status()
}

#[tauri::command]
pub fn mochi_permissions_revoke(
    runtime: tauri::State<'_, MochiRuntime>,
    tool_id: String,
) -> Result<PermissionStatus, String> {
    let _removed = runtime.permissions.revoke_always_allow(&tool_id)?;
    Ok(runtime.permissions.status())
}

#[tauri::command]
pub fn mochi_permissions_reset_default(
    runtime: tauri::State<'_, MochiRuntime>,
) -> Result<PermissionStatus, String> {
    runtime.permissions.reset_default()?;
    Ok(runtime.permissions.status())
}

#[tauri::command]
pub fn mochi_audit_list(
    runtime: tauri::State<'_, MochiRuntime>,
    limit: Option<usize>,
) -> Result<Vec<audit::AuditEntry>, String> {
    Ok(runtime.audit.list(limit))
}

pub(crate) fn emit_run_state(
    app: &AppHandle,
    state: MochiRunState,
    tool: Option<&str>,
    detail: Option<String>,
) {
    let _ = app.emit_to(
        crate::island::WINDOW_LABEL,
        MOCHI_RUN_STATE_EVENT,
        MochiRunStatePayload {
            state,
            tool: tool.map(str::to_string),
            detail,
        },
    );
}

/// Atualiza a pausa rápida unificada (integrações, ferramentas, voz e celular).
pub fn set_paused(app: &AppHandle, paused: bool) {
    crate::integrations::set_paused(paused);
    if paused {
        if let Some(runtime) = app.try_state::<MochiRuntime>() {
            let _ = runtime.cancel_execution(Some(app));
        }
        #[cfg(windows)]
        if let Some(toasts) = app.try_state::<crate::permission_notifications::PermissionToasts>() {
            crate::permission_notifications::clear_all(&toasts);
        }
    }
    let _ = app.emit_to(crate::island::WINDOW_LABEL, MOCHI_PAUSED_EVENT, paused);
}

#[tauri::command]
pub fn mochi_cancel_execution(app: AppHandle, runtime: tauri::State<'_, MochiRuntime>) -> bool {
    runtime.cancel_execution(Some(&app))
}

#[tauri::command]
pub fn mochi_audit_clear(runtime: tauri::State<'_, MochiRuntime>) -> Result<(), String> {
    runtime.audit.clear()
}

fn safe_tool_audit_summary(
    tool: &ToolDefinition,
    arguments: &Value,
    result: &Result<Value, ToolError>,
) -> String {
    let from_tool = (tool.audit_summary)(arguments, result);
    match result {
        Ok(_) => from_tool,
        Err(err) => {
            let trimmed = from_tool.trim();
            if trimmed.is_empty() {
                err.to_string()
            } else {
                format!("{trimmed}: {err}")
            }
        }
    }
}

async fn wait_for_cancel(cancel_rx: &mut watch::Receiver<bool>) {
    loop {
        if *cancel_rx.borrow_and_update() {
            return;
        }
        if cancel_rx.changed().await.is_err() {
            // Se o remetente for descartado sem cancelar, não interpreta como cancelamento.
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;

    struct TempPermissionsDir {
        dir: PathBuf,
    }

    impl TempPermissionsDir {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "lumen-mochi-runtime-{tag}-{}-{nanos}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn config_path(&self) -> PathBuf {
            self.dir.join(permissions::PERMISSIONS_FILE_NAME)
        }

        fn audit_path(&self) -> PathBuf {
            self.dir.join(audit::AUDIT_FILE_NAME)
        }

        fn write_raw(&self, content: &str) {
            std::fs::write(self.config_path(), content.as_bytes()).unwrap();
        }
    }

    impl Drop for TempPermissionsDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn runtime_with_valid_perms(tag: &str, registry: ToolRegistry) -> (TempPermissionsDir, MochiRuntime) {
        let temp = TempPermissionsDir::new(tag);
        temp.write_raw("{\"version\":1,\"alwaysAllow\":[]}\n");
        let perms = Arc::new(PermissionEngine::load_from(temp.config_path()));
        let audit = Arc::new(AuditLog::load_from(temp.audit_path()));
        let runtime = MochiRuntime::new_with_parts(Arc::new(registry), perms, audit);
        (temp, runtime)
    }

    struct FixedRequester {
        reply: UserApprovalReply,
        calls: AtomicUsize,
    }

    impl FixedRequester {
        fn new(reply: UserApprovalReply) -> Self {
            Self {
                reply,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl ApprovalRequester for FixedRequester {
        fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let reply = self.reply;
            Box::pin(async move { reply })
        }
    }

    fn slow_probe_tool() -> ToolDefinition {
        ToolDefinition {
            id: "slow_probe",
            description: "Ferramenta lenta de teste para validar timeout e cancelamento.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "delayMs": { "type": "integer", "minimum": 1, "maximum": 60000 }
                },
                "required": ["delayMs"],
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_millis(40),
            check_availability: |_| Availability::Available,
            dry_run: |_| Ok("Aguardar atraso simulado.".to_string()),
            execute: |args, _ctx| {
                Box::pin(async move {
                    let ms = args
                        .get("delayMs")
                        .and_then(Value::as_u64)
                        .unwrap_or(500);
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    Ok(json!({ "done": true }))
                })
            },
            verify: |_, output| {
                if output.get("done").and_then(Value::as_bool) == Some(true) {
                    Ok(())
                } else {
                    Err(ToolError::VerificationFailed("saída sem done=true".into()))
                }
            },
            audit_summary: |_, _| "sonda lenta".to_string(),
        }
    }

    fn bad_verify_probe_tool() -> ToolDefinition {
        ToolDefinition {
            id: "bad_verify_probe",
            description: "Ferramenta cuja verificação pós-execução sempre falha.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_secs(2),
            check_availability: |_| Availability::Available,
            dry_run: |_| Ok("Executar sonda com falha na verificação.".to_string()),
            execute: |_, _| Box::pin(async move { Ok(json!({ "status": "unexpected" })) }),
            verify: |_, _| {
                Err(ToolError::VerificationFailed(
                    "estado esperado não confirmado após execução".to_string(),
                ))
            },
            audit_summary: |_, _| "sonda verificação".to_string(),
        }
    }

    fn unavailable_probe_tool() -> ToolDefinition {
        ToolDefinition {
            id: "unavailable_probe",
            description: "Ferramenta com adaptador ausente.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_secs(2),
            check_availability: |_| Availability::Unavailable {
                reason: "adaptador ausente".to_string(),
            },
            dry_run: |_| Ok("Não deve rodar.".to_string()),
            execute: |_, _| Box::pin(async move { Ok(json!({})) }),
            verify: |_, _| Ok(()),
            audit_summary: |_, _| "indisponível".to_string(),
        }
    }

    fn execution_fail_probe_tool() -> ToolDefinition {
        ToolDefinition {
            id: "failing_probe",
            description: "Ferramenta que falha durante execute.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_secs(2),
            check_availability: |_| Availability::Available,
            dry_run: |_| Ok("Executar falha simulada.".to_string()),
            execute: |_, _| {
                Box::pin(async move {
                    Err(ToolError::ExecutionFailed("erro simulado".to_string()))
                })
            },
            verify: |_, _| Ok(()),
            audit_summary: |_, _| "falha simulada".to_string(),
        }
    }

    #[tokio::test]
    async fn ping_executes_with_empty_object_and_verifies_output() {
        let (_temp, runtime) = runtime_with_valid_perms("ping", ToolRegistry::with_default_tools());
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let outcome = runtime
            .invoke_tool("ping", json!({}), &ctx, &DefaultDenyRequester, rx)
            .await
            .expect("ping com objeto vazio e config válida deve suceder");

        assert_eq!(outcome.tool_id, "ping");
        assert_eq!(outcome.risk, RiskLevel::Low);
        assert_eq!(outcome.decision, PermissionDecision::AutoLow);
        assert_eq!(outcome.dry_run_summary, "Executar ping local inofensivo do Lumen.");
        assert_eq!(outcome.audit_summary, "ping local");
        assert_eq!(
            outcome.output,
            json!({
                "status": "ok",
                "pong": "lumen-mochi-m1"
            })
        );
    }

    #[tokio::test]
    async fn runtime_rejects_unknown_disabled_and_unavailable_tools() {
        let mut registry = ToolRegistry::with_default_tools();
        let mut disabled_ping = ping_tool();
        disabled_ping.id = "hidden_tool";
        registry.register(disabled_ping).unwrap();
        registry.register_and_enable(unavailable_probe_tool()).unwrap();

        let (_temp, runtime) = runtime_with_valid_perms("unknown", registry);
        let ctx = ToolContext::new();
        assert_eq!(
            runtime
                .list_available(&ctx)
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec!["ping"]
        );

        let (_tx1, rx1) = watch::channel(false);
        assert_eq!(
            runtime
                .invoke_tool("powershell", json!({ "cmd": "whoami" }), &ctx, &DefaultDenyRequester, rx1)
                .await
                .unwrap_err(),
            ToolError::UnknownTool("powershell".to_string())
        );

        let (_tx2, rx2) = watch::channel(false);
        assert_eq!(
            runtime
                .invoke_tool("hidden_tool", json!({}), &ctx, &DefaultDenyRequester, rx2)
                .await
                .unwrap_err(),
            ToolError::DisabledTool("hidden_tool".to_string())
        );

        let (_tx3, rx3) = watch::channel(false);
        assert_eq!(
            runtime
                .invoke_tool("unavailable_probe", json!({}), &ctx, &DefaultDenyRequester, rx3)
                .await
                .unwrap_err(),
            ToolError::PrerequisiteMissing("adaptador ausente".to_string())
        );
    }

    #[tokio::test]
    async fn runtime_rejects_arguments_outside_schema() {
        let (_temp, runtime) = runtime_with_valid_perms("schema", ToolRegistry::with_default_tools());
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .invoke_tool("ping", json!({ "cmd": "whoami" }), &ctx, &DefaultDenyRequester, rx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn runtime_aborts_slow_tool_on_timeout() {
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(slow_probe_tool()).unwrap();
        let (_temp, runtime) = runtime_with_valid_perms("timeout", registry);
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .invoke_tool(
                "slow_probe",
                json!({ "delayMs": 500 }),
                &ctx,
                &DefaultDenyRequester,
                rx,
            )
            .await
            .unwrap_err();
        assert_eq!(err, ToolError::Timeout);
    }

    #[tokio::test]
    async fn runtime_aborts_running_tool_on_cancellation() {
        let mut registry = ToolRegistry::with_default_tools();
        let mut slow = slow_probe_tool();
        slow.timeout = Duration::from_secs(5);
        registry.register_and_enable(slow).unwrap();

        let (_temp, runtime) = runtime_with_valid_perms("cancel", registry);
        let ctx = ToolContext::new();
        let (tx, rx) = watch::channel(false);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(true);
        });

        let err = runtime
            .invoke_tool(
                "slow_probe",
                json!({ "delayMs": 2000 }),
                &ctx,
                &DefaultDenyRequester,
                rx,
            )
            .await
            .unwrap_err();
        assert_eq!(err, ToolError::Cancelled);
    }

    #[tokio::test]
    async fn runtime_returns_verification_failed_when_verify_fails() {
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(bad_verify_probe_tool()).unwrap();
        let (_temp, runtime) = runtime_with_valid_perms("verify", registry);
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .invoke_tool("bad_verify_probe", json!({}), &ctx, &DefaultDenyRequester, rx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::VerificationFailed(_)));
    }

    #[tokio::test]
    async fn runtime_propagates_execution_failed_and_blocks_when_paused() {
        static EXECUTED: AtomicBool = AtomicBool::new(false);

        fn tracked_execute(_args: Value, ctx: ExecutionContext) -> ToolFuture {
            let _ = ctx.is_cancelled();
            let _ = ctx.permit_tool_id();
            let _ = ctx.permit_decision();
            EXECUTED.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({ "ok": true })) })
        }

        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(execution_fail_probe_tool()).unwrap();
        registry
            .register_and_enable(ToolDefinition {
                id: "tracked_probe",
                description: "Sonda para verificar bloqueio por pausa.",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::Low,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Sonda rastreada.".to_string()),
                execute: tracked_execute,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "sonda rastreada".to_string(),
            })
            .unwrap();

        let (_temp, runtime) = runtime_with_valid_perms("paused", registry);

        let paused_ctx = ToolContext::new().with_paused(true);
        let (_tx1, rx1) = watch::channel(false);
        assert_eq!(
            runtime
                .invoke_tool("tracked_probe", json!({}), &paused_ctx, &DefaultDenyRequester, rx1)
                .await
                .unwrap_err(),
            ToolError::Paused
        );
        assert!(!EXECUTED.load(Ordering::SeqCst));

        let active_ctx = ToolContext::new();
        let (_tx2, rx2) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool("failing_probe", json!({}), &active_ctx, &DefaultDenyRequester, rx2)
                .await
                .unwrap_err(),
            ToolError::ExecutionFailed(_)
        ));
    }

    #[tokio::test]
    async fn medium_tool_enforces_approval_always_allow_and_revoke() {
        static MED_COUNT: AtomicUsize = AtomicUsize::new(0);
        MED_COUNT.store(0, Ordering::SeqCst);

        fn med_exec(_args: Value, ctx: ExecutionContext) -> ToolFuture {
            assert_eq!(ctx.permit_tool_id(), "med_probe");
            MED_COUNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({ "ok": true })) })
        }

        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "med_probe",
                description: "Sonda de risco médio.",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::Medium,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Alterar estado local de teste.".to_string()),
                execute: med_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "sonda média".to_string(),
            })
            .unwrap();

        let (_temp, runtime) = runtime_with_valid_perms("med_flow", registry);
        let ctx = ToolContext::new();

        // Sem resposta não executa.
        let (_tx0, rx0) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool("med_probe", json!({}), &ctx, &DefaultDenyRequester, rx0)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert_eq!(MED_COUNT.load(Ordering::SeqCst), 0);

        // AllowOnce executa 1x e volta a pedir na 2ª.
        let once = FixedRequester::new(UserApprovalReply::AllowOnce);
        let (_tx1, rx1) = watch::channel(false);
        let out1 = runtime
            .invoke_tool("med_probe", json!({}), &ctx, &once, rx1)
            .await
            .unwrap();
        assert_eq!(out1.decision, PermissionDecision::AllowedOnce);
        assert_eq!(MED_COUNT.load(Ordering::SeqCst), 1);

        let (_tx2, rx2) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool("med_probe", json!({}), &ctx, &DefaultDenyRequester, rx2)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert_eq!(MED_COUNT.load(Ordering::SeqCst), 1);

        // AllowAlways grava e a chamada seguinte roda sem requester.
        let always = FixedRequester::new(UserApprovalReply::AllowAlways);
        let (_tx3, rx3) = watch::channel(false);
        let out2 = runtime
            .invoke_tool("med_probe", json!({}), &ctx, &always, rx3)
            .await
            .unwrap();
        assert_eq!(out2.decision, PermissionDecision::AllowedAlways);
        assert_eq!(always.calls(), 1);
        assert_eq!(MED_COUNT.load(Ordering::SeqCst), 2);

        let (_tx4, rx4) = watch::channel(false);
        let out3 = runtime
            .invoke_tool("med_probe", json!({}), &ctx, &DefaultDenyRequester, rx4)
            .await
            .unwrap();
        assert_eq!(out3.decision, PermissionDecision::AlreadyAlwaysAllowed);
        assert_eq!(MED_COUNT.load(Ordering::SeqCst), 3);

        // Revogar faz pedir novamente e sem resposta não executa.
        assert!(runtime.permissions().revoke_always_allow("med_probe").unwrap());
        let (_tx5, rx5) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool("med_probe", json!({}), &ctx, &DefaultDenyRequester, rx5)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert_eq!(MED_COUNT.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn high_tool_never_auto_runs_even_with_manual_always_allow_or_allow_always_reply() {
        static HIGH_COUNT: AtomicUsize = AtomicUsize::new(0);
        HIGH_COUNT.store(0, Ordering::SeqCst);

        fn high_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            HIGH_COUNT.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({ "ok": true })) })
        }

        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "high_probe",
                description: "Sonda de risco alto.",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::High,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Ação de risco alto.".to_string()),
                execute: high_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "sonda alta".to_string(),
            })
            .unwrap();

        let temp = TempPermissionsDir::new("high_flow");
        temp.write_raw("{\"version\":1,\"alwaysAllow\":[\"high_probe\"]}\n");
        let perms = Arc::new(PermissionEngine::load_from(temp.config_path()));
        let runtime = MochiRuntime::new(Arc::new(registry), perms);
        let ctx = ToolContext::new();

        let (_tx1, rx1) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool("high_probe", json!({}), &ctx, &DefaultDenyRequester, rx1)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert_eq!(HIGH_COUNT.load(Ordering::SeqCst), 0);

        // Mesmo se requester responder AllowAlways, executa 1x como AllowedOnce e volta a pedir depois.
        let bad_req = FixedRequester::new(UserApprovalReply::AllowAlways);
        let (_tx2, rx2) = watch::channel(false);
        let out = runtime
            .invoke_tool("high_probe", json!({}), &ctx, &bad_req, rx2)
            .await
            .unwrap();
        assert_eq!(out.decision, PermissionDecision::AllowedOnce);
        assert_eq!(HIGH_COUNT.load(Ordering::SeqCst), 1);

        let (_tx3, rx3) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool("high_probe", json!({}), &ctx, &DefaultDenyRequester, rx3)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert_eq!(HIGH_COUNT.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn denied_declined_timeout_and_no_response_never_execute_tool() {
        static EXECUTED: AtomicBool = AtomicBool::new(false);
        EXECUTED.store(false, Ordering::SeqCst);

        fn guarded_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            EXECUTED.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({})) })
        }

        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "guarded_probe",
                description: "Sonda para validar não execução.",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::Medium,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Sonda protegida.".to_string()),
                execute: guarded_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "sonda protegida".to_string(),
            })
            .unwrap();

        let (_temp, runtime) = runtime_with_valid_perms("non_exec", registry);
        let ctx = ToolContext::new();

        for reply in [
            UserApprovalReply::NoResponse,
            UserApprovalReply::Deny,
            UserApprovalReply::Declined,
            UserApprovalReply::Timeout,
        ] {
            let req = FixedRequester::new(reply);
            let (_tx, rx) = watch::channel(false);
            assert!(runtime
                .invoke_tool("guarded_probe", json!({}), &ctx, &req, rx)
                .await
                .is_err());
            assert!(!EXECUTED.load(Ordering::SeqCst));
        }
    }

    #[tokio::test]
    async fn fail_closed_denies_even_ping_and_preserves_invalid_file() {
        let temp = TempPermissionsDir::new("failclosed_runtime");
        let invalid = r#"{"version":1,"alwaysAllow":[],"mode":"AUTO"}"#;
        temp.write_raw(invalid);

        let perms = Arc::new(PermissionEngine::load_from(temp.config_path()));
        let runtime = MochiRuntime::new(Arc::new(ToolRegistry::with_default_tools()), perms);
        let ctx = ToolContext::new();
        let allow_req = FixedRequester::new(UserApprovalReply::AllowAlways);
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .invoke_tool("ping", json!({}), &ctx, &allow_req, rx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
        assert_eq!(allow_req.calls(), 0);
        assert_eq!(std::fs::read_to_string(temp.config_path()).unwrap(), invalid);
    }

    #[tokio::test]
    async fn execute_with_permit_rejects_mismatched_token_tool_id() {
        static OTHER_EXECUTED: AtomicBool = AtomicBool::new(false);
        OTHER_EXECUTED.store(false, Ordering::SeqCst);

        fn other_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            OTHER_EXECUTED.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({})) })
        }

        let other_def = ToolDefinition {
            id: "other_probe",
            description: "Ferramenta distinta para testar token trocado.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_secs(2),
            check_availability: |_| Availability::Available,
            dry_run: |_| Ok("Outra sonda.".to_string()),
            execute: other_exec,
            verify: |_, _| Ok(()),
            audit_summary: |_, _| "outra sonda".to_string(),
        };

        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(other_def.clone()).unwrap();
        let (_temp, runtime) = runtime_with_valid_perms("token_mismatch", registry);

        let ping_permit = runtime
            .permissions()
            .authorize("ping", RiskLevel::Low, "ping", &DefaultDenyRequester)
            .await
            .unwrap();
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .execute_with_permit(&other_def, json!({}), &ctx, &ping_permit, rx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
        assert!(!OTHER_EXECUTED.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancellation_while_awaiting_approval_aborts_before_execution() {
        static EXECUTED: AtomicBool = AtomicBool::new(false);
        EXECUTED.store(false, Ordering::SeqCst);

        struct SlowRequester;
        impl ApprovalRequester for SlowRequester {
            fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    UserApprovalReply::AllowOnce
                })
            }
        }

        fn slow_auth_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            EXECUTED.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({})) })
        }

        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "slow_auth_probe",
                description: "Sonda que aguarda clique.",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                risk: RiskLevel::Medium,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Aguardar clique.".to_string()),
                execute: slow_auth_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "aguardar".to_string(),
            })
            .unwrap();

        let (_temp, runtime) = runtime_with_valid_perms("cancel_wait", registry);
        let ctx = ToolContext::new();
        let (tx, rx) = watch::channel(false);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(true);
        });

        let err = runtime
            .invoke_tool("slow_auth_probe", json!({}), &ctx, &SlowRequester, rx)
            .await
            .unwrap_err();
        assert_eq!(err, ToolError::Cancelled);
        assert!(!EXECUTED.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn runtime_records_redacted_audit_for_all_outcomes_without_leaking_secrets_or_paths() {
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(unavailable_probe_tool()).unwrap();
        registry.register_and_enable(execution_fail_probe_tool()).unwrap();
        registry.register_and_enable(bad_verify_probe_tool()).unwrap();

        // Ferramenta de teste que tenta vazar o valor de um argumento com segredo e caminho pessoal
        // tanto no resumo de auditoria quanto na mensagem de erro.
        registry
            .register_and_enable(ToolDefinition {
                id: "leaky_probe",
                description: "Sonda para testar redação no pipeline completo.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "secret": { "type": "string" },
                        "path": { "type": "string" },
                        "fail": { "type": "boolean" }
                    },
                    "required": ["secret", "path"],
                    "additionalProperties": false
                }),
                risk: RiskLevel::Medium,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Sonda com argumentos sensíveis.".to_string()),
                execute: |args, _| {
                    Box::pin(async move {
                        if args.get("fail").and_then(Value::as_bool) == Some(true) {
                            let s = args.get("secret").and_then(Value::as_str).unwrap_or("");
                            let p = args.get("path").and_then(Value::as_str).unwrap_or("");
                            return Err(ToolError::ExecutionFailed(format!(
                                "falha usando {s} em {p}"
                            )));
                        }
                        Ok(json!({ "ok": true }))
                    })
                },
                verify: |_, _| Ok(()),
                audit_summary: |args, _| {
                    let s = args.get("secret").and_then(Value::as_str).unwrap_or("");
                    let p = args.get("path").and_then(Value::as_str).unwrap_or("");
                    format!("processou {s} em {p}")
                },
            })
            .unwrap();

        let (temp, runtime) = runtime_with_valid_perms("audit_all", registry);
        let ctx = ToolContext::new();
        let allow_once = FixedRequester::new(UserApprovalReply::AllowOnce);
        let allow_always = FixedRequester::new(UserApprovalReply::AllowAlways);

        // 1) Sucesso Low (ping)
        let (_tx, rx) = watch::channel(false);
        runtime
            .invoke_tool("ping", json!({}), &ctx, &DefaultDenyRequester, rx)
            .await
            .unwrap();

        // 2) Sucesso Medium tentando vazar segredos/caminhos pelo audit_summary
        let secret_sk = "sk-secret-token-998877665544332211";
        let secret_bearer = "Bearer eyJhbGciOi...";
        let secret_ghp = "ghp_1234567890abcdef1234567890";
        let win_path = r"C:\Users\alice\Desktop\segredo.txt";
        let posix_path = "/home/alice/.ssh/id_rsa";

        let (_tx, rx) = watch::channel(false);
        runtime
            .invoke_tool(
                "leaky_probe",
                json!({ "secret": secret_sk, "path": win_path }),
                &ctx,
                &allow_once,
                rx,
            )
            .await
            .unwrap();

        // 3) Falha de execução tentando vazar Bearer, ghp_ e caminho POSIX
        let (_tx, rx) = watch::channel(false);
        assert!(matches!(
            runtime
                .invoke_tool(
                    "leaky_probe",
                    json!({
                        "secret": format!("{secret_bearer} {secret_ghp}"),
                        "path": posix_path,
                        "fail": true
                    }),
                    &ctx,
                    &allow_always,
                    rx,
                )
                .await
                .unwrap_err(),
            ToolError::ExecutionFailed(_)
        ));

        // 4) AlreadyAlwaysAllowed + ExecutionFailed + VerificationFailed
        let (_tx, rx) = watch::channel(false);
        runtime
            .invoke_tool(
                "leaky_probe",
                json!({ "secret": secret_ghp, "path": posix_path }),
                &ctx,
                &DefaultDenyRequester,
                rx,
            )
            .await
            .unwrap();

        let (_tx, rx) = watch::channel(false);
        assert!(runtime
            .invoke_tool("bad_verify_probe", json!({}), &ctx, &DefaultDenyRequester, rx)
            .await
            .is_err());

        // 5) Pausa, desconhecida com segredo no ID, schema inválido, negada, timeout
        let (_tx, rx) = watch::channel(false);
        assert_eq!(
            runtime
                .invoke_tool("ping", json!({}), &ToolContext::new().with_paused(true), &DefaultDenyRequester, rx)
                .await
                .unwrap_err(),
            ToolError::Paused
        );

        let (_tx, rx) = watch::channel(false);
        assert!(runtime
            .invoke_tool(secret_sk, json!({}), &ctx, &DefaultDenyRequester, rx)
            .await
            .is_err());

        let (_tx, rx) = watch::channel(false);
        assert!(runtime
            .invoke_tool("ping", json!({ "extra": secret_ghp }), &ctx, &DefaultDenyRequester, rx)
            .await
            .is_err());

        // Revoga para testar Denied e TimedOut
        assert!(runtime.permissions().revoke_always_allow("leaky_probe").unwrap());
        let (_tx, rx) = watch::channel(false);
        assert!(runtime
            .invoke_tool(
                "leaky_probe",
                json!({ "secret": secret_sk, "path": win_path }),
                &ctx,
                &FixedRequester::new(UserApprovalReply::Deny),
                rx,
            )
            .await
            .is_err());

        let (_tx, rx) = watch::channel(false);
        assert!(runtime
            .invoke_tool(
                "leaky_probe",
                json!({ "secret": secret_sk, "path": win_path }),
                &ctx,
                &FixedRequester::new(UserApprovalReply::Timeout),
                rx,
            )
            .await
            .is_err());

        // 6) FailClosed bloqueia e registra BlockedFailClosed
        temp.write_raw("{\"version\":1,\"alwaysAllow\":[],\"mode\":\"AUTO\"}\n");
        let (_tx, rx) = watch::channel(false);
        assert!(runtime
            .invoke_tool("ping", json!({}), &ctx, &DefaultDenyRequester, rx)
            .await
            .is_err());

        // 7) Cancelamento prévio registra Cancelled
        let (_tx, rx) = watch::channel(true);
        assert_eq!(
            runtime
                .invoke_tool("ping", json!({}), &ctx, &DefaultDenyRequester, rx)
                .await
                .unwrap_err(),
            ToolError::Cancelled
        );

        let entries = runtime.audit().list(None);
        assert_eq!(entries.len(), 12);
        // Mais recentes primeiro:
        assert_eq!(entries[0].decision, AuditDecision::Cancelled);
        assert_eq!(entries[0].result, AuditResult::Cancelled);
        assert_eq!(entries[1].decision, AuditDecision::BlockedFailClosed);
        assert_eq!(entries[1].result, AuditResult::Blocked);
        assert_eq!(entries[2].decision, AuditDecision::TimedOut);
        assert_eq!(entries[3].decision, AuditDecision::Denied);
        assert_eq!(entries[4].decision, AuditDecision::Rejected);
        assert_eq!(entries[5].decision, AuditDecision::Rejected);
        assert_eq!(entries[6].decision, AuditDecision::BlockedPaused);
        assert_eq!(entries[7].result, AuditResult::Failed);
        assert_eq!(entries[8].decision, AuditDecision::AlreadyAlwaysAllowed);
        assert_eq!(entries[8].result, AuditResult::Success);
        assert_eq!(entries[9].decision, AuditDecision::AllowedAlways);
        assert_eq!(entries[9].result, AuditResult::Failed);
        assert_eq!(entries[10].decision, AuditDecision::AllowedOnce);
        assert_eq!(entries[10].result, AuditResult::Success);
        assert_eq!(entries[11].tool_id, "ping");
        assert_eq!(entries[11].decision, AuditDecision::AutoLow);
        assert_eq!(entries[11].result, AuditResult::Success);

        // Garante que nenhum segredo nem caminho pessoal foi gravado em mochi-audit.json.
        let disk = std::fs::read_to_string(temp.audit_path()).unwrap();
        for forbidden in [
            secret_sk,
            "998877665544332211",
            "eyJhbGciOi",
            secret_ghp,
            "alice",
            "segredo.txt",
            "id_rsa",
            r"C:\Users",
            "/home/",
        ] {
            assert!(
                !disk.contains(forbidden),
                "conteúdo sensível '{forbidden}' não deve aparecer em mochi-audit.json: {disk}"
            );
        }

        // clear() limpa memória e disco.
        runtime.audit().clear().unwrap();
        assert!(runtime.audit().list(None).is_empty());
        assert_eq!(std::fs::read_to_string(temp.audit_path()).unwrap().trim(), "[]");
    }

    #[test]
    fn active_run_cancel_execution_wakes_watchers_and_is_idempotent() {
        let (_temp, runtime) =
            runtime_with_valid_perms("active-run", ToolRegistry::with_default_tools());
        assert!(!runtime.cancel_execution(None));

        let (run_id, rx) = runtime.begin_run();
        assert!(!*rx.borrow());
        runtime.set_pending_request(run_id, Some("mochi-1".into()));

        assert!(runtime.cancel_execution(None));
        assert!(*rx.borrow());
        assert!(!runtime.cancel_execution(None));

        let (_run2, mut rx2) = runtime.begin_run();
        let (run3, _rx3) = runtime.begin_run();
        // Novo begin_run cancela eventual run anterior órfão.
        assert!(*rx2.borrow_and_update());
        runtime.finish_run(run3);
        assert!(!runtime.cancel_execution(None));
    }
}
