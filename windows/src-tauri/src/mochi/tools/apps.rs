//! Ferramenta `apps` (M2.3): lista os aplicativos do Menu Iniciar ou abre um deles pelo nome.
//!
//! `nome` só serve para comparar com os atalhos da varredura. O que abre é o `.lnk` encontrado,
//! pela shell (verbo `open`), sem linha de comando montada a partir do pedido.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::mochi::registry::{
    Availability, ExecutionContext, RiskLevel, ToolContext, ToolDefinition, ToolError, ToolFuture,
};
use crate::mochi::sanitize::sanitize_external_text;
use crate::voice::wake::normalize;

const SEM_MENU: &str = "nenhuma pasta do Menu Iniciar encontrada";
const NAO_ENCONTRADO: &str = "aplicativo não encontrado no Menu Iniciar";
const NAO_ABRIU: &str = "o Windows não abriu o atalho";
const NAO_APARECEU: &str = "o aplicativo não apareceu";
const NAO_CONFIRMADO: &str = "não dá para confirmar que o aplicativo abriu";
const MAX_NOME: usize = 64;
const MAX_LISTA: usize = 200;
const MAX_ESCOLHAS: usize = 5;
/// 10 pausas de 500 ms: a espera de 5 s do processo.
const PASSOS: u32 = 10;

/// Nomes que o Menu Iniciar não usa, já normalizados. Lista fixa e curta (spec, seção 9).
const ALIASES: [(&str, &str); 4] = [
    ("vs code", "visual studio code"),
    ("vscode", "visual studio code"),
    ("chrome", "google chrome"),
    ("edge", "microsoft edge"),
];

/// Operação já validada: é o que `run_with` leva ao sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    Listar,
    Abrir(String),
}

/// Um atalho `.lnk` do Menu Iniciar: o nome exibido e o caminho do arquivo.
#[derive(Debug, Clone)]
pub(crate) struct Atalho {
    pub nome: String,
    pub lnk: PathBuf,
}

/// Menu Iniciar e processos. Os testes usam um falso; o real só existe no Windows fora de `cfg(test)`.
pub(crate) trait AppBackend {
    fn atalhos(&self) -> Vec<Atalho>;
    /// Nome do executável que o atalho abre, lido por `IShellLinkW::GetPath`. `None` quando não há um `.exe` a conferir.
    fn alvo(&self, lnk: &Path) -> Option<String>;
    fn abrir(&self, lnk: &Path) -> bool;
    fn ativo(&self, exe: &str) -> bool;
    fn pausa(&self);
}

pub fn tool() -> ToolDefinition {
    ToolDefinition {
        id: "apps",
        description: "Lista os aplicativos do Menu Iniciar ou abre um deles pelo nome.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "acao": { "type": "string", "enum": ["listar", "abrir"] },
                "nome": { "type": "string", "minLength": 1, "maxLength": 64 }
            },
            "required": ["acao"],
            "additionalProperties": false
        }),
        risk: RiskLevel::Low,
        timeout: Duration::from_secs(10),
        check_availability,
        dry_run,
        execute,
        verify,
        audit_summary,
    }
}

fn check_availability(ctx: &ToolContext) -> Availability {
    if ctx.has_capability("start_menu") {
        Availability::Available
    } else {
        Availability::Unavailable {
            reason: SEM_MENU.to_string(),
        }
    }
}

fn plano(args: &Value) -> Result<Op, ToolError> {
    let acao = args.get("acao").and_then(Value::as_str).unwrap_or_default();
    let nome = args.get("nome").and_then(Value::as_str);
    match (acao, nome) {
        // listar só lê: um nome solto é ignorado, para o modelo não repetir o erro até estourar os passos
        ("listar", _) => Ok(Op::Listar),
        ("abrir", Some(nome)) => Ok(Op::Abrir(nome.to_string())),
        ("abrir", None) => Err(ToolError::InvalidInput("abrir exige nome".into())),
        _ => Err(ToolError::InvalidInput("ação de aplicativos desconhecida".into())),
    }
}

