//! Núcleo de execução do assistente Mochi no Lumen.
//!
//! Orquestra pausa rápida, registro deny-by-default, validação estrita de entrada,
//! simulação (`dry_run`), autorização fail-closed (`PermissionEngine` + `PermitToken`),
//! execução sob timeout + cancelamento cooperativo (`tokio::select!`), verificação
//! obrigatória pós-execução (`verify`) e resumo para auditoria.

#![cfg_attr(not(test), allow(dead_code))]

pub mod permissions;
pub mod registry;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::Value;
use tauri::Manager;
use tokio::sync::watch;

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
}

impl Default for MochiRuntime {
    fn default() -> Self {
        Self::new_default()
    }
}

impl MochiRuntime {
    /// Cria o runtime padrão (`ping` habilitada + `PermissionEngine` em `%APPDATA%\Coucou\mochi-permissions.json`).
    pub fn new_default() -> Self {
        Self::new(
            Arc::new(ToolRegistry::with_default_tools()),
            Arc::new(PermissionEngine::new_default()),
        )
    }

    /// Cria o runtime usando `ToolRegistry` e `PermissionEngine` explícitos.
    pub fn new(registry: Arc<ToolRegistry>, permissions: Arc<PermissionEngine>) -> Self {
        Self {
            registry,
            permissions,
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

    /// Informa se a execução está bloqueada pela pausa rápida do contexto ou do aplicativo.
    pub fn is_paused(&self, ctx: &ToolContext) -> bool {
        ctx.paused || crate::integrations::PAUSED.load(Ordering::Relaxed)
    }

    /// Lista apenas ferramentas habilitadas e disponíveis para o contexto atual.
    pub fn list_available(&self, ctx: &ToolContext) -> Vec<ToolDescriptor> {
        self.registry.list_available(ctx)
    }

    /// Pipeline único e obrigatório de execução de ferramentas:
    /// 1. pausa -> 2. registro/disponibilidade -> 3. schema estrito -> 4. dry_run ->
    /// 5. autorização (`PermissionEngine::authorize` -> `PermitToken`) ->
    /// 6. execução autenticada (`execute_with_permit`) -> 7. `verify` -> 8. `audit_summary`.
    pub async fn invoke_tool(
        &self,
        tool_id: &str,
        arguments: Value,
        tool_ctx: &ToolContext,
        requester: &dyn ApprovalRequester,
        cancel_rx: watch::Receiver<bool>,
    ) -> Result<ToolExecutionOutcome, ToolError> {
        if self.is_paused(tool_ctx) {
            return Err(ToolError::Paused);
        }
        if *cancel_rx.borrow() {
            return Err(ToolError::Cancelled);
        }

        let tool = self.registry.get_if_available(tool_id, tool_ctx)?;
        validate_schema(&tool.input_schema, &arguments)?;
        let dry_run_summary = (tool.dry_run)(&arguments)?;

        let mut cancel_watch = cancel_rx.clone();
        let permit = tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel_watch) => {
                return Err(ToolError::Cancelled);
            }
            res = self.permissions.authorize(tool.id, tool.risk, &dry_run_summary, requester) => {
                res?
            }
        };

        let output = self
            .execute_with_permit(&tool, arguments.clone(), tool_ctx, &permit, cancel_rx)
            .await?;

        (tool.verify)(&arguments, &output)?;
        let ok_result = Ok(output.clone());
        let audit_summary = (tool.audit_summary)(&arguments, &ok_result);

        Ok(ToolExecutionOutcome {
            tool_id: tool.id.to_string(),
            risk: tool.risk,
            decision: permit.decision(),
            dry_run_summary,
            output,
            audit_summary,
        })
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

/// Inicializa permissões do Mochi na partida do Tauri (cria config v1 apenas na 1ª execução).
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
        let runtime = MochiRuntime::new(Arc::new(registry), perms);
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
}
