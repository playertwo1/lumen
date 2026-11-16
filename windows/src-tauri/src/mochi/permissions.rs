//! Motor de permissões fail-closed do Mochi (M1.2).
//!
//! Invariantes:
//! - Configuração persistida exclusivamente em `mochi-permissions.json` (sem tocar em `settings.json`).
//! - Parser estrito (`deny_unknown_fields`, `version == 1`, IDs válidos e sem duplicatas).
//! - Regra QwenPaw: arquivo ausente, vazio, corrompido, versão != 1, ID inválido ou campo
//!   desconhecido coloca o motor em `FailClosed` e nega TODAS as chamadas (inclusive `Low`),
//!   sem jamais sobrescrever o arquivo inválido no disco.
//! - `PermitToken` tem construtor privado neste módulo; nenhuma execução ocorre sem ele.
//! - `RiskLevel::High` sempre pede clique explícito e nunca pode ser salvo em `alwaysAllow`.

use std::collections::BTreeSet;
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::registry::{is_valid_tool_id, RiskLevel, ToolError};

/// Versão suportada do schema de permissões do Mochi.
pub const PERMISSIONS_SCHEMA_VERSION: u32 = 1;

/// Nome do arquivo de permissões em `platform::config_dir()`.
pub const PERMISSIONS_FILE_NAME: &str = "mochi-permissions.json";

/// Nome do marcador de primeira inicialização em `platform::local_dir()`.
pub const PERMISSIONS_INIT_MARKER_NAME: &str = "mochi-permissions.init";

/// Retorna o caminho padrão de `mochi-permissions.json` (`%APPDATA%\Coucou\mochi-permissions.json`).
pub fn default_config_path() -> PathBuf {
    crate::platform::config_dir().join(PERMISSIONS_FILE_NAME)
}

/// Retorna o caminho padrão do marcador de primeira inicialização (`%LOCALAPPDATA%\Coucou\mochi-permissions.init`).
pub fn default_init_marker_path() -> PathBuf {
    crate::platform::local_dir().join(PERMISSIONS_INIT_MARKER_NAME)
}

/// Estrutura estrita persistida em `mochi-permissions.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionConfigFile {
    pub version: u32,
    pub always_allow: Vec<String>,
}

impl Default for PermissionConfigFile {
    fn default() -> Self {
        Self {
            version: PERMISSIONS_SCHEMA_VERSION,
            always_allow: Vec::new(),
        }
    }
}

/// Decisão que originou um `PermitToken`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionDecision {
    AutoLow,
    AllowedOnce,
    AllowedAlways,
    AlreadyAlwaysAllowed,
}

/// Token não forjável emitido exclusivamente por `PermissionEngine::authorize`.
#[derive(Debug)]
pub struct PermitToken {
    tool_id: String,
    decision: PermissionDecision,
    _private: (),
}

impl PermitToken {
    /// Identificador da ferramenta para a qual este token foi emitido.
    pub fn tool_id(&self) -> &str {
        &self.tool_id
    }

    /// Decisão de permissão que gerou este token.
    pub fn decision(&self) -> PermissionDecision {
        self.decision
    }
}

/// Resposta recebida da camada de aprovação humana para uma chamada `Medium` ou `High`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserApprovalReply {
    AllowOnce,
    AllowAlways,
    Deny,
    Declined,
    Timeout,
    NoResponse,
}

/// Pedido enviado à porta de aprovação (`ApprovalRequester`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPromptRequest {
    pub tool_id: String,
    pub summary: String,
    pub risk: RiskLevel,
    pub allow_always: bool,
}

/// Futuro retornado por `ApprovalRequester::request`.
pub type ApprovalFuture<'a> = Pin<Box<dyn Future<Output = UserApprovalReply> + Send + 'a>>;

/// Porta assíncrona de confirmação explícita pelo usuário.
pub trait ApprovalRequester: Send + Sync {
    fn request<'a>(&'a self, prompt: ApprovalPromptRequest) -> ApprovalFuture<'a>;
}

/// Implementação padrão sem UI que nunca concede permissão (`NoResponse`).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultDenyRequester;

