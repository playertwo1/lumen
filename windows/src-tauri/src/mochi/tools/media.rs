//! Ferramenta `midia` (M2.5): toca, pausa, avança ou volta a faixa do player de mídia ativo do Windows.
//!
//! `acao` é enum. Só a sessão que o Windows marca como atual recebe o comando; nenhum texto do pedido
//! chega ao sistema, e nada é aberto quando não há player.

use std::time::Duration;

use serde_json::{json, Value};

use crate::mochi::registry::{
    Availability, ExecutionContext, RiskLevel, ToolContext, ToolDefinition, ToolError, ToolFuture,
};

const SEM_PLAYER: &str = "nenhum player de mídia ativo";
const NADA_ABERTO: &str = "nenhum player de mídia ativo; nada foi aberto";
const RECUSADO: &str = "o player recusou o pedido";
const ILEGIVEL: &str = "o player não informou o estado";
/// A troca de faixa demora: até 1,5 s de releituras, em passos de 250 ms.
const PASSOS: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Tocar,
    Pausar,
    Proxima,
    Anterior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Estado {
    Tocando,
    Pausado,
    Outro,
}

/// Estado da sessão atual. `faixa` é um hash de título, artista e álbum: serve só para comparar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Leitura {
    pub estado: Estado,
    pub faixa: Option<u64>,
}

/// A sessão atual do Windows. `None` quando a leitura ou o comando não podem ser feitos.
pub(crate) trait Sessao {
    fn leitura(&self) -> Option<Leitura>;
    /// `true` quando o player aceitou o pedido.
    fn controle(&self, op: Op) -> bool;
}

/// Qual sessão é a atual, e a pausa entre leituras. Os testes usam um falso.
pub(crate) trait MidiaBackend {
    /// Só a sessão marcada como atual; nunca a lista de sessões.
    fn atual(&self) -> Option<Box<dyn Sessao>>;
    /// Espera entre duas leituras. Nos testes não dorme.
    fn pausa(&self);
}

pub fn tool() -> ToolDefinition {
    ToolDefinition {
        id: "midia",
        description: "Toca, pausa, avança ou volta a faixa do player de mídia ativo do Windows.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "acao": { "type": "string", "enum": ["tocar", "pausar", "proxima", "anterior"] }
            },
            "required": ["acao"],
            "additionalProperties": false
        }),
        risk: RiskLevel::Low,
        timeout: Duration::from_secs(5),
        check_availability,
        dry_run,
        execute,
        verify,
        audit_summary,
    }
}

fn check_availability(ctx: &ToolContext) -> Availability {
    if ctx.has_capability("media_session") {
        Availability::Available
    } else {
        Availability::Unavailable {
            reason: SEM_PLAYER.to_string(),
        }
    }
}

fn plano(args: &Value) -> Result<Op, ToolError> {
    match args.get("acao").and_then(Value::as_str) {
        Some("tocar") => Ok(Op::Tocar),
        Some("pausar") => Ok(Op::Pausar),
        Some("proxima") => Ok(Op::Proxima),
        Some("anterior") => Ok(Op::Anterior),
        _ => Err(ToolError::InvalidInput("ação de mídia desconhecida".into())),
    }
}

fn acao(op: Op) -> &'static str {
    match op {
        Op::Tocar => "tocar",
        Op::Pausar => "pausar",
        Op::Proxima => "proxima",
        Op::Anterior => "anterior",
    }
}

/// O que o pedido espera ver depois da ação: um status, ou uma faixa nova.
fn pedido(op: Op) -> &'static str {
    match op {
        Op::Tocar => "tocando",
        Op::Pausar => "pausado",
        Op::Proxima | Op::Anterior => "troca_de_faixa",
    }
}

fn dry_run(args: &Value) -> Result<String, ToolError> {
    Ok(match plano(args)? {
        Op::Tocar => "Tocar no player de mídia ativo do Windows.".to_string(),
        Op::Pausar => "Pausar o player de mídia ativo do Windows.".to_string(),
        Op::Proxima => "Pular para a próxima faixa do player de mídia ativo.".to_string(),
        Op::Anterior => "Voltar para a faixa anterior do player de mídia ativo.".to_string(),
    })
}

fn confirmado(op: Op, antes: &Leitura, depois: &Leitura) -> bool {
    match op {
        Op::Tocar => depois.estado == Estado::Tocando,
        Op::Pausar => depois.estado == Estado::Pausado,
        Op::Proxima | Op::Anterior => depois.faixa != antes.faixa,
    }
}

