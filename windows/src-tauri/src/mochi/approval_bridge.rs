//! Ponte de aprovação entre `PermissionEngine` e a ilha / toasts do Windows (`crate::pipe`).

use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::mpsc;

use crate::pipe::{self, Pending, Reply, ACK_TIMEOUT, DECISION_TIMEOUT};

use super::permissions::{
    ApprovalFuture, ApprovalPromptRequest, ApprovalRequester, UserApprovalReply,
};
use super::registry::RiskLevel;

/// Solicitante de aprovação humana conectado ao canal da ilha (`crate::pipe`).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TauriApprovalRequester {
    pub app: AppHandle,
}

#[allow(dead_code)]
impl TauriApprovalRequester {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ApprovalRequester for TauriApprovalRequester {
    fn request<'a>(&'a self, prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
        Box::pin(async move { request_mochi_approval(&self.app, prompt).await })
    }
}

/// Apenas ferramentas `Medium` podem oferecer e persistir `"Sempre permitir"`.
pub(crate) fn effective_allow_always(prompt: &ApprovalPromptRequest) -> bool {
    prompt.allow_always && prompt.risk == RiskLevel::Medium
}

pub(crate) fn risk_as_str(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
    }
}

/// Monta o evento `"hook"` emitido para a ilha (`WINDOW_LABEL`) no formato esperado por `hooks.ts`.
pub(crate) fn build_hook_payload(request_id: &str, prompt: &ApprovalPromptRequest) -> Value {
    let allow_always = effective_allow_always(prompt);
    json!({
        "hook_event_name": "PermissionRequest",
        "request_id": request_id,
        "session_id": "mochi",
        "coucou_kind": "mochi_tool",
        "tool_name": prompt.tool_id,
        "mochi_summary": prompt.summary,
        "mochi_risk": risk_as_str(prompt.risk),
        "mochi_allow_always": allow_always,
        "tool_input": { "command": prompt.summary },
    })
}

/// Converte a resposta do canal `crate::pipe` em `UserApprovalReply` (defesa em profundidade para `allow_always`).
pub fn map_pipe_reply_to_user_reply(reply: Option<Reply>, allow_always: bool) -> UserApprovalReply {
    match reply {
        Some(Reply::Decision(decision)) => match decision.as_str() {
            "always" if allow_always => UserApprovalReply::AllowAlways,
            "always" | "allow" => UserApprovalReply::AllowOnce,
            _ => UserApprovalReply::Deny,
        },
        Some(Reply::Decline) => UserApprovalReply::Declined,
        Some(Reply::Ack) | None => UserApprovalReply::Timeout,
    }
}

/// Aguarda primeiro o `Ack` da ilha e depois a decisão humana dentro dos respectivos prazos.
pub(crate) async fn await_pipe_reply(
    rx: &mut mpsc::Receiver<Reply>,
    ack_timeout: Duration,
    decision_timeout: Duration,
) -> Option<Reply> {
    match tokio::time::timeout(ack_timeout, rx.recv()).await {
        Ok(Some(Reply::Ack)) => {}
        Ok(Some(Reply::Decision(decision))) => return Some(Reply::Decision(decision)),
        Ok(Some(Reply::Decline)) => return Some(Reply::Decline),
        Ok(None) | Err(_) => return None,
    }

    let deadline = tokio::time::Instant::now() + decision_timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(Reply::Ack)) => continue,
            Ok(Some(other)) => return Some(other),
            Ok(None) | Err(_) => return None,
        }
    }
}

#[allow(dead_code)]
struct PendingCleanupGuard {
    app: AppHandle,
    request_id: String,
    armed: bool,
}

