//! Ferramentas de energia (M2.6): `bloquear_tela` (médio) e `suspender`, `reiniciar`, `desligar` (altos).
//!
//! O id escolhe a ação e não há argumento: nenhum texto do pedido chega ao sistema. A API só diz se
//! aceitou o pedido, então o relato é "pedido aceito", nunca "feito".

use std::time::Duration;

use serde_json::{json, Value};

use crate::mochi::registry::{
    Availability, ExecutionContext, RiskLevel, ToolContext, ToolDefinition, ToolError, ToolFuture,
};

const TIMEOUT: Duration = Duration::from_secs(5);
/// `ERROR_NOT_SUPPORTED`: código devolvido fora do Windows e nos testes, onde nada é chamado.
const SEM_BACKEND: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acao {
    Bloquear,
    Suspender,
    Reiniciar,
    Desligar,
}

impl Acao {
    fn id(self) -> &'static str {
        match self {
            Self::Bloquear => "bloquear_tela",
            Self::Suspender => "suspender",
            Self::Reiniciar => "reiniciar",
            Self::Desligar => "desligar",
        }
    }
}

/// Chamada ao sistema. Quando a API recusa, devolve o código Win32.
pub(crate) trait EnergiaBackend {
    fn pedir(&self, acao: Acao) -> Result<(), u32>;
}

pub fn bloquear_tela() -> ToolDefinition {
    ToolDefinition {
        id: Acao::Bloquear.id(),
        description: "Bloqueia a tela.",
        input_schema: schema(),
        risk: RiskLevel::Medium,
        timeout: TIMEOUT,
        check_availability: |ctx| disponivel(Acao::Bloquear, ctx),
        dry_run: |_| Ok(texto(Acao::Bloquear).to_string()),
        execute: |_, ctx| executar_pedido(Acao::Bloquear, ctx),
        verify,
        audit_summary: |_, r| resumo(Acao::Bloquear, r),
    }
}

pub fn suspender() -> ToolDefinition {
    ToolDefinition {
        id: Acao::Suspender.id(),
        description: "Suspende o computador.",
        input_schema: schema(),
        risk: RiskLevel::High,
        timeout: TIMEOUT,
        check_availability: |ctx| disponivel(Acao::Suspender, ctx),
        dry_run: |_| Ok(texto(Acao::Suspender).to_string()),
        execute: |_, ctx| executar_pedido(Acao::Suspender, ctx),
        verify,
        audit_summary: |_, r| resumo(Acao::Suspender, r),
    }
}

pub fn reiniciar() -> ToolDefinition {
    ToolDefinition {
        id: Acao::Reiniciar.id(),
        description: "Reinicia o computador.",
        input_schema: schema(),
        risk: RiskLevel::High,
        timeout: TIMEOUT,
        check_availability: |ctx| disponivel(Acao::Reiniciar, ctx),
        dry_run: |_| Ok(texto(Acao::Reiniciar).to_string()),
        execute: |_, ctx| executar_pedido(Acao::Reiniciar, ctx),
        verify,
        audit_summary: |_, r| resumo(Acao::Reiniciar, r),
    }
}

pub fn desligar() -> ToolDefinition {
    ToolDefinition {
        id: Acao::Desligar.id(),
        description: "Desliga o computador.",
        input_schema: schema(),
        risk: RiskLevel::High,
        timeout: TIMEOUT,
        check_availability: |ctx| disponivel(Acao::Desligar, ctx),
        dry_run: |_| Ok(texto(Acao::Desligar).to_string()),
        execute: |_, ctx| executar_pedido(Acao::Desligar, ctx),
        verify,
        audit_summary: |_, r| resumo(Acao::Desligar, r),
    }
}