impl ApprovalRequester for DefaultDenyRequester {
    fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
        Box::pin(async move { UserApprovalReply::NoResponse })
    }
}

/// Estado resumido das permissões retornado para a UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub healthy: bool,
    pub fail_closed_reason: Option<String>,
    pub always_allow: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EngineState {
    Loaded(PermissionConfigFile),
    FailClosed { reason: String },
}

/// Motor de permissões fail-closed do Mochi.
#[derive(Debug)]
pub struct PermissionEngine {
    config_path: PathBuf,
    state: Mutex<EngineState>,
}

impl PermissionEngine {
    /// Cria o motor apontando para `%APPDATA%\Coucou\mochi-permissions.json`.
    pub fn new_default() -> Self {
        Self::load_from(default_config_path())
    }

    /// Carrega e valida `mochi-permissions.json` a partir de `path`, entrando em
    /// `FailClosed` se ausente ou inválido (nunca cria nem sobrescreve o arquivo).
    pub fn load_from(path: impl Into<PathBuf>) -> Self {
        let config_path = path.into();
        let initial = match load_config_from(&config_path) {
            Ok(cfg) => EngineState::Loaded(cfg),
            Err(reason) => EngineState::FailClosed { reason },
        };
        Self {
            config_path,
            state: Mutex::new(initial),
        }
    }

    /// Caminho do arquivo `mochi-permissions.json` gerenciado por este motor.
    #[allow(dead_code)]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Recarrega do disco e retorna o estado atual para exibição/diagnóstico.
    pub fn status(&self) -> PermissionStatus {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        reload_locked(&self.config_path, &mut guard);
        match &*guard {
            EngineState::Loaded(cfg) => PermissionStatus {
                healthy: true,
                fail_closed_reason: None,
                always_allow: cfg.always_allow.clone(),
            },
            EngineState::FailClosed { reason } => PermissionStatus {
                healthy: false,
                fail_closed_reason: Some(reason.clone()),
                always_allow: Vec::new(),
            },
        }
    }

    /// Autoriza a execução de `tool_id` de acordo com `risk` e configuração atual.
    ///
    /// Sempre recarrega e valida o arquivo antes e depois de eventuais prompts para
    /// detectar corrupção ou remoção em tempo de execução sem segurar mutex durante `.await`.
    pub async fn authorize(
        &self,
        tool_id: &str,
        risk: RiskLevel,
        summary: &str,
        requester: &dyn ApprovalRequester,
    ) -> Result<PermitToken, ToolError> {
        self.authorize_with_guard(
            tool_id,
            risk,
            summary,
            requester,
            || crate::integrations::PAUSED.load(Ordering::Relaxed),
            None,
        )
        .await
    }