/// Age na sessão atual, relê até confirmar ou esgotar a espera, e devolve o pedido ao lado do observado.
pub(crate) fn run_with(backend: &dyn MidiaBackend, op: Op) -> Result<Value, ToolError> {
    let sessao = backend
        .atual()
        .ok_or_else(|| ToolError::PrerequisiteMissing(NADA_ABERTO.into()))?;
    let antes = sessao.leitura().ok_or_else(|| ToolError::ExecutionFailed(ILEGIVEL.into()))?;
    if !sessao.controle(op) {
        return Err(ToolError::ExecutionFailed(RECUSADO.into()));
    }
    let mut depois = sessao.leitura().ok_or_else(|| ToolError::ExecutionFailed(ILEGIVEL.into()))?;
    let mut passos = 0;
    while !confirmado(op, &antes, &depois) && passos < PASSOS {
        backend.pausa();
        passos += 1;
        depois = sessao.leitura().ok_or_else(|| ToolError::ExecutionFailed(ILEGIVEL.into()))?;
    }
    let status = match depois.estado {
        Estado::Tocando => "tocando",
        Estado::Pausado => "pausado",
        Estado::Outro => "outro",
    };
    Ok(json!({
        "acao": acao(op),
        "status": status,
        "faixa_mudou": depois.faixa != antes.faixa,
        "pedido": pedido(op),
    }))
}

pub(crate) fn esp32_media_with(backend: &dyn MidiaBackend, action: &str) -> Result<bool, String> {
    let sessao = backend
        .atual()
        .ok_or_else(|| "no_active_player".to_string())?;
    let antes = sessao
        .leitura()
        .ok_or_else(|| "player_unreadable".to_string())?;
    let (op, target_playing) = match action {
        "play_pause" => {
            if antes.estado == Estado::Tocando {
                (Op::Pausar, false)
            } else {
                (Op::Tocar, true)
            }
        }
        "next" => (Op::Proxima, antes.estado == Estado::Tocando),
        "prev" => (Op::Anterior, antes.estado == Estado::Tocando),
        _ => return Err("invalid_action".into()),
    };
    if !sessao.controle(op) {
        return Err("player_rejected".into());
    }
    Ok(target_playing)
}