fn dry_run(args: &Value) -> Result<String, ToolError> {
    Ok(match plano(args)? {
        Op::Listar => "Listar os aplicativos do Menu Iniciar.".to_string(),
        Op::Abrir(nome) => format!("Abrir o aplicativo {}.", sanitize_external_text(&nome, MAX_NOME)),
    })
}

pub(crate) fn run_with(backend: &dyn AppBackend, op: Op) -> Result<Value, ToolError> {
    match op {
        Op::Listar => Ok(listar(atalhos(backend))),
        Op::Abrir(nome) => abrir(backend, &nome),
    }
}

/// Os atalhos sem desinstaladores: a ferramenta é de risco baixo e abre sem clique, então nunca
/// pode rodar um "Desinstalar X". O Windows também os esconde de "Todos os apps".
fn atalhos(backend: &dyn AppBackend) -> Vec<Atalho> {
    backend
        .atalhos()
        .into_iter()
        .filter(|a| {
            let nome = normalize(&a.nome);
            !(nome.contains("uninstall") || nome.contains("desinstal"))
        })
        .collect()
}

fn listar(mut atalhos: Vec<Atalho>) -> Value {
    // Um app instalado para o usuário e para todos aparece em duas pastas: conta uma vez.
    atalhos.sort_by_key(|a| normalize(&a.nome));
    atalhos.dedup_by_key(|a| normalize(&a.nome));
    let apps: Vec<String> = atalhos
        .iter()
        .take(MAX_LISTA)
        .map(|a| sanitize_external_text(&a.nome, MAX_NOME))
        .collect();
    json!({ "apps": apps })
}

fn abrir(backend: &dyn AppBackend, nome: &str) -> Result<Value, ToolError> {
    let pedido = alias(&normalize(nome));
    let mut casados = casar(atalhos(backend), &pedido);
    match casados.len() {
        0 => Err(ToolError::ExecutionFailed(NAO_ENCONTRADO.into())),
        1 => {
            let atalho = casados.remove(0);
            // Lido antes de abrir: sem ele não há processo para conferir depois.
            let exe = backend.alvo(&atalho.lnk);
            if !backend.abrir(&atalho.lnk) {
                return Err(ToolError::ExecutionFailed(NAO_ABRIU.into()));
            }
            let observado = exe.map(|exe| esperar(backend, &exe));
            Ok(json!({
                "aberto": sanitize_external_text(&atalho.nome, MAX_NOME),
                "observado": observado,
            }))
        }
        // Empate no melhor nível: nada abre, e o chat pede um nome mais preciso.
        _ => Ok(json!({
            "escolher": casados
                .iter()
                .take(MAX_ESCOLHAS)
                .map(|a| sanitize_external_text(&a.nome, MAX_NOME))
                .collect::<Vec<_>>(),
        })),
    }
}

fn alias(pedido: &str) -> String {
    ALIASES
        .iter()
        .find(|(de, _)| *de == pedido)
        .map_or_else(|| pedido.to_string(), |(_, para)| para.to_string())
}

/// Os atalhos do melhor nível (exato, depois prefixo, depois contém), um por nome, em ordem alfabética.
fn casar(atalhos: Vec<Atalho>, pedido: &str) -> Vec<Atalho> {
    // Pedido vazio casaria com tudo, pois todo texto contém "".
    if pedido.is_empty() {
        return Vec::new();
    }
    let mut pontuados: Vec<(u8, String, Atalho)> = atalhos
        .into_iter()
        .filter_map(|a| {
            let nome = normalize(&a.nome);
            let nivel = if nome == pedido {
                0
            } else if nome.starts_with(pedido) {
                1
            } else if nome.contains(pedido) {
                2
            } else {
                return None;
            };
            Some((nivel, nome, a))
        })
        .collect();
    let Some(melhor) = pontuados.iter().map(|p| p.0).min() else {
        return Vec::new();
    };
    pontuados.retain(|p| p.0 == melhor);
    pontuados.sort_by(|a, b| a.1.cmp(&b.1));
    pontuados.dedup_by(|a, b| a.1 == b.1);
    pontuados.into_iter().map(|p| p.2).collect()
}