    /// Autoriza a execução verificando pausa rápida e cancelamento antes de pedir clique
    /// e antes de persistir `alwaysAllow` em `mochi-permissions.json`.
    pub async fn authorize_with_guard(
        &self,
        tool_id: &str,
        risk: RiskLevel,
        summary: &str,
        requester: &dyn ApprovalRequester,
        is_paused: impl Fn() -> bool,
        cancel_rx: Option<&watch::Receiver<bool>>,
    ) -> Result<PermitToken, ToolError> {
        if is_paused() {
            return Err(ToolError::Paused);
        }
        if cancel_rx.is_some_and(|rx| *rx.borrow()) {
            return Err(ToolError::Cancelled);
        }

        let preflight = {
            let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            reload_locked(&self.config_path, &mut guard);
            let cfg = match &*guard {
                EngineState::Loaded(cfg) => cfg,
                EngineState::FailClosed { reason } => {
                    return Err(ToolError::PermissionDenied(reason.clone()));
                }
            };
            if !is_valid_tool_id(tool_id) {
                return Err(ToolError::PermissionDenied(format!(
                    "identificador de ferramenta inválido: '{tool_id}'"
                )));
            }
            match risk {
                RiskLevel::Low => Some(PermissionDecision::AutoLow),
                RiskLevel::Medium => {
                    if cfg.always_allow.iter().any(|id| id == tool_id) {
                        Some(PermissionDecision::AlreadyAlwaysAllowed)
                    } else {
                        None
                    }
                }
                // Risco alto ignora qualquer entrada manual em `alwaysAllow` e pede sempre.
                RiskLevel::High => None,
            }
        };

        if let Some(decision) = preflight {
            return Ok(PermitToken {
                tool_id: tool_id.to_string(),
                decision,
                _private: (),
            });
        }

        if is_paused() {
            return Err(ToolError::Paused);
        }
        if cancel_rx.is_some_and(|rx| *rx.borrow()) {
            return Err(ToolError::Cancelled);
        }

        let prompt = ApprovalPromptRequest {
            tool_id: tool_id.to_string(),
            summary: summary.to_string(),
            risk,
            allow_always: risk == RiskLevel::Medium,
        };
        let reply = requester.request(prompt).await;

        if is_paused() {
            return Err(ToolError::Paused);
        }
        if cancel_rx.is_some_and(|rx| *rx.borrow()) {
            return Err(ToolError::Cancelled);
        }

        let persist_always = match reply {
            UserApprovalReply::AllowOnce => false,
            // Mesmo que um requester mal-comportado retorne AllowAlways para High, nunca persiste.
            UserApprovalReply::AllowAlways => risk == RiskLevel::Medium,
            UserApprovalReply::Deny => {
                return Err(ToolError::PermissionDenied(
                    "negado pelo usuário".to_string(),
                ));
            }
            UserApprovalReply::Declined => {
                return Err(ToolError::PermissionDenied(
                    "pedido de aprovação recusado".to_string(),
                ));
            }
            UserApprovalReply::NoResponse => {
                return Err(ToolError::PermissionDenied(
                    "sem resposta de aprovação".to_string(),
                ));
            }
            UserApprovalReply::Timeout => {
                return Err(ToolError::Timeout);
            }
        };

        // Revalida após o `.await` para impedir execução ou gravação se o arquivo
        // foi removido ou corrompido enquanto aguardava o clique.
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        reload_locked(&self.config_path, &mut guard);
        let cfg = match &mut *guard {
            EngineState::Loaded(cfg) => cfg,
            EngineState::FailClosed { reason } => {
                return Err(ToolError::PermissionDenied(reason.clone()));
            }
        };

        let decision = if persist_always {
            if is_paused() {
                return Err(ToolError::Paused);
            }
            if cancel_rx.is_some_and(|rx| *rx.borrow()) {
                return Err(ToolError::Cancelled);
            }
            if !cfg.always_allow.iter().any(|id| id == tool_id) {
                cfg.always_allow.push(tool_id.to_string());
                cfg.always_allow.sort();
                cfg.always_allow.dedup();
            }
            write_config_atomic(&self.config_path, cfg).map_err(ToolError::PermissionDenied)?;
            PermissionDecision::AllowedAlways
        } else {
            PermissionDecision::AllowedOnce
        };

        Ok(PermitToken {
            tool_id: tool_id.to_string(),
            decision,
            _private: (),
        })
    }

    /// Concede `"Sempre permitir"` manualmente para uma ferramenta (recusa `RiskLevel::High`).
    pub fn grant_always_allow(&self, tool_id: &str, risk: RiskLevel) -> Result<(), String> {
        if !is_valid_tool_id(tool_id) {
            return Err(format!("identificador de ferramenta inválido: '{tool_id}'"));
        }
        if risk == RiskLevel::High {
            return Err(
                "ferramentas de risco alto nunca podem ser gravadas em alwaysAllow".to_string(),
            );
        }

        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        reload_locked(&self.config_path, &mut guard);
        let cfg = match &mut *guard {
            EngineState::Loaded(cfg) => cfg,
            EngineState::FailClosed { reason } => return Err(reason.clone()),
        };

        if !cfg.always_allow.iter().any(|id| id == tool_id) {
            cfg.always_allow.push(tool_id.to_string());
            cfg.always_allow.sort();
            cfg.always_allow.dedup();
            write_config_atomic(&self.config_path, cfg)?;
        }
        Ok(())
    }

