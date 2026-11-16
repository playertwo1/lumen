//! Núcleo de execução do assistente Mochi no Lumen.
//!
//! Orquestra registro, validação estrita de entrada, simulação (`dry_run`),
//! timeout + cancelamento cooperativo (`tokio::select!`) e verificação obrigatória
//! pós-execução (`verify`). Na M1.2 este fluxo receberá a trava de `PermissionEngine`
//! e `PermitToken`.

#![cfg_attr(not(test), allow(dead_code))]

pub mod registry;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::watch;

pub use registry::{
    is_valid_tool_id, ping_tool, validate_schema, Availability, ExecutionContext, RiskLevel,
    ToolContext, ToolDefinition, ToolDescriptor, ToolError, ToolFuture, ToolRegistry,
};

/// Resultado completo de uma execução de ferramenta bem-sucedida e verificada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionOutcome {
    pub tool_id: String,
    pub risk: RiskLevel,
    pub dry_run_summary: String,
    pub output: Value,
    pub audit_summary: String,
}

/// Runtime central do Mochi na camada Rust.
#[derive(Debug, Clone)]
pub struct MochiRuntime {
    pub registry: Arc<ToolRegistry>,
}

impl Default for MochiRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl MochiRuntime {
    /// Cria o runtime com o registro padrão (`ping` habilitada).
    pub fn new() -> Self {
        Self::new_with_registry(Arc::new(ToolRegistry::with_default_tools()))
    }

    /// Cria o runtime usando um `ToolRegistry` explícito.
    pub fn new_with_registry(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    /// Retorna referência ao registro de ferramentas.
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// Informa se a execução está bloqueada pela pausa rápida do contexto ou do aplicativo.
    pub fn is_paused(&self, ctx: &ToolContext) -> bool {
        ctx.paused || crate::integrations::PAUSED.load(Ordering::Relaxed)
    }

    /// Lista apenas ferramentas habilitadas e disponíveis para o contexto atual.
    pub fn list_available(&self, ctx: &ToolContext) -> Vec<ToolDescriptor> {
        self.registry.list_available(ctx)
    }

    /// Executa uma ferramenta registrada seguindo todo o pipeline obrigatório:
    /// pausa -> registro/disponibilidade -> schema estrito -> dry_run ->
    /// execute (timeout + cancelamento) -> verify -> resumo de auditoria.
    ///
    /// Nota M1.2: será travado por `&PermitToken` emitido por `PermissionEngine`.
    pub async fn execute_tool_core(
        &self,
        tool_id: &str,
        arguments: Value,
        tool_ctx: &ToolContext,
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

        let exec_ctx = ExecutionContext::new(cancel_rx.clone());
        let mut cancel_watch = cancel_rx;

        let output = tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel_watch) => {
                return Err(ToolError::Cancelled);
            }
            timed = tokio::time::timeout(tool.timeout, (tool.execute)(arguments.clone(), exec_ctx)) => {
                match timed {
                    Ok(res) => res?,
                    Err(_) => return Err(ToolError::Timeout),
                }
            }
        };

        (tool.verify)(&arguments, &output)?;
        let ok_result = Ok(output.clone());
        let audit_summary = (tool.audit_summary)(&arguments, &ok_result);

        Ok(ToolExecutionOutcome {
            tool_id: tool.id.to_string(),
            risk: tool.risk,
            dry_run_summary,
            output,
            audit_summary,
        })
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use serde_json::json;

    use super::*;

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
        let runtime = MochiRuntime::new();
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let outcome = runtime
            .execute_tool_core("ping", json!({}), &ctx, rx)
            .await
            .expect("ping com objeto vazio deve suceder");

        assert_eq!(outcome.tool_id, "ping");
        assert_eq!(outcome.risk, RiskLevel::Low);
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

        let runtime = MochiRuntime::new_with_registry(Arc::new(registry));
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
                .execute_tool_core("powershell", json!({ "cmd": "whoami" }), &ctx, rx1)
                .await
                .unwrap_err(),
            ToolError::UnknownTool("powershell".to_string())
        );

        let (_tx2, rx2) = watch::channel(false);
        assert_eq!(
            runtime
                .execute_tool_core("hidden_tool", json!({}), &ctx, rx2)
                .await
                .unwrap_err(),
            ToolError::DisabledTool("hidden_tool".to_string())
        );

        let (_tx3, rx3) = watch::channel(false);
        assert_eq!(
            runtime
                .execute_tool_core("unavailable_probe", json!({}), &ctx, rx3)
                .await
                .unwrap_err(),
            ToolError::PrerequisiteMissing("adaptador ausente".to_string())
        );
    }

    #[tokio::test]
    async fn runtime_rejects_arguments_outside_schema() {
        let runtime = MochiRuntime::new();
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .execute_tool_core("ping", json!({ "cmd": "whoami" }), &ctx, rx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn runtime_aborts_slow_tool_on_timeout() {
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(slow_probe_tool()).unwrap();
        let runtime = MochiRuntime::new_with_registry(Arc::new(registry));
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .execute_tool_core("slow_probe", json!({ "delayMs": 500 }), &ctx, rx)
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

        let runtime = MochiRuntime::new_with_registry(Arc::new(registry));
        let ctx = ToolContext::new();
        let (tx, rx) = watch::channel(false);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(true);
        });

        let err = runtime
            .execute_tool_core("slow_probe", json!({ "delayMs": 2000 }), &ctx, rx)
            .await
            .unwrap_err();
        assert_eq!(err, ToolError::Cancelled);
    }

    #[tokio::test]
    async fn runtime_returns_verification_failed_when_verify_fails() {
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(bad_verify_probe_tool()).unwrap();
        let runtime = MochiRuntime::new_with_registry(Arc::new(registry));
        let ctx = ToolContext::new();
        let (_tx, rx) = watch::channel(false);

        let err = runtime
            .execute_tool_core("bad_verify_probe", json!({}), &ctx, rx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::VerificationFailed(_)));
    }

    #[tokio::test]
    async fn runtime_propagates_execution_failed_and_blocks_when_paused() {
        static EXECUTED: AtomicBool = AtomicBool::new(false);

        fn tracked_execute(_args: Value, ctx: ExecutionContext) -> ToolFuture {
            let _ = ctx.is_cancelled();
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

        let runtime = MochiRuntime::new_with_registry(Arc::new(registry));

        let paused_ctx = ToolContext::new().with_paused(true);
        let (_tx1, rx1) = watch::channel(false);
        assert_eq!(
            runtime
                .execute_tool_core("tracked_probe", json!({}), &paused_ctx, rx1)
                .await
                .unwrap_err(),
            ToolError::Paused
        );
        assert!(!EXECUTED.load(Ordering::SeqCst));

        let active_ctx = ToolContext::new();
        let (_tx2, rx2) = watch::channel(false);
        assert!(matches!(
            runtime
                .execute_tool_core("failing_probe", json!({}), &active_ctx, rx2)
                .await
                .unwrap_err(),
            ToolError::ExecutionFailed(_)
        ));
    }
}