#[allow(dead_code)]
impl PendingCleanupGuard {
    fn new(app: AppHandle, request_id: String) -> Self {
        Self {
            app,
            request_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingCleanupGuard {
    fn drop(&mut self) {
        if self.armed {
            pipe::expire_mochi_request(&self.app, &self.request_id);
        }
    }
}

/// Solicita aprovação humana pela ilha e aguarda resposta sem bloquear se o pedido for cancelado.
#[allow(dead_code)]
pub async fn request_mochi_approval(
    app: &AppHandle,
    prompt: ApprovalPromptRequest,
) -> UserApprovalReply {
    let allow_always = effective_allow_always(&prompt);
    let request_id = pipe::next_mochi_request_id();
    let payload = build_hook_payload(&request_id, &prompt);
    let Some(mut rx) = pipe::start_mochi_request(app, request_id.clone(), payload, allow_always)
    else {
        return UserApprovalReply::NoResponse;
    };

    let mut guard = PendingCleanupGuard::new(app.clone(), request_id.clone());
    let reply = await_pipe_reply(&mut rx, ACK_TIMEOUT, DECISION_TIMEOUT).await;
    guard.disarm();

    let declined = matches!(reply, Some(Reply::Decline));
    if !matches!(reply, Some(Reply::Decision(_))) {
        pipe::finish_mochi_request(app, &request_id, declined);
    }

    map_pipe_reply_to_user_reply(reply, allow_always)
}

#[cfg(test)]
struct PendingMapGuard<'a> {
    pending: &'a Pending,
    request_id: String,
    armed: bool,
}

#[cfg(test)]
impl<'a> Drop for PendingMapGuard<'a> {
    fn drop(&mut self) {
        if self.armed {
            self.pending.remove_mochi(&self.request_id);
        }
    }
}

#[cfg(test)]
pub(crate) async fn request_via_pending(
    pending: &Pending,
    request_id: String,
    prompt: ApprovalPromptRequest,
    ack_timeout: Duration,
    decision_timeout: Duration,
) -> UserApprovalReply {
    let allow_always = effective_allow_always(&prompt);
    let mut rx = pending.insert_mochi(
        request_id.clone(),
        allow_always,
        ack_timeout,
        decision_timeout,
    );
    let mut guard = PendingMapGuard {
        pending,
        request_id: request_id.clone(),
        armed: true,
    };
    let reply = await_pipe_reply(&mut rx, ack_timeout, decision_timeout).await;
    guard.armed = false;
    pending.remove_mochi(&request_id);
    map_pipe_reply_to_user_reply(reply, allow_always)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tokio::sync::watch;

    use super::*;
    use crate::mochi::permissions::{PermissionDecision, PermissionEngine};
    use crate::mochi::registry::{
        Availability, ExecutionContext, ToolContext, ToolDefinition, ToolError, ToolFuture,
        ToolRegistry,
    };
    use crate::mochi::MochiRuntime;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "lumen-mochi-approval-{tag}-{}-{nanos}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn config_path(&self) -> PathBuf {
            self.path.join("mochi-permissions.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct PendingTestRequester {
        pending: Arc<Pending>,
        next_id: String,
        ack_timeout: Duration,
        decision_timeout: Duration,
    }

    impl ApprovalRequester for PendingTestRequester {
        fn request<'a>(&'a self, prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
            let id = self.next_id.clone();
            Box::pin(async move {
                request_via_pending(
                    &self.pending,
                    id,
                    prompt,
                    self.ack_timeout,
                    self.decision_timeout,
                )
                .await
            })
        }
    }

    fn probe_tool(id: &'static str, risk: RiskLevel) -> ToolDefinition {
        match id {
            "med_bridge" => {
                fn exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
                    MED_BRIDGE_RUNS.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move { Ok(json!({ "ok": true })) })
                }
                ToolDefinition {
                    id,
                    description: "Ferramenta de teste de risco médio.",
                    input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                    risk,
                    timeout: Duration::from_secs(2),
                    check_availability: |_| Availability::Available,
                    dry_run: |_| Ok("Alterar nota local de teste.".to_string()),
                    execute: exec,
                    verify: |_, _| Ok(()),
                    audit_summary: |_, _| "teste médio".to_string(),
                }
            }
            _ => {
                fn exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
                    HIGH_BRIDGE_RUNS.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move { Ok(json!({ "ok": true })) })
                }
                ToolDefinition {
                    id,
                    description: "Ferramenta de teste de risco alto.",
                    input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                    risk,
                    timeout: Duration::from_secs(2),
                    check_availability: |_| Availability::Available,
                    dry_run: |_| Ok("Apagar registro permanente de teste.".to_string()),
                    execute: exec,
                    verify: |_, _| Ok(()),
                    audit_summary: |_, _| "teste alto".to_string(),
                }
            }
        }
    }

    static MED_BRIDGE_RUNS: AtomicUsize = AtomicUsize::new(0);
    static HIGH_BRIDGE_RUNS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn map_pipe_reply_handles_all_variants_and_enforces_allow_always() {
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Decision("allow".into())), false),
            UserApprovalReply::AllowOnce
        );
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Decision("always".into())), true),
            UserApprovalReply::AllowAlways
        );
        // Mesmo se "always" vier com allow_always=false, rebaixa para AllowOnce (nunca Always).
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Decision("always".into())), false),
            UserApprovalReply::AllowOnce
        );
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Decision("deny".into())), true),
            UserApprovalReply::Deny
        );
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Decision("unexpected".into())), true),
            UserApprovalReply::Deny
        );
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Decline), true),
            UserApprovalReply::Declined
        );
        assert_eq!(
            map_pipe_reply_to_user_reply(Some(Reply::Ack), true),
            UserApprovalReply::Timeout
        );
        assert_eq!(
            map_pipe_reply_to_user_reply(None, true),
            UserApprovalReply::Timeout
        );
    }

    #[test]
    fn hook_payload_includes_expected_fields_and_blocks_always_on_high() {
        let med = ApprovalPromptRequest {
            tool_id: "med_tool".into(),
            summary: "Salvar rascunho local.".into(),
            risk: RiskLevel::Medium,
            allow_always: true,
        };
        assert_eq!(
            build_hook_payload("req-1", &med),
            json!({
                "hook_event_name": "PermissionRequest",
                "request_id": "req-1",
                "session_id": "mochi",
                "coucou_kind": "mochi_tool",
                "tool_name": "med_tool",
                "mochi_summary": "Salvar rascunho local.",
                "mochi_risk": "medium",
                "mochi_allow_always": true,
                "tool_input": { "command": "Salvar rascunho local." }
            })
        );

        let high_forged = ApprovalPromptRequest {
            tool_id: "danger_tool".into(),
            summary: "Ação crítica.".into(),
            risk: RiskLevel::High,
            allow_always: true,
        };
        let payload = build_hook_payload("req-2", &high_forged);
        assert_eq!(payload["mochi_risk"], "high");
        assert_eq!(payload["mochi_allow_always"], false);
    }

    #[tokio::test]
    async fn pending_bridge_allows_once_and_persists_always_for_medium_only() {
        MED_BRIDGE_RUNS.store(0, Ordering::SeqCst);
        HIGH_BRIDGE_RUNS.store(0, Ordering::SeqCst);

        let temp = TempDir::new("allow_flow");
        std::fs::write(temp.config_path(), b"{\"version\":1,\"alwaysAllow\":[]}\n").unwrap();

        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(probe_tool("med_bridge", RiskLevel::Medium))
            .unwrap();
        registry
            .register_and_enable(probe_tool("high_bridge", RiskLevel::High))
            .unwrap();

        let perms = Arc::new(PermissionEngine::load_from(temp.config_path()));
        let runtime = MochiRuntime::new(Arc::new(registry), perms);
        let pending = Arc::new(Pending::default());
        let ctx = ToolContext::new();

        // 1) Medium -> AllowOnce
        let req_once = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-med-once".into(),
            ack_timeout: Duration::from_millis(500),
            decision_timeout: Duration::from_secs(2),
        };
        let pending_answer = pending.clone();
        let driver = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            assert!(pending_answer.acknowledge_request("req-med-once"));
            tokio::time::sleep(Duration::from_millis(15)).await;
            assert!(pending_answer.answer_request("req-med-once", "allow").is_some());
        });
        let (_tx1, rx1) = watch::channel(false);
        let out1 = runtime
            .invoke_tool("med_bridge", json!({}), &ctx, &req_once, rx1)
            .await
            .unwrap();
        driver.await.unwrap();
        assert_eq!(out1.decision, PermissionDecision::AllowedOnce);
        assert_eq!(MED_BRIDGE_RUNS.load(Ordering::SeqCst), 1);

        // 2) Medium -> AllowAlways (persiste em mochi-permissions.json)
        let req_always = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-med-always".into(),
            ack_timeout: Duration::from_millis(500),
            decision_timeout: Duration::from_secs(2),
        };
        let pending_answer = pending.clone();
        let driver = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            assert!(pending_answer.acknowledge_request("req-med-always"));
            assert!(pending_answer.acknowledge_request("req-med-always"));
            tokio::time::sleep(Duration::from_millis(15)).await;
            assert!(pending_answer.answer_request("req-med-always", "always").is_some());
        });
        let (_tx2, rx2) = watch::channel(false);
        let out2 = runtime
            .invoke_tool("med_bridge", json!({}), &ctx, &req_always, rx2)
            .await
            .unwrap();
        driver.await.unwrap();
        assert_eq!(out2.decision, PermissionDecision::AllowedAlways);
        assert_eq!(runtime.permissions().status().always_allow, vec!["med_bridge".to_string()]);

        // 3) High -> tentativa de "always" vira apenas "allow" uma vez e nunca persiste.
        let req_high = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-high".into(),
            ack_timeout: Duration::from_millis(500),
            decision_timeout: Duration::from_secs(2),
        };
        let pending_answer = pending.clone();
        let driver = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            // Clique antes mesmo do Ack deve funcionar e "always" em High deve virar AllowOnce.
            assert!(pending_answer.answer_request("req-high", "always").is_some());
        });
        let (_tx3, rx3) = watch::channel(false);
        let out3 = runtime
            .invoke_tool("high_bridge", json!({}), &ctx, &req_high, rx3)
            .await
            .unwrap();
        driver.await.unwrap();
        assert_eq!(out3.decision, PermissionDecision::AllowedOnce);
        assert!(!runtime
            .permissions()
            .status()
            .always_allow
            .contains(&"high_bridge".to_string()));
        assert_eq!(HIGH_BRIDGE_RUNS.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pending_bridge_denies_declines_times_out_and_cancels_without_executing() {
        static EXECUTED: AtomicBool = AtomicBool::new(false);
        EXECUTED.store(false, Ordering::SeqCst);

        fn guarded_exec(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
            EXECUTED.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(json!({})) })
        }

        let temp = TempDir::new("reject_flow");
        std::fs::write(temp.config_path(), b"{\"version\":1,\"alwaysAllow\":[]}\n").unwrap();

        let mut registry = ToolRegistry::with_default_tools();
        registry
            .register_and_enable(ToolDefinition {
                id: "guarded_bridge",
                description: "Sonda para testar recusa, declínio, timeout e cancelamento.",
                input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                risk: RiskLevel::High,
                timeout: Duration::from_secs(2),
                check_availability: |_| Availability::Available,
                dry_run: |_| Ok("Executar ação protegida.".to_string()),
                execute: guarded_exec,
                verify: |_, _| Ok(()),
                audit_summary: |_, _| "sonda protegida".to_string(),
            })
            .unwrap();

        let perms = Arc::new(PermissionEngine::load_from(temp.config_path()));
        let runtime = MochiRuntime::new(Arc::new(registry), perms);
        let pending = Arc::new(Pending::default());
        let ctx = ToolContext::new();

        // Deny
        let req_deny = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-deny".into(),
            ack_timeout: Duration::from_millis(400),
            decision_timeout: Duration::from_secs(1),
        };
        let p = pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            let _ = p.acknowledge_request("req-deny");
            tokio::time::sleep(Duration::from_millis(15)).await;
            let _ = p.answer_request("req-deny", "deny");
        });
        let (_tx1, rx1) = watch::channel(false);
        assert!(matches!(
            runtime.invoke_tool("guarded_bridge", json!({}), &ctx, &req_deny, rx1).await,
            Err(ToolError::PermissionDenied(_))
        ));

        // Decline
        let req_decline = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-decline".into(),
            ack_timeout: Duration::from_millis(400),
            decision_timeout: Duration::from_secs(1),
        };
        let p = pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            let _ = p.decline_request("req-decline");
        });
        let (_tx2, rx2) = watch::channel(false);
        assert!(matches!(
            runtime.invoke_tool("guarded_bridge", json!({}), &ctx, &req_decline, rx2).await,
            Err(ToolError::PermissionDenied(_))
        ));

        // Timeout sem Ack
        let req_ack_to = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-ack-to".into(),
            ack_timeout: Duration::from_millis(25),
            decision_timeout: Duration::from_secs(1),
        };
        let (_tx3, rx3) = watch::channel(false);
        assert_eq!(
            runtime.invoke_tool("guarded_bridge", json!({}), &ctx, &req_ack_to, rx3).await.unwrap_err(),
            ToolError::Timeout
        );
        assert!(pending.remaining_duration("req-ack-to").is_none());

        // Timeout após Ack sem decisão
        let req_dec_to = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-dec-to".into(),
            ack_timeout: Duration::from_millis(200),
            decision_timeout: Duration::from_millis(30),
        };
        let p = pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let _ = p.acknowledge_request("req-dec-to");
        });
        let (_tx4, rx4) = watch::channel(false);
        assert_eq!(
            runtime.invoke_tool("guarded_bridge", json!({}), &ctx, &req_dec_to, rx4).await.unwrap_err(),
            ToolError::Timeout
        );

        // Cancelamento externo pelo canal de Pending (remove_mochi -> canal fechado -> Timeout)
        let req_removed = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-removed".into(),
            ack_timeout: Duration::from_millis(400),
            decision_timeout: Duration::from_secs(2),
        };
        let p = pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            let _ = p.acknowledge_request("req-removed");
            tokio::time::sleep(Duration::from_millis(15)).await;
            assert!(p.remove_mochi("req-removed"));
        });
        let (_tx5, rx5) = watch::channel(false);
        assert_eq!(
            runtime.invoke_tool("guarded_bridge", json!({}), &ctx, &req_removed, rx5).await.unwrap_err(),
            ToolError::Timeout
        );

        // Cancelamento cooperativo pelo watch::Sender limpa o Pending ao dropar a future.
        let req_cancel = PendingTestRequester {
            pending: pending.clone(),
            next_id: "req-cancel".into(),
            ack_timeout: Duration::from_secs(2),
            decision_timeout: Duration::from_secs(5),
        };
        let (tx6, rx6) = watch::channel(false);
        let p = pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(p.remaining_duration("req-cancel").is_some());
            let _ = tx6.send(true);
        });
        assert_eq!(
            runtime.invoke_tool("guarded_bridge", json!({}), &ctx, &req_cancel, rx6).await.unwrap_err(),
            ToolError::Cancelled
        );
        assert!(pending.remaining_duration("req-cancel").is_none());
        assert!(!EXECUTED.load(Ordering::SeqCst));
    }
}