/// Até 5 s de espera. Um processo que já estava aberto conta de primeira.
fn esperar(backend: &dyn AppBackend, exe: &str) -> bool {
    let mut passos = 0;
    while !backend.ativo(exe) {
        if passos == PASSOS {
            return false;
        }
        backend.pausa();
        passos += 1;
    }
    true
}

fn execute(args: Value, ctx: ExecutionContext) -> ToolFuture {
    Box::pin(async move {
        let op = plano(&args)?;
        if ctx.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        // COM e a espera ficam na thread do bloqueio e não atravessam `await`.
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
    run_with(&SemApps, op)
}

/// Fora do Windows e nos testes: o Menu Iniciar está vazio, e nada é aberto.
#[cfg(not(all(windows, not(test))))]
struct SemApps;

#[cfg(not(all(windows, not(test))))]
impl AppBackend for SemApps {
    fn atalhos(&self) -> Vec<Atalho> {
        Vec::new()
    }

    fn alvo(&self, _lnk: &Path) -> Option<String> {
        None
    }

    fn abrir(&self, _lnk: &Path) -> bool {
        false
    }

    fn ativo(&self, _exe: &str) -> bool {
        false
    }

    fn pausa(&self) {}
}

/// Há pasta do Menu Iniciar? Sondagem para `host::context`; nunca roda nos testes.
#[cfg(all(windows, not(test)))]
pub(crate) fn start_menu_present() -> bool {
    crate::voice::start_menu_roots().iter().any(|p| p.is_dir())
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn start_menu_present() -> bool {
    false
}

/// Confere o que `execute` observou. Listagem e escolha não abriram nada, então passam.
fn verify(_args: &Value, output: &Value) -> Result<(), ToolError> {
    if output.get("apps").is_some() || output.get("escolher").is_some() {
        return Ok(());
    }
    match output["observado"].as_bool() {
        Some(true) => Ok(()),
        Some(false) => Err(ToolError::VerificationFailed(NAO_APARECEU.into())),
        None => Err(ToolError::VerificationFailed(NAO_CONFIRMADO.into())),
    }
}

fn audit_summary(_args: &Value, result: &Result<Value, ToolError>) -> String {
    let Ok(out) = result else {
        return "falha ao listar ou abrir aplicativos".to_string();
    };
    if let Some(apps) = out["apps"].as_array() {
        return format!("listagem de aplicativos ({} itens)", apps.len());
    }
    if let Some(opcoes) = out["escolher"].as_array() {
        return format!("escolha de aplicativo ({} opções)", opcoes.len());
    }
    let nome: String = out["aberto"].as_str().unwrap_or_default().chars().take(60).collect();
    format!("abrir aplicativo: {nome}")
}

#[cfg(all(windows, not(test)))]
mod win {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    use super::{AppBackend, Atalho};
    use crate::platform::{process_running, shell_open_uri};

    pub struct Os;

    impl AppBackend for Os {
        fn atalhos(&self) -> Vec<Atalho> {
            crate::voice::start_menu_shortcuts()
                .into_iter()
                .filter_map(|lnk| {
                    let nome = lnk.file_stem()?.to_string_lossy().into_owned();
                    Some(Atalho { nome, lnk })
                })
                .collect()
        }

        fn alvo(&self, lnk: &Path) -> Option<String> {
            unsafe {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
                let persist: IPersistFile = link.cast().ok()?;
                persist.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
                let mut alvo = vec![0u16; 32768];
                link.GetPath(&mut alvo, std::ptr::null_mut(), 0).ok()?;
                let fim = alvo.iter().position(|c| *c == 0)?;
                let caminho = PathBuf::from(String::from_utf16_lossy(&alvo[..fim]));
                let nome = caminho.file_name()?.to_string_lossy().into_owned();
                let exe = caminho.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe"));
                // Apps da Store têm `explorer.exe shell:AppsFolder\...` como alvo: o explorer está sempre aberto.
                (exe && !nome.eq_ignore_ascii_case("explorer.exe")).then_some(nome)
            }
        }

        fn abrir(&self, lnk: &Path) -> bool {
            let caminho: Vec<u16> = lnk.as_os_str().encode_wide().chain(once(0)).collect();
            shell_open_uri(PCWSTR(caminho.as_ptr()))
        }

        fn ativo(&self, exe: &str) -> bool {
            process_running(exe)
        }

        fn pausa(&self) {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::mochi::registry::{validate_schema, ToolRegistry};

    /// Menu falso com caminhos sintéticos. `aparece_apos`: quantas pausas até o processo aparecer;
    /// `Some(0)` é "já estava aberto", `None` é "nunca aparece".
    struct Falso {
        atalhos: Vec<Atalho>,
        exe: Option<String>,
        aparece_apos: Option<u32>,
        abertos: RefCell<Vec<PathBuf>>,
        pausas: Cell<u32>,
    }

    fn atalho(nome: &str) -> Atalho {
        Atalho {
            nome: nome.into(),
            lnk: PathBuf::from(format!("C:\\Sintetico\\Menu\\{nome}.lnk")),
        }
    }

    fn falso(nomes: &[&str]) -> Falso {
        Falso {
            atalhos: nomes.iter().map(|n| atalho(n)).collect(),
            exe: Some("sintetico.exe".into()),
            aparece_apos: Some(1),
            abertos: RefCell::new(Vec::new()),
            pausas: Cell::new(0),
        }
    }

    impl AppBackend for Falso {
        fn atalhos(&self) -> Vec<Atalho> {
            self.atalhos.clone()
        }

        fn alvo(&self, _lnk: &Path) -> Option<String> {
            self.exe.clone()
        }

        fn abrir(&self, lnk: &Path) -> bool {
            self.abertos.borrow_mut().push(lnk.to_path_buf());
            true
        }

        fn ativo(&self, _exe: &str) -> bool {
            self.aparece_apos.is_some_and(|n| self.pausas.get() >= n)
        }

        fn pausa(&self) {
            self.pausas.set(self.pausas.get() + 1);
        }
    }

    fn abre(nome: &str) -> Op {
        Op::Abrir(nome.into())
    }

    #[test]
    fn a_text_with_separators_matches_nothing_and_opens_nothing() {
        let falso_menu = falso(&["Calculator", "Notepad"]);
        assert_eq!(
            run_with(&falso_menu, abre("calc; del x & notepad")),
            Err(ToolError::ExecutionFailed(NAO_ENCONTRADO.into()))
        );
        assert!(falso_menu.abertos.borrow().is_empty());
    }

    #[test]
    fn an_exact_name_beats_prefix_and_contains() {
        let menu = falso(&["Code Helper", "Visual Studio Code", "Code"]);
        let out = run_with(&menu, abre("code")).unwrap();
        assert_eq!(out["aberto"], "Code");
        assert_eq!(*menu.abertos.borrow(), vec![PathBuf::from("C:\\Sintetico\\Menu\\Code.lnk")]);
    }

    #[test]
    fn a_tie_at_the_best_level_asks_and_opens_nothing() {
        let menu = falso(&["Calendar", "Notepad", "Calculator"]);
        let out = run_with(&menu, abre("cal")).unwrap();
        assert_eq!(out["escolher"], json!(["Calculator", "Calendar"]));
        assert!(menu.abertos.borrow().is_empty());
        assert_eq!(menu.pausas.get(), 0);
    }

    #[test]
    fn the_same_app_in_two_folders_is_not_a_tie() {
        let menu = falso(&["Discord", "Discord"]);
        let out = run_with(&menu, abre("discord")).unwrap();
        assert_eq!(out["aberto"], "Discord");
        assert_eq!(menu.abertos.borrow().len(), 1);
    }

    #[test]
    fn a_missing_app_is_an_error_and_nothing_opens() {
        let menu = falso(&["Notepad"]);
        assert_eq!(
            run_with(&menu, abre("xyzzy")),
            Err(ToolError::ExecutionFailed(NAO_ENCONTRADO.into()))
        );
        assert!(menu.abertos.borrow().is_empty());
    }

    #[test]
    fn an_empty_name_matches_nothing() {
        let menu = falso(&["Notepad"]);
        assert!(run_with(&menu, abre("  ")).is_err());
        assert!(menu.abertos.borrow().is_empty());
    }

    #[test]
    fn vs_code_is_an_alias_of_visual_studio_code() {
        let menu = falso(&["Visual Studio Code"]);
        let out = run_with(&menu, abre("VS Code")).unwrap();
        assert_eq!(out["aberto"], "Visual Studio Code");
        assert_eq!(out["observado"], true);
    }

    #[test]
    fn the_process_is_awaited_up_to_its_limit() {
        let mut lento = falso(&["Notepad"]);
        lento.aparece_apos = Some(3);
        let out = run_with(&lento, abre("notepad")).unwrap();
        assert_eq!(out["observado"], true);
        assert_eq!(lento.pausas.get(), 3);

        let mut nunca = falso(&["Notepad"]);
        nunca.aparece_apos = None;
        let out = run_with(&nunca, abre("notepad")).unwrap();
        assert_eq!(out["observado"], false);
        assert_eq!(nunca.pausas.get(), PASSOS);
        assert_eq!(
            verify(&json!({}), &out),
            Err(ToolError::VerificationFailed(NAO_APARECEU.into()))
        );
    }

    #[test]
    fn an_app_already_open_counts_as_open_without_waiting() {
        let mut aberto = falso(&["Notepad"]);
        aberto.aparece_apos = Some(0);
        let out = run_with(&aberto, abre("notepad")).unwrap();
        assert_eq!(out["observado"], true);
        assert_eq!(aberto.pausas.get(), 0);
    }

    #[test]
    fn a_shortcut_without_a_checkable_target_is_not_claimed_as_open() {
        let mut sem_alvo = falso(&["Loja"]);
        sem_alvo.exe = None;
        let out = run_with(&sem_alvo, abre("loja")).unwrap();
        assert!(out["observado"].is_null());
        assert_eq!(sem_alvo.pausas.get(), 0);
        assert_eq!(
            verify(&json!({}), &out),
            Err(ToolError::VerificationFailed(NAO_CONFIRMADO.into()))
        );
    }

    #[test]
    fn uninstallers_are_never_listed_nor_opened() {
        let menu = falso(&["Uninstall Foo", "Desinstalar Foo", "Foo"]);
        assert_eq!(run_with(&menu, Op::Listar).unwrap()["apps"], json!(["Foo"]));
        assert!(run_with(&menu, abre("uninstall foo")).is_err());
        assert!(run_with(&menu, abre("desinstalar")).is_err());
        assert!(menu.abertos.borrow().is_empty());
    }

    #[test]
    fn listing_is_sorted_once_per_name() {
        let menu = falso(&["Zeta", "alpha", "Alpha"]);
        let out = run_with(&menu, Op::Listar).unwrap();
        assert_eq!(out["apps"], json!(["alpha", "Zeta"]));
    }

    #[test]
    fn names_are_sanitized_and_capped() {
        let mut menu = falso(&["Foo <b>\nBar"]);
        menu.atalhos.push(atalho(&"x".repeat(100)));
        let out = run_with(&menu, Op::Listar).unwrap();
        let apps = out["apps"].as_array().unwrap();
        assert!(apps.contains(&json!("Foo &lt;b&gt; Bar")));
        assert!(apps.iter().all(|n| n.as_str().unwrap().chars().count() <= 64));
        assert!(!apps.iter().any(|n| n.as_str().unwrap().contains('\n')));
    }

    #[test]
    fn verify_accepts_listings_and_choices_and_confirmed_opens() {
        assert!(verify(&json!({}), &json!({"apps": []})).is_ok());
        assert!(verify(&json!({}), &json!({"escolher": ["A", "B"]})).is_ok());
        assert!(verify(&json!({}), &json!({"aberto": "A", "observado": true})).is_ok());
        assert_eq!(
            verify(&json!({}), &json!({"aberto": "A", "observado": false})),
            Err(ToolError::VerificationFailed(NAO_APARECEU.into()))
        );
    }

    #[test]
    fn audit_summary_never_names_the_shortcut_path() {
        let menu = falso(&["Visual Studio Code"]);
        let out = run_with(&menu, abre("vs code")).unwrap();
        let resumo = audit_summary(&json!({}), &Ok(out));
        assert_eq!(resumo, "abrir aplicativo: Visual Studio Code");
        assert!(!resumo.contains("Sintetico"));

        let listagem = run_with(&menu, Op::Listar).unwrap();
        assert_eq!(audit_summary(&json!({}), &Ok(listagem)), "listagem de aplicativos (1 itens)");
        assert_eq!(
            audit_summary(&json!({}), &Err(ToolError::ExecutionFailed(NAO_ENCONTRADO.into()))),
            "falha ao listar ou abrir aplicativos"
        );
    }

    #[test]
    fn schema_and_dry_run_refuse_wrong_combinations() {
        let s = tool().input_schema;
        for ok in [json!({"acao": "listar"}), json!({"acao": "abrir", "nome": "Notepad"})] {
            assert!(validate_schema(&s, &ok).is_ok(), "{ok}");
        }
        for bad in [
            json!({"acao": "apagar"}),
            json!({"acao": "abrir", "nome": ""}),
            json!({"acao": "abrir", "nome": "a".repeat(65)}),
            json!({"acao": "abrir", "cmd": "cmd"}),
            json!({}),
        ] {
            assert!(matches!(validate_schema(&s, &bad), Err(ToolError::InvalidInput(_))), "{bad}");
        }
        assert!(dry_run(&json!({"acao": "listar", "nome": "Notepad"})).is_ok());
        assert!(dry_run(&json!({"acao": "abrir"})).is_err());
        assert_eq!(dry_run(&json!({"acao": "listar"})).unwrap(), "Listar os aplicativos do Menu Iniciar.");
        assert_eq!(
            dry_run(&json!({"acao": "abrir", "nome": "Notepad"})).unwrap(),
            "Abrir o aplicativo Notepad."
        );
    }

    #[test]
    fn test_build_never_reaches_the_real_menu() {
        assert_eq!(run_os(Op::Listar).unwrap(), json!({"apps": []}));
        assert_eq!(
            run_os(abre("notepad")).unwrap_err(),
            ToolError::ExecutionFailed(NAO_ENCONTRADO.into())
        );
        assert!(!start_menu_present());
    }

    #[test]
    fn availability_needs_the_start_menu_capability() {
        let registry = ToolRegistry::with_default_tools();
        assert!(registry.is_enabled("apps"));
        assert_eq!(
            registry.get_if_available("apps", &ToolContext::new()).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_MENU.into())
        );
        let ctx = ToolContext::new().with_capability("start_menu");
        assert!(registry.get_if_available("apps", &ctx).is_ok());
    }
}