fn schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn disponivel(acao: Acao, ctx: &ToolContext) -> Availability {
    let (ok, motivo) = match acao {
        // Bloquear a tela não depende de nada do computador, só do Windows.
        Acao::Bloquear => (cfg!(windows), "exclusivo do Windows"),
        Acao::Suspender => (ctx.has_capability("sleep_supported"), "suspensão desligada neste computador"),
        Acao::Reiniciar | Acao::Desligar => (
            ctx.has_capability("shutdown_privilege"),
            "sem privilégio para reiniciar ou desligar o computador",
        ),
    };
    if ok {
        Availability::Available
    } else {
        Availability::Unavailable {
            reason: motivo.to_string(),
        }
    }
}

fn texto(acao: Acao) -> &'static str {
    match acao {
        Acao::Bloquear => "Bloquear a tela agora.",
        Acao::Suspender => "Suspender o computador agora. O Lumen não sabe se há trabalho não salvo, e apps não são avisados.",
        Acao::Reiniciar => "Reiniciar o computador agora. Apps podem pedir para salvar; se algum recusar, a ação é cancelada. O Lumen não sabe se há trabalho não salvo.",
        Acao::Desligar => "Desligar o computador agora. Apps podem pedir para salvar; se algum recusar, a ação é cancelada. O Lumen não sabe se há trabalho não salvo.",
    }
}

/// Envia o pedido ao sistema. O sucesso só diz que a API aceitou.
pub(crate) fn run_with(backend: &dyn EnergiaBackend, acao: Acao) -> Result<Value, ToolError> {
    backend.pedir(acao).map_err(|codigo| {
        ToolError::ExecutionFailed(format!("o sistema recusou o pedido (erro Win32 {codigo})"))
    })?;
    Ok(json!({ "acao": acao.id(), "aceito": true }))
}

fn executar_pedido(acao: Acao, ctx: ExecutionContext) -> ToolFuture {
    Box::pin(async move {
        if ctx.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        // Chamadas ao sistema saem do runtime assíncrono, como nas outras ferramentas da M2.
        tokio::task::spawn_blocking(move || run_os(acao))
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
    })
}

#[cfg(all(windows, not(test)))]
fn run_os(acao: Acao) -> Result<Value, ToolError> {
    run_with(&win::Os, acao)
}

/// Fora do Windows e nos testes: nada é chamado, e o pedido falha sem tocar em nada.
#[cfg(not(all(windows, not(test))))]
fn run_os(acao: Acao) -> Result<Value, ToolError> {
    run_with(&SemBackend, acao)
}

#[cfg(not(all(windows, not(test))))]
struct SemBackend;

#[cfg(not(all(windows, not(test))))]
impl EnergiaBackend for SemBackend {
    fn pedir(&self, _acao: Acao) -> Result<(), u32> {
        Err(SEM_BACKEND)
    }
}