    /// Remove `tool_id` de `alwaysAllow` e persiste atomicamente.
    pub fn revoke_always_allow(&self, tool_id: &str) -> Result<bool, String> {
        if !is_valid_tool_id(tool_id) {
            return Err(format!("identificador de ferramenta inválido: '{tool_id}'"));
        }

        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        reload_locked(&self.config_path, &mut guard);
        let cfg = match &mut *guard {
            EngineState::Loaded(cfg) => cfg,
            EngineState::FailClosed { reason } => return Err(reason.clone()),
        };

        let before = cfg.always_allow.len();
        cfg.always_allow.retain(|id| id != tool_id);
        let removed = cfg.always_allow.len() != before;
        if removed {
            cfg.always_allow.sort();
            write_config_atomic(&self.config_path, cfg)?;
        }
        Ok(removed)
    }

    /// Restaura `{"version":1,"alwaysAllow":[]}` apenas por ação explícita do usuário.
    pub fn reset_default(&self) -> Result<(), String> {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let cfg = PermissionConfigFile::default();
        write_config_atomic(&self.config_path, &cfg)?;
        *guard = EngineState::Loaded(cfg);
        Ok(())
    }
}

/// Faz parse estrito *fail-closed* do conteúdo textual de `mochi-permissions.json`.
pub fn parse_config(raw: &str) -> Result<PermissionConfigFile, String> {
    let without_bom = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let trimmed = without_bom.trim();
    if trimmed.is_empty() {
        return Err("arquivo de permissões vazio".to_string());
    }

    let config: PermissionConfigFile = serde_json::from_str(trimmed)
        .map_err(|err| format!("configuração de permissões inválida: {err}"))?;

    if config.version != PERMISSIONS_SCHEMA_VERSION {
        return Err(format!(
            "versão de permissões não suportada: {} (esperada {})",
            config.version, PERMISSIONS_SCHEMA_VERSION
        ));
    }

    let mut seen = BTreeSet::new();
    for id in &config.always_allow {
        if !is_valid_tool_id(id) {
            return Err(format!(
                "identificador de ferramenta inválido em alwaysAllow: '{id}'"
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!(
                "identificador de ferramenta duplicado em alwaysAllow: '{id}'"
            ));
        }
    }

    Ok(config)
}

/// Lê e valida `mochi-permissions.json` de `path` de forma estritamente *fail-closed*.
pub fn load_config_from(path: &Path) -> Result<PermissionConfigFile, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err("arquivo de permissões ausente".to_string());
        }
        Err(err) => {
            return Err(format!(
                "erro ao ler arquivo de permissões: {}",
                err.kind()
            ));
        }
    };
    let raw = std::str::from_utf8(&bytes)
        .map_err(|_| "arquivo de permissões não está em UTF-8 válido".to_string())?;
    parse_config(raw)
}

/// Cria `{"version":1,"alwaysAllow":[]}` e o marcador `.init` apenas na 1ª inicialização.
///
/// Se o arquivo foi removido após `.init` existir, ou se já existe (mesmo corrompido),
/// nada é recriado nem sobrescrito.
pub fn bootstrap_default_if_first_run(
    config_path: &Path,
    init_marker_path: &Path,
) -> Result<(), String> {
    if path_exists_or_unreadable(config_path)? || path_exists_or_unreadable(init_marker_path)? {
        return Ok(());
    }

    write_config_atomic(config_path, &PermissionConfigFile::default())?;
    ensure_parent_private_dir(init_marker_path)?;
    crate::tasks::store::write_atomic(init_marker_path, b"")
}

fn reload_locked(path: &Path, state: &mut EngineState) {
    match load_config_from(path) {
        Ok(cfg) => *state = EngineState::Loaded(cfg),
        Err(reason) => *state = EngineState::FailClosed { reason },
    }
}

fn ensure_parent_private_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            crate::platform::ensure_private_dir(parent).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn write_config_atomic(path: &Path, config: &PermissionConfigFile) -> Result<(), String> {
    ensure_parent_private_dir(path)?;
    let mut bytes = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    crate::tasks::store::write_atomic(path, &bytes)
}