#[cfg(all(windows, not(test)))]
pub(crate) fn esp32_media_control(action: &str) -> Result<bool, String> {
    esp32_media_with(&win::Os, action)
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn esp32_media_control(_action: &str) -> Result<bool, String> {
    Err("no_active_player".into())
}

fn execute(args: Value, ctx: ExecutionContext) -> ToolFuture {
    Box::pin(async move {
        let op = plano(&args)?;
        if ctx.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        // COM fica na thread do bloqueio e não atravessa `await`.
        tokio::task::spawn_blocking(move || run_os(op))
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
    })
}

#[cfg(all(windows, not(test)))]
fn run_os(op: Op) -> Result<Value, ToolError> {
    let _com = crate::mochi::tools::audio::win::Com::init();
    run_with(&win::Os, op)
}

#[cfg(not(all(windows, not(test))))]
fn run_os(op: Op) -> Result<Value, ToolError> {
    run_with(&SemPlayer, op)
}

/// Fora do Windows e nos testes: não há player, e nada é tocado.
#[cfg(not(all(windows, not(test))))]
struct SemPlayer;

#[cfg(not(all(windows, not(test))))]
impl MidiaBackend for SemPlayer {
    fn atual(&self) -> Option<Box<dyn Sessao>> {
        None
    }

    fn pausa(&self) {}
}

/// Há sessão de mídia atual? Sondagem para `host::context`; nunca roda nos testes.
#[cfg(all(windows, not(test)))]
pub(crate) fn session_present() -> bool {
    let _com = crate::mochi::tools::audio::win::Com::init();
    // Guarda o resultado antes do COM sair de escopo, como em `audio::output_present`.
    let presente = win::Os.atual().is_some();
    presente
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn session_present() -> bool {
    false
}

fn verify(_args: &Value, output: &Value) -> Result<(), ToolError> {
    match output["pedido"].as_str() {
        Some("troca_de_faixa") if output["faixa_mudou"].as_bool() == Some(true) => Ok(()),
        Some("troca_de_faixa") => Err(ToolError::VerificationFailed("a faixa não mudou".into())),
        Some(esperado) if output["status"].as_str() == Some(esperado) => Ok(()),
        _ => Err(ToolError::VerificationFailed(
            "o player não confirmou o estado pedido".into(),
        )),
    }
}

fn audit_summary(_args: &Value, result: &Result<Value, ToolError>) -> String {
    let Ok(out) = result else {
        return "falha no controle de mídia".to_string();
    };
    match out["acao"].as_str() {
        Some("tocar") => "tocar no player ativo".to_string(),
        Some("pausar") => "pausar no player ativo".to_string(),
        Some("proxima") => "próxima faixa no player ativo".to_string(),
        Some("anterior") => "faixa anterior no player ativo".to_string(),
        _ => "controle de mídia".to_string(),
    }
}

#[cfg(all(windows, not(test)))]
mod win {
    use std::time::Duration;

    use windows::core::Interface;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    use super::{Estado, Leitura, MidiaBackend, Op, Sessao};

    /// A sessão que o Windows marca como atual.
    struct Atual(Session);

    /// FNV-1a, como o id de faixa de `spotify.rs`: só para comparar, nunca sai.
    fn faixa_id(titulo: &str, artista: &str, album: &str) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in [artista, album, titulo].join("\u{1f}").bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    impl Sessao for Atual {
        fn leitura(&self) -> Option<Leitura> {
            let status = self.0.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?;
            let estado = if status == Status::Playing {
                Estado::Tocando
            } else if status == Status::Paused {
                Estado::Pausado
            } else {
                Estado::Outro
            };
            let faixa = self
                .0
                .TryGetMediaPropertiesAsync()
                .and_then(|op| op.get())
                .ok()
                .and_then(|p| {
                    let titulo = p.Title().map(|s| s.to_string_lossy()).unwrap_or_default();
                    if titulo.trim().is_empty() {
                        return None;
                    }
                    let artista = p.Artist().map(|s| s.to_string_lossy()).unwrap_or_default();
                    let album = p.AlbumTitle().map(|s| s.to_string_lossy()).unwrap_or_default();
                    Some(faixa_id(&titulo, &artista, &album))
                });
            Some(Leitura { estado, faixa })
        }

        fn controle(&self, op: Op) -> bool {
            // Play e pause separados, não o toggle: o comando não inverte um estado que já era o pedido.
            let feito = match op {
                Op::Tocar => self.0.TryPlayAsync(),
                Op::Pausar => self.0.TryPauseAsync(),
                Op::Proxima => self.0.TrySkipNextAsync(),
                Op::Anterior => self.0.TrySkipPreviousAsync(),
            };
            feito.and_then(|op| op.get()).unwrap_or(false)
        }
    }

    pub struct Os;

    impl MidiaBackend for Os {
        fn atual(&self) -> Option<Box<dyn Sessao>> {
            let sessao = Manager::RequestAsync()
                .and_then(|op| op.get())
                .ok()?
                .GetCurrentSession()
                .ok()?;
            // Sem sessão atual, o Windows devolve um ponteiro nulo, não erro.
            (!sessao.as_raw().is_null()).then(|| Box::new(Atual(sessao)) as Box<dyn Sessao>)
        }

        fn pausa(&self) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::mochi::registry::{validate_schema, ToolRegistry};

    /// Sessão falsa. `aceita: false` recusa o comando; `aplica: false` aceita e não muda nada.
    #[derive(Clone)]
    struct Player {
        estado: Rc<Cell<Estado>>,
        faixa: Rc<Cell<Option<u64>>>,
        aceita: bool,
        aplica: bool,
        chamadas: Rc<Cell<u32>>,
    }

    impl Player {
        fn com(estado: Estado, faixa: Option<u64>) -> Self {
            Self {
                estado: Rc::new(Cell::new(estado)),
                faixa: Rc::new(Cell::new(faixa)),
                aceita: true,
                aplica: true,
                chamadas: Rc::new(Cell::new(0)),
            }
        }
    }

    impl Sessao for Player {
        fn leitura(&self) -> Option<Leitura> {
            Some(Leitura {
                estado: self.estado.get(),
                faixa: self.faixa.get(),
            })
        }

        fn controle(&self, op: Op) -> bool {
            self.chamadas.set(self.chamadas.get() + 1);
            if !self.aceita {
                return false;
            }
            if self.aplica {
                match op {
                    Op::Tocar => self.estado.set(Estado::Tocando),
                    Op::Pausar => self.estado.set(Estado::Pausado),
                    Op::Proxima | Op::Anterior => self.faixa.set(Some(self.faixa.get().unwrap_or(0) + 1)),
                }
            }
            true
        }
    }

    struct Falso {
        sessoes: Vec<Player>,
        atual: Option<usize>,
        pausas: Cell<u32>,
    }

    fn falso(sessoes: Vec<Player>) -> Falso {
        Falso {
            sessoes,
            atual: Some(0),
            pausas: Cell::new(0),
        }
    }

    impl MidiaBackend for Falso {
        fn atual(&self) -> Option<Box<dyn Sessao>> {
            let i = self.atual?;
            self.sessoes.get(i).map(|p| Box::new(p.clone()) as Box<dyn Sessao>)
        }

        fn pausa(&self) {
            self.pausas.set(self.pausas.get() + 1);
        }
    }

    #[test]
    fn schema_accepts_the_four_actions_and_refuses_the_rest() {
        let s = tool().input_schema;
        for acao in ["tocar", "pausar", "proxima", "anterior"] {
            assert!(validate_schema(&s, &json!({ "acao": acao })).is_ok(), "{acao}");
        }
        for bad in [
            json!({}),
            json!({ "acao": "shell" }),
            json!({ "acao": 1 }),
            json!({ "acao": "tocar", "cmd": "whoami" }),
        ] {
            assert!(matches!(validate_schema(&s, &bad), Err(ToolError::InvalidInput(_))), "{bad}");
        }
    }

    #[test]
    fn each_action_maps_to_one_operation_and_expected_status() {
        assert_eq!(plano(&json!({ "acao": "tocar" })).unwrap(), Op::Tocar);
        assert_eq!(plano(&json!({ "acao": "pausar" })).unwrap(), Op::Pausar);
        assert_eq!(plano(&json!({ "acao": "proxima" })).unwrap(), Op::Proxima);
        assert_eq!(plano(&json!({ "acao": "anterior" })).unwrap(), Op::Anterior);
        assert!(matches!(plano(&json!({ "acao": "parar" })), Err(ToolError::InvalidInput(_))));
        assert_eq!(pedido(Op::Tocar), "tocando");
        assert_eq!(pedido(Op::Pausar), "pausado");
        assert_eq!(pedido(Op::Proxima), "troca_de_faixa");
        assert_eq!(pedido(Op::Anterior), "troca_de_faixa");
    }

    #[test]
    fn verify_compares_status_or_track_change_without_reading_the_system() {
        let troca = json!({ "acao": "proxima" });
        let igual = json!({ "acao": "proxima", "status": "tocando", "faixa_mudou": false, "pedido": "troca_de_faixa" });
        let nova = json!({ "acao": "proxima", "status": "tocando", "faixa_mudou": true, "pedido": "troca_de_faixa" });
        assert_eq!(
            verify(&troca, &igual),
            Err(ToolError::VerificationFailed("a faixa não mudou".into()))
        );
        assert!(verify(&troca, &nova).is_ok());

        let tocar = json!({ "acao": "tocar" });
        assert!(verify(&tocar, &json!({ "status": "tocando", "faixa_mudou": false, "pedido": "tocando" })).is_ok());
        assert!(matches!(
            verify(&tocar, &json!({ "status": "pausado", "faixa_mudou": false, "pedido": "tocando" })),
            Err(ToolError::VerificationFailed(_))
        ));
    }

    #[test]
    fn no_session_is_prerequisite_missing_with_zero_calls() {
        let player = Player::com(Estado::Tocando, Some(1));
        let sem_atual = Falso {
            sessoes: vec![player.clone()],
            atual: None,
            pausas: Cell::new(0),
        };
        assert_eq!(
            run_with(&sem_atual, Op::Pausar).unwrap_err(),
            ToolError::PrerequisiteMissing(NADA_ABERTO.into())
        );
        assert_eq!(player.chamadas.get(), 0);
    }

    #[test]
    fn only_the_current_session_receives_the_command() {
        let a = Player::com(Estado::Tocando, Some(1));
        let b = Player::com(Estado::Pausado, Some(2));
        let duas = Falso {
            sessoes: vec![a.clone(), b.clone()],
            atual: Some(1),
            pausas: Cell::new(0),
        };
        let out = run_with(&duas, Op::Tocar).unwrap();
        assert_eq!(out["status"], "tocando");
        assert_eq!(a.chamadas.get(), 0);
        assert_eq!(b.chamadas.get(), 1);
    }

    #[test]
    fn refused_play_is_execution_failed() {
        let mut recusa = Player::com(Estado::Pausado, Some(1));
        recusa.aceita = false;
        assert!(matches!(
            run_with(&falso(vec![recusa]), Op::Tocar),
            Err(ToolError::ExecutionFailed(_))
        ));
    }

    #[test]
    fn a_state_that_does_not_follow_waits_a_bounded_time_without_sleeping() {
        let mut teimoso = Player::com(Estado::Pausado, Some(1));
        teimoso.aplica = false;
        let f = falso(vec![teimoso]);
        let out = run_with(&f, Op::Tocar).unwrap();
        assert_eq!(out["status"], "pausado");
        assert_eq!(f.pausas.get(), PASSOS);
        assert!(verify(&json!({ "acao": "tocar" }), &out).is_err());
    }

    #[test]
    fn a_stopped_player_reports_outro_and_fails_play() {
        let mut parado = Player::com(Estado::Outro, None);
        parado.aplica = false;
        let out = run_with(&falso(vec![parado]), Op::Tocar).unwrap();
        assert_eq!(out["status"], "outro");
        assert!(verify(&json!({ "acao": "tocar" }), &out).is_err());
    }

    #[test]
    fn a_state_that_already_matches_does_not_wait() {
        let f = falso(vec![Player::com(Estado::Tocando, Some(1))]);
        let out = run_with(&f, Op::Tocar).unwrap();
        assert_eq!(out["status"], "tocando");
        assert_eq!(f.pausas.get(), 0);
    }

    #[test]
    fn next_track_that_never_changes_fails_verification() {
        let mut parado = Player::com(Estado::Tocando, Some(1));
        parado.aplica = false;
        let f = falso(vec![parado]);
        let out = run_with(&f, Op::Proxima).unwrap();
        assert_eq!(out["faixa_mudou"], false);
        assert_eq!(f.pausas.get(), PASSOS);
        assert_eq!(
            verify(&json!({ "acao": "proxima" }), &out),
            Err(ToolError::VerificationFailed("a faixa não mudou".into()))
        );
    }

    #[test]
    fn next_track_that_changes_verifies_and_exposes_no_track_data() {
        let f = falso(vec![Player::com(Estado::Tocando, Some(1))]);
        let out = run_with(&f, Op::Anterior).unwrap();
        assert_eq!(out["faixa_mudou"], true);
        assert!(verify(&json!({ "acao": "anterior" }), &out).is_ok());
        let mut chaves: Vec<&str> = out.as_object().unwrap().keys().map(String::as_str).collect();
        chaves.sort();
        assert_eq!(chaves, vec!["acao", "faixa_mudou", "pedido", "status"]);
    }

    #[test]
    fn esp32_media_with_toggles_play_pause() {
        let f = falso(vec![Player::com(Estado::Tocando, Some(1))]);
        assert_eq!(esp32_media_with(&f, "play_pause").unwrap(), false);
        assert_eq!(esp32_media_with(&f, "play_pause").unwrap(), true);
        assert_eq!(esp32_media_with(&f, "next").unwrap(), true);
        assert_eq!(esp32_media_with(&f, "prev").unwrap(), true);
    }

    #[test]
    fn the_real_backend_is_never_reached_in_tests() {
        assert_eq!(
            run_os(Op::Tocar).unwrap_err(),
            ToolError::PrerequisiteMissing(NADA_ABERTO.into())
        );
        assert!(!session_present());
    }

    #[test]
    fn availability_needs_a_media_session() {
        let registry = ToolRegistry::with_default_tools();
        assert!(registry.is_enabled("midia"));
        assert_eq!(
            registry.get_if_available("midia", &ToolContext::new()).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_PLAYER.into())
        );
        let ctx = ToolContext::new().with_capability("media_session");
        assert!(registry.get_if_available("midia", &ctx).is_ok());
    }

    #[test]
    fn dry_run_describes_the_action_and_refuses_unknown_ones() {
        assert_eq!(
            dry_run(&json!({ "acao": "pausar" })).unwrap(),
            "Pausar o player de mídia ativo do Windows."
        );
        assert!(matches!(dry_run(&json!({ "acao": "parar" })), Err(ToolError::InvalidInput(_))));
    }

    #[test]
    fn audit_summary_names_the_action_without_track_or_app() {
        let ok = |v: Value| -> Result<Value, ToolError> { Ok(v) };
        assert_eq!(
            audit_summary(&json!({}), &ok(json!({ "acao": "tocar" }))),
            "tocar no player ativo"
        );
        assert_eq!(
            audit_summary(&json!({}), &ok(json!({ "acao": "proxima" }))),
            "próxima faixa no player ativo"
        );
        assert_eq!(
            audit_summary(&json!({}), &Err(ToolError::ExecutionFailed("x".into()))),
            "falha no controle de mídia"
        );
    }
}