/// O computador suporta suspensão S3? Sondagem para `host::context`; nunca roda nos testes.
#[cfg(all(windows, not(test)))]
pub(crate) fn sleep_supported() -> bool {
    win::suspensao_suportada()
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn sleep_supported() -> bool {
    false
}

/// O token do processo tem `SeShutdownPrivilege`? Só consulta; nunca habilita. Nunca roda nos testes.
#[cfg(all(windows, not(test)))]
pub(crate) fn shutdown_privilege() -> bool {
    win::privilegio_desligamento_presente()
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn shutdown_privilege() -> bool {
    false
}

fn verify(_args: &Value, output: &Value) -> Result<(), ToolError> {
    if output.get("aceito").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(ToolError::VerificationFailed(
            "o sistema não aceitou o pedido".into(),
        ))
    }
}

fn resumo(acao: Acao, resultado: &Result<Value, ToolError>) -> String {
    let desfecho = if resultado.is_ok() { "pedido aceito" } else { "falha" };
    format!("{}: {desfecho}", acao.id())
}

#[cfg(all(windows, not(test)))]
mod win {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenPrivileges,
        LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES,
        TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Power::{GetPwrCapabilities, SetSuspendState, SYSTEM_POWER_CAPABILITIES};
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, LockWorkStation, EWX_REBOOT, EWX_SHUTDOWN, EXIT_WINDOWS_FLAGS,
        SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION, SHUTDOWN_REASON,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use super::{Acao, EnergiaBackend};

    /// Código Win32 de um erro do windows-rs: o HRESULT traz o código em 0x8007xxxx.
    fn codigo(e: windows::core::Error) -> u32 {
        let h = e.code().0 as u32;
        if h >> 16 == 0x8007 {
            h & 0xFFFF
        } else {
            h
        }
    }

    /// API que devolve BOOL: falso quer dizer que `GetLastError` tem o código.
    fn bool_ou_codigo(ok: bool) -> Result<(), u32> {
        if ok {
            Ok(())
        } else {
            Err(unsafe { GetLastError() }.0)
        }
    }

    /// Sem `EWX_FORCE`: apps que pedirem para salvar podem recusar, e o Windows cancela o encerramento.
    fn encerra(flag: EXIT_WINDOWS_FLAGS) -> Result<(), u32> {
        let motivo = SHUTDOWN_REASON(SHTDN_REASON_MAJOR_APPLICATION.0 | SHTDN_REASON_FLAG_PLANNED.0);
        unsafe { ExitWindowsEx(flag, motivo) }.map_err(codigo)
    }

    /// Habilita `SeShutdownPrivilege` no token do processo: o passo que o desligamento exige.
    fn habilita_desligamento() -> Result<(), u32> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token).map_err(codigo)?;
            let resultado = ajusta_privilegio(token);
            let _ = CloseHandle(token);
            resultado
        }
    }

    unsafe fn ajusta_privilegio(token: HANDLE) -> Result<(), u32> {
        let mut luid = LUID::default();
        LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut luid).map_err(codigo)?;
        let novo = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        AdjustTokenPrivileges(token, false, Some(&novo as *const _), 0, None, None).map_err(codigo)?;
        // Com sucesso, a API pode não ter concedido o privilégio: o motivo fica em GetLastError.
        let ultimo = GetLastError();
        if ultimo == ERROR_NOT_ALL_ASSIGNED {
            Err(ultimo.0)
        } else {
            Ok(())
        }
    }

    pub struct Os;

    impl EnergiaBackend for Os {
        fn pedir(&self, acao: Acao) -> Result<(), u32> {
            match acao {
                Acao::Bloquear => unsafe { LockWorkStation() }.map_err(codigo),
                Acao::Suspender => bool_ou_codigo(unsafe { SetSuspendState(false, false, false) }),
                // O Win32 não tem EWX_RESTART: o reinício é EWX_REBOOT.
                Acao::Reiniciar => habilita_desligamento().and_then(|()| encerra(EWX_REBOOT)),
                Acao::Desligar => habilita_desligamento().and_then(|()| encerra(EWX_SHUTDOWN)),
            }
        }
    }

    pub fn suspensao_suportada() -> bool {
        let mut caps = SYSTEM_POWER_CAPABILITIES::default();
        let lido = unsafe { GetPwrCapabilities(&mut caps) };
        lido && caps.SystemS3
    }

    pub fn privilegio_desligamento_presente() -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let presente = tem_privilegio(token);
            let _ = CloseHandle(token);
            presente
        }
    }

    /// Só lê a lista do token, habilitado ou não.
    unsafe fn tem_privilegio(token: HANDLE) -> bool {
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut luid).is_err() {
            return false;
        }
        let mut tamanho = 0u32;
        let _ = GetTokenInformation(token, TokenPrivileges, None, 0, &mut tamanho);
        if tamanho == 0 {
            return false;
        }
        // u64 alinha o buffer com TOKEN_PRIVILEGES.
        let mut buf = vec![0u64; (tamanho as usize).div_ceil(8)];
        if GetTokenInformation(token, TokenPrivileges, Some(buf.as_mut_ptr().cast()), tamanho, &mut tamanho)
            .is_err()
        {
            return false;
        }
        let lista = buf.as_ptr().cast::<TOKEN_PRIVILEGES>();
        let quantos = (*lista).PrivilegeCount as usize;
        let primeiro = std::ptr::addr_of!((*lista).Privileges).cast::<LUID_AND_ATTRIBUTES>();
        std::slice::from_raw_parts(primeiro, quantos)
            .iter()
            .any(|p| p.Luid == luid)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use tokio::sync::watch;

    use super::*;
    use crate::mochi::registry::{validate_schema, ToolRegistry};
    use crate::mochi::{
        ApprovalFuture, ApprovalPromptRequest, ApprovalRequester, AuditLog, DefaultDenyRequester,
        MochiRuntime, PermissionDecision, PermissionEngine, ToolExecutionOutcome, UserApprovalReply,
    };

    /// Backend falso: conta as chamadas e responde com `recusa` (`None` = aceito).
    struct Falso {
        chamadas: AtomicU32,
        recusa: Option<u32>,
    }

    impl EnergiaBackend for Falso {
        fn pedir(&self, _acao: Acao) -> Result<(), u32> {
            self.chamadas.fetch_add(1, Ordering::SeqCst);
            self.recusa.map_or(Ok(()), Err)
        }
    }

    /// Backend compartilhado pelos testes de permissão. Só um teste o usa, então a contagem é exata.
    static FALSO: Falso = Falso {
        chamadas: AtomicU32::new(0),
        recusa: None,
    };

    /// Responde a um pedido de clique com a resposta dada e conta quantos pedidos chegaram.
    struct Resposta {
        reply: UserApprovalReply,
        pedidos: AtomicU32,
    }

    impl ApprovalRequester for Resposta {
        fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
            self.pedidos.fetch_add(1, Ordering::SeqCst);
            let reply = self.reply;
            Box::pin(async move { reply })
        }
    }

    fn resposta(reply: UserApprovalReply) -> Resposta {
        Resposta {
            reply,
            pedidos: AtomicU32::new(0),
        }
    }

    /// Mesma definição real, com só a execução trocada pelo backend falso. Id e risco são os de produção.
    fn execucao_falsa(_args: Value, ctx: ExecutionContext) -> ToolFuture {
        let acao = acao_do_id(ctx.permit_tool_id());
        Box::pin(async move { run_with(&FALSO, acao) })
    }

    fn acao_do_id(id: &str) -> Acao {
        [Acao::Bloquear, Acao::Suspender, Acao::Reiniciar, Acao::Desligar]
            .into_iter()
            .find(|a| a.id() == id)
            .expect("id de energia conhecido")
    }

    fn registro_falso() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        for def in [bloquear_tela(), suspender(), reiniciar(), desligar()] {
            reg.register_and_enable(ToolDefinition {
                execute: execucao_falsa,
                ..def
            })
            .unwrap();
        }
        reg
    }

    struct Dir(PathBuf);

    impl Dir {
        fn novo(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("lumen-power-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn runtime(tag: &str, registro: ToolRegistry) -> (Dir, MochiRuntime) {
        let dir = Dir::novo(tag);
        std::fs::write(dir.0.join("permissoes.json"), "{\"version\":1,\"alwaysAllow\":[]}\n").unwrap();
        let perms = Arc::new(PermissionEngine::load_from(dir.0.join("permissoes.json")));
        let audit = Arc::new(AuditLog::load_from(dir.0.join("auditoria.json")));
        let rt = MochiRuntime::new_with_parts(Arc::new(registro), perms, audit);
        (dir, rt)
    }

    async fn chama(
        rt: &MochiRuntime,
        id: &str,
        ctx: &ToolContext,
        req: &dyn ApprovalRequester,
    ) -> Result<ToolExecutionOutcome, ToolError> {
        let (_tx, rx) = watch::channel(false);
        rt.invoke_tool(id, json!({}), ctx, req, rx).await
    }

    fn com_capacidades() -> ToolContext {
        ToolContext::new()
            .with_capability("sleep_supported")
            .with_capability("shutdown_privilege")
    }

    #[test]
    fn ids_and_risks_are_fixed_per_tool() {
        let casos = [
            (bloquear_tela(), "bloquear_tela", RiskLevel::Medium),
            (suspender(), "suspender", RiskLevel::High),
            (reiniciar(), "reiniciar", RiskLevel::High),
            (desligar(), "desligar", RiskLevel::High),
        ];
        for (def, id, risco) in casos {
            assert_eq!(def.id, id);
            assert_eq!(def.risk, risco, "{id}");
            assert_eq!(def.timeout, TIMEOUT);
        }
    }

    #[test]
    fn schema_takes_no_arguments() {
        for def in [bloquear_tela(), suspender(), reiniciar(), desligar()] {
            assert!(validate_schema(&def.input_schema, &json!({})).is_ok(), "{}", def.id);
            assert!(
                matches!(
                    validate_schema(&def.input_schema, &json!({ "cmd": "whoami" })),
                    Err(ToolError::InvalidInput(_))
                ),
                "{}",
                def.id
            );
        }
    }

    #[test]
    fn dry_run_warns_about_unsaved_work_on_the_high_risk_tools() {
        assert_eq!((bloquear_tela().dry_run)(&json!({})).unwrap(), "Bloquear a tela agora.");
        for def in [suspender(), reiniciar(), desligar()] {
            let texto = (def.dry_run)(&json!({})).unwrap();
            assert!(texto.contains("trabalho não salvo"), "{}", def.id);
        }
    }

    #[test]
    fn availability_needs_the_matching_capability() {
        let vazio = ToolContext::new();
        assert_eq!(
            (suspender().check_availability)(&vazio),
            Availability::Unavailable {
                reason: "suspensão desligada neste computador".into()
            }
        );
        assert_eq!(
            (suspender().check_availability)(&ToolContext::new().with_capability("sleep_supported")),
            Availability::Available
        );
        assert!(matches!(
            (reiniciar().check_availability)(&vazio),
            Availability::Unavailable { .. }
        ));
        assert!(matches!(
            (desligar().check_availability)(&vazio),
            Availability::Unavailable { .. }
        ));
        assert_eq!(
            (desligar().check_availability)(&ToolContext::new().with_capability("shutdown_privilege")),
            Availability::Available
        );
        // Bloquear só depende do Windows, não de capacidade.
        assert_eq!(
            (bloquear_tela().check_availability)(&vazio) == Availability::Available,
            cfg!(windows)
        );
    }

    #[test]
    fn api_refusal_is_execution_failed_with_win32_code() {
        let recusado = Falso {
            chamadas: AtomicU32::new(0),
            recusa: Some(1314),
        };
        match run_with(&recusado, Acao::Desligar) {
            Err(ToolError::ExecutionFailed(msg)) => assert!(msg.contains("1314"), "{msg}"),
            outro => panic!("esperado ExecutionFailed, veio {outro:?}"),
        }
        assert_eq!(recusado.chamadas.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn accepted_request_reports_aceito_and_verify_checks_only_that() {
        let aceito = Falso {
            chamadas: AtomicU32::new(0),
            recusa: None,
        };
        let out = run_with(&aceito, Acao::Suspender).unwrap();
        assert_eq!(out, json!({ "acao": "suspender", "aceito": true }));
        assert!(verify(&json!({}), &out).is_ok());
        assert!(matches!(
            verify(&json!({}), &json!({ "acao": "suspender", "aceito": false })),
            Err(ToolError::VerificationFailed(_))
        ));
        assert!(matches!(
            verify(&json!({}), &json!({ "acao": "suspender" })),
            Err(ToolError::VerificationFailed(_))
        ));
    }

    #[test]
    fn test_build_never_reaches_the_real_api() {
        // Em cfg(test) o módulo `win` nem é compilado; o pedido cai no backend inerte.
        assert_eq!(
            run_os(Acao::Desligar).unwrap_err(),
            ToolError::ExecutionFailed(format!("o sistema recusou o pedido (erro Win32 {SEM_BACKEND})"))
        );
        assert!(!sleep_supported());
        assert!(!shutdown_privilege());
    }

    #[test]
    fn audit_summary_has_only_action_and_result() {
        let ok: Result<Value, ToolError> = Ok(json!({}));
        assert_eq!(resumo(Acao::Bloquear, &ok), "bloquear_tela: pedido aceito");
        assert_eq!(
            resumo(Acao::Reiniciar, &Err(ToolError::Cancelled)),
            "reiniciar: falha"
        );
    }

    #[tokio::test]
    async fn energia_so_age_com_clique_e_conta_uma_vez_quando_aceita() {
        let (_dir, rt) = runtime("energia-clique", registro_falso());
        let ctx = com_capacidades();

        // Negar, expirar e pausa não chamam o backend.
        let negou = chama(&rt, "suspender", &ctx, &DefaultDenyRequester).await;
        assert!(matches!(negou, Err(ToolError::PermissionDenied(_))));
        let recusou = resposta(UserApprovalReply::Deny);
        assert!(matches!(
            chama(&rt, "suspender", &ctx, &recusou).await,
            Err(ToolError::PermissionDenied(_))
        ));
        let expirou = resposta(UserApprovalReply::Timeout);
        assert_eq!(chama(&rt, "suspender", &ctx, &expirou).await.unwrap_err(), ToolError::Timeout);
        let pausado = ctx.clone().with_paused(true);
        assert_eq!(
            chama(&rt, "suspender", &pausado, &resposta(UserApprovalReply::AllowOnce))
                .await
                .unwrap_err(),
            ToolError::Paused
        );
        assert_eq!(FALSO.chamadas.load(Ordering::SeqCst), 0);

        // Aceito uma vez: executa uma vez e devolve o relato.
        let aceitou = resposta(UserApprovalReply::AllowOnce);
        let out = chama(&rt, "suspender", &ctx, &aceitou).await.unwrap();
        assert_eq!(out.output, json!({ "acao": "suspender", "aceito": true }));
        assert_eq!(out.decision, PermissionDecision::AllowedOnce);
        assert_eq!(FALSO.chamadas.load(Ordering::SeqCst), 1);

        // Os altos não se lembram: "sempre permitir" não dispensa o clique da próxima vez.
        for id in ["reiniciar", "desligar", "suspender"] {
            let sempre = resposta(UserApprovalReply::AllowAlways);
            let out = chama(&rt, id, &ctx, &sempre).await.unwrap();
            assert_eq!(out.decision, PermissionDecision::AllowedOnce, "{id}");
            assert_eq!(sempre.pedidos.load(Ordering::SeqCst), 1, "{id}");
            assert!(matches!(
                chama(&rt, id, &ctx, &DefaultDenyRequester).await,
                Err(ToolError::PermissionDenied(_))
            ), "{id}");
            assert!(!rt.permissions().status().always_allow.iter().any(|x| x == id));
        }
        assert_eq!(FALSO.chamadas.load(Ordering::SeqCst), 4);

        // Bloquear é médio: o "sempre permitir" fica gravado e a próxima chamada não pede clique.
        let sempre = resposta(UserApprovalReply::AllowAlways);
        let out = chama(&rt, "bloquear_tela", &ctx, &sempre).await.unwrap();
        assert_eq!(out.decision, PermissionDecision::AllowedAlways);
        let lembrado = chama(&rt, "bloquear_tela", &ctx, &DefaultDenyRequester).await.unwrap();
        assert_eq!(lembrado.decision, PermissionDecision::AlreadyAlwaysAllowed);
        assert_eq!(FALSO.chamadas.load(Ordering::SeqCst), 6);
    }
}