fn path_exists_or_unreadable(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(format!(
            "não foi possível verificar estado de permissões: {}",
            err.kind()
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

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
                "lumen-mochi-perm-{tag}-{}-{nanos}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn config(&self) -> PathBuf {
            self.path.join(PERMISSIONS_FILE_NAME)
        }

        fn marker(&self) -> PathBuf {
            self.path.join(PERMISSIONS_INIT_MARKER_NAME)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct StubRequester {
        reply: UserApprovalReply,
        calls: AtomicUsize,
        last_prompt: StdMutex<Option<ApprovalPromptRequest>>,
    }

    impl StubRequester {
        fn new(reply: UserApprovalReply) -> Self {
            Self {
                reply,
                calls: AtomicUsize::new(0),
                last_prompt: StdMutex::new(None),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_prompt(&self) -> Option<ApprovalPromptRequest> {
            self.last_prompt.lock().unwrap().clone()
        }
    }

    impl ApprovalRequester for StubRequester {
        fn request<'a>(&'a self, prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_prompt.lock().unwrap() = Some(prompt);
            let reply = self.reply;
            Box::pin(async move { reply })
        }
    }

    fn write_valid_default(path: &Path) {
        std::fs::write(path, b"{\"version\":1,\"alwaysAllow\":[]}\n").unwrap();
    }

    #[tokio::test]
    async fn low_risk_executes_directly_when_config_is_valid() {
        let dir = TempDir::new("low");
        write_valid_default(&dir.config());

        let engine = PermissionEngine::load_from(dir.config());
        let requester = StubRequester::new(UserApprovalReply::Deny);

        let token = engine
            .authorize("ping", RiskLevel::Low, "Executar ping local.", &requester)
            .await
            .expect("risco baixo com config válida deve autorizar direto");

        assert_eq!(token.tool_id(), "ping");
        assert_eq!(token.decision(), PermissionDecision::AutoLow);
        assert_eq!(requester.call_count(), 0);
        assert!(engine.status().healthy);
    }

    #[tokio::test]
    async fn medium_risk_prompts_once_persists_always_allow_and_revokes() {
        let dir = TempDir::new("medium");
        write_valid_default(&dir.config());

        let engine = PermissionEngine::load_from(dir.config());

        // 1ª chamada com AllowOnce: executa 1x e não persiste.
        let once_req = StubRequester::new(UserApprovalReply::AllowOnce);
        let t1 = engine
            .authorize("med_tool", RiskLevel::Medium, "Ajustar nota local.", &once_req)
            .await
            .expect("AllowOnce deve autorizar");
        assert_eq!(t1.decision(), PermissionDecision::AllowedOnce);
        assert_eq!(once_req.call_count(), 1);
        assert_eq!(
            once_req.last_prompt().unwrap(),
            ApprovalPromptRequest {
                tool_id: "med_tool".to_string(),
                summary: "Ajustar nota local.".to_string(),
                risk: RiskLevel::Medium,
                allow_always: true,
            }
        );
        assert!(engine.status().always_allow.is_empty());

        // 2ª chamada ainda precisa pedir clique.
        let always_req = StubRequester::new(UserApprovalReply::AllowAlways);
        let t2 = engine
            .authorize("med_tool", RiskLevel::Medium, "Ajustar nota local.", &always_req)
            .await
            .expect("AllowAlways deve autorizar e persistir");
        assert_eq!(t2.decision(), PermissionDecision::AllowedAlways);
        assert_eq!(always_req.call_count(), 1);

        // Recarrega do disco em nova instância para comprovar persistência em mochi-permissions.json.
        let reloaded = PermissionEngine::load_from(dir.config());
        assert_eq!(reloaded.status().always_allow, vec!["med_tool".to_string()]);
        let t3 = reloaded
            .authorize(
                "med_tool",
                RiskLevel::Medium,
                "Ajustar nota local.",
                &DefaultDenyRequester,
            )
            .await
            .expect("alwaysAllow persistido não deve chamar requester");
        assert_eq!(t3.decision(), PermissionDecision::AlreadyAlwaysAllowed);

        // Revogar remove do arquivo e faz a próxima chamada voltar a pedir.
        assert!(reloaded.revoke_always_allow("med_tool").unwrap());
        assert!(!reloaded.revoke_always_allow("med_tool").unwrap());
        assert!(reloaded.status().always_allow.is_empty());

        let denied = reloaded
            .authorize(
                "med_tool",
                RiskLevel::Medium,
                "Ajustar nota local.",
                &DefaultDenyRequester,
            )
            .await
            .unwrap_err();
        assert!(matches!(denied, ToolError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn high_risk_always_prompts_and_ignores_always_allow() {
        let dir = TempDir::new("high");
        // Mesmo com edição manual colocando a ferramenta High em alwaysAllow, não pode auto-autorizar.
        std::fs::write(
            dir.config(),
            b"{\"version\":1,\"alwaysAllow\":[\"danger_tool\"]}\n",
        )
        .unwrap();

        let engine = PermissionEngine::load_from(dir.config());
        assert!(engine
            .grant_always_allow("danger_tool", RiskLevel::High)
            .is_err());

        // Mesmo que um requester mal-comportado retorne AllowAlways, emite só AllowedOnce e não grava.
        let bad_req = StubRequester::new(UserApprovalReply::AllowAlways);
        let token = engine
            .authorize("danger_tool", RiskLevel::High, "Ação crítica.", &bad_req)
            .await
            .expect("clique em High autoriza somente esta chamada");

        assert_eq!(bad_req.call_count(), 1);
        assert!(!bad_req.last_prompt().unwrap().allow_always);
        assert_eq!(token.decision(), PermissionDecision::AllowedOnce);

        // Em arquivo limpo, AllowAlways também nunca grava ferramenta High e a próxima pede de novo.
        write_valid_default(&dir.config());
        let clean_engine = PermissionEngine::load_from(dir.config());
        let t2 = clean_engine
            .authorize("danger_tool", RiskLevel::High, "Ação crítica.", &bad_req)
            .await
            .unwrap();
        assert_eq!(t2.decision(), PermissionDecision::AllowedOnce);
        assert!(clean_engine.status().always_allow.is_empty());

        let err = clean_engine
            .authorize(
                "danger_tool",
                RiskLevel::High,
                "Ação crítica.",
                &DefaultDenyRequester,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn non_approval_replies_never_emit_token_or_persist() {
        let dir = TempDir::new("replies");
        write_valid_default(&dir.config());
        let engine = PermissionEngine::load_from(dir.config());

        for reply in [
            UserApprovalReply::NoResponse,
            UserApprovalReply::Deny,
            UserApprovalReply::Declined,
        ] {
            let req = StubRequester::new(reply);
            let err = engine
                .authorize("med_tool", RiskLevel::Medium, "Teste.", &req)
                .await
                .unwrap_err();
            assert!(matches!(err, ToolError::PermissionDenied(_)));
            assert_eq!(req.call_count(), 1);
        }

        let timeout_req = StubRequester::new(UserApprovalReply::Timeout);
        let err = engine
            .authorize("med_tool", RiskLevel::Medium, "Teste.", &timeout_req)
            .await
            .unwrap_err();
        assert_eq!(err, ToolError::Timeout);
        assert!(engine.status().always_allow.is_empty());
    }

    #[tokio::test]
    async fn qwenpaw_invalid_or_missing_configs_fail_closed_without_overwriting() {
        let dir = TempDir::new("failclosed");
        let missing_engine = PermissionEngine::load_from(dir.config());
        let status = missing_engine.status();
        assert!(!status.healthy);
        assert!(status.fail_closed_reason.is_some());

        let allow_req = StubRequester::new(UserApprovalReply::AllowAlways);
        assert!(matches!(
            missing_engine
                .authorize("ping", RiskLevel::Low, "ping", &allow_req)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert_eq!(allow_req.call_count(), 0);
        assert!(!dir.config().exists());

        let too_long_id = format!(r#"{{"version":1,"alwaysAllow":["{}"]}}"#, "a".repeat(65));
        let invalid_cases: Vec<&str> = vec![
            "",
            "   \n\t ",
            "{not valid json",
            "{}",
            "[]",
            "null",
            "\"AUTO\"",
            r#"{"version":2,"alwaysAllow":[]}"#,
            r#"{"version":0,"alwaysAllow":[]}"#,
            r#"{"version":1,"alwaysAllow":["../evil"]}"#,
            r#"{"version":1,"alwaysAllow":[""]}"#,
            r#"{"version":1,"alwaysAllow":["UpperCase"]}"#,
            r#"{"version":1,"alwaysAllow":["ping","ping"]}"#,
            r#"{"version":1,"alwaysAllow":[],"mode":"AUTO"}"#,
            &too_long_id,
        ];

        for raw in invalid_cases {
            std::fs::write(dir.config(), raw.as_bytes()).unwrap();
            let engine = PermissionEngine::load_from(dir.config());
            let st = engine.status();
            assert!(!st.healthy, "deveria falhar fechado para: {raw}");
            assert!(
                matches!(
                    engine
                        .authorize("ping", RiskLevel::Low, "ping", &allow_req)
                        .await
                        .unwrap_err(),
                    ToolError::PermissionDenied(_)
                ),
                "ping deveria ser negado em FailClosed para: {raw}"
            );
            assert!(engine.revoke_always_allow("ping").is_err());
            assert!(engine.grant_always_allow("med", RiskLevel::Medium).is_err());
            // Arquivo inválido nunca pode ser sobrescrito automaticamente.
            assert_eq!(std::fs::read_to_string(dir.config()).unwrap(), raw);
        }

        // Restauração explícita recupera o estado saudável.
        missing_engine.reset_default().unwrap();
        assert!(missing_engine.status().healthy);
        assert!(missing_engine
            .authorize("ping", RiskLevel::Low, "ping", &DefaultDenyRequester)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn utf8_bom_is_accepted_on_valid_config_and_runtime_corruption_trips_fail_closed() {
        let dir = TempDir::new("bom");
        let bom_valid = "\u{FEFF}{\"version\":1,\"alwaysAllow\":[\"med_tool\"]}";
        std::fs::write(dir.config(), bom_valid.as_bytes()).unwrap();

        let engine = PermissionEngine::load_from(dir.config());
        assert!(engine.status().healthy);
        let tok = engine
            .authorize("med_tool", RiskLevel::Medium, "ok", &DefaultDenyRequester)
            .await
            .unwrap();
        assert_eq!(tok.decision(), PermissionDecision::AlreadyAlwaysAllowed);

        // Corrupção em tempo de execução é detectada na chamada seguinte.
        std::fs::write(
            dir.config(),
            b"{\"version\":1,\"alwaysAllow\":[],\"mode\":\"AUTO\"}",
        )
        .unwrap();
        assert!(matches!(
            engine
                .authorize("ping", RiskLevel::Low, "ping", &DefaultDenyRequester)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
        assert!(!engine.status().healthy);
    }

    #[tokio::test]
    async fn corruption_during_prompt_prevents_grant_and_execution() {
        struct CorruptingRequester {
            path: PathBuf,
        }

        impl ApprovalRequester for CorruptingRequester {
            fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
                let path = self.path.clone();
                Box::pin(async move {
                    std::fs::write(&path, b"{corrupted").unwrap();
                    UserApprovalReply::AllowAlways
                })
            }
        }

        let dir = TempDir::new("race");
        write_valid_default(&dir.config());
        let engine = PermissionEngine::load_from(dir.config());
        let req = CorruptingRequester { path: dir.config() };

        let err = engine
            .authorize("med_tool", RiskLevel::Medium, "teste", &req)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
        assert_eq!(std::fs::read_to_string(dir.config()).unwrap(), "{corrupted");
    }

    #[tokio::test]
    async fn bootstrap_creates_v1_only_on_first_run_and_stays_fail_closed_if_deleted() {
        let dir = TempDir::new("bootstrap");

        // 1ª execução: sem arquivo e sem .init -> cria ambos.
        bootstrap_default_if_first_run(&dir.config(), &dir.marker()).unwrap();
        assert!(dir.config().exists());
        assert!(dir.marker().exists());

        let parsed = load_config_from(&dir.config()).unwrap();
        assert_eq!(
            parsed,
            PermissionConfigFile {
                version: 1,
                always_allow: Vec::new(),
            }
        );

        // Config personalizada existente não é sobrescrita em inicializações seguintes.
        let engine = PermissionEngine::load_from(dir.config());
        engine
            .grant_always_allow("med_tool", RiskLevel::Medium)
            .unwrap();
        bootstrap_default_if_first_run(&dir.config(), &dir.marker()).unwrap();
        assert_eq!(engine.status().always_allow, vec!["med_tool".to_string()]);

        // Apagar mochi-permissions.json mantendo .init não recria e mantém FailClosed.
        std::fs::remove_file(dir.config()).unwrap();
        bootstrap_default_if_first_run(&dir.config(), &dir.marker()).unwrap();
        assert!(!dir.config().exists());

        let after_delete = PermissionEngine::load_from(dir.config());
        assert!(!after_delete.status().healthy);
        assert!(matches!(
            after_delete
                .authorize("ping", RiskLevel::Low, "ping", &DefaultDenyRequester)
                .await
                .unwrap_err(),
            ToolError::PermissionDenied(_)
        ));

        // Mesmo sem .init, um arquivo pré-existente (inclusive corrompido) nunca é sobrescrito.
        let dir2 = TempDir::new("bootstrap_existing");
        std::fs::write(dir2.config(), b"{\"mode\":\"AUTO\"}").unwrap();
        bootstrap_default_if_first_run(&dir2.config(), &dir2.marker()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir2.config()).unwrap(),
            r#"{"mode":"AUTO"}"#
        );
        assert!(!dir2.marker().exists());
    }

    #[test]
    fn parse_config_handles_bom_and_rejects_duplicates_and_unknown_fields() {
        let ok = parse_config("\u{FEFF}  {\"version\":1,\"alwaysAllow\":[\"a_1\",\"b-2\"]} ").unwrap();
        assert_eq!(ok.version, 1);
        assert_eq!(ok.always_allow, vec!["a_1".to_string(), "b-2".to_string()]);

        assert!(parse_config("\u{FEFF}   ").is_err());
        assert!(parse_config(r#"{"version":1,"alwaysAllow":["a","a"]}"#).is_err());
        assert!(parse_config(r#"{"version":1,"alwaysAllow":[],"mode":"AUTO"}"#).is_err());
    }

    #[tokio::test]
    async fn paused_or_cancelled_never_persists_always_allow() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        struct PausingRequester {
            flag: Arc<AtomicBool>,
        }
        impl ApprovalRequester for PausingRequester {
            fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
                Box::pin(async move {
                    self.flag.store(true, Ordering::SeqCst);
                    UserApprovalReply::AllowAlways
                })
            }
        }

        let dir = TempDir::new("paused-no-persist");
        std::fs::write(dir.config(), "{\"version\":1,\"alwaysAllow\":[]}\n").unwrap();
        let engine = PermissionEngine::load_from(dir.config());

        let paused = Arc::new(AtomicBool::new(false));
        let req = PausingRequester {
            flag: Arc::clone(&paused),
        };
        let err = engine
            .authorize_with_guard(
                "med_tool",
                RiskLevel::Medium,
                "teste",
                &req,
                || paused.load(Ordering::SeqCst),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err, ToolError::Paused);
        assert!(engine.status().always_allow.is_empty());

        let (cancel_tx, cancel_rx) = watch::channel(false);
        struct CancellingRequester {
            tx: watch::Sender<bool>,
        }
        impl ApprovalRequester for CancellingRequester {
            fn request<'a>(&'a self, _prompt: ApprovalPromptRequest) -> ApprovalFuture<'a> {
                Box::pin(async move {
                    let _ = self.tx.send(true);
                    UserApprovalReply::AllowAlways
                })
            }
        }
        let cancel_req = CancellingRequester { tx: cancel_tx };
        let err_cancel = engine
            .authorize_with_guard(
                "med_tool",
                RiskLevel::Medium,
                "teste",
                &cancel_req,
                || false,
                Some(&cancel_rx),
            )
            .await
            .unwrap_err();
        assert_eq!(err_cancel, ToolError::Cancelled);
        assert!(engine.status().always_allow.is_empty());
    }
}
