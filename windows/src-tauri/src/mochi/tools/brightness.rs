//! Ferramenta `brilho` (M2.2): define o brilho da tela interna (WMI) ou dos monitores externos que
//! respondem por DDC/CI, de 0 a 100.
//!
//! `nivel` é inteiro validado e segue como número. Nenhum texto do pedido chega ao sistema.

use std::time::Duration;

use serde_json::{json, Value};

use crate::mochi::registry::{
    Availability, ExecutionContext, RiskLevel, ToolContext, ToolDefinition, ToolError, ToolFuture,
};

const SEM_TELA: &str = "nenhuma tela com brilho controlável";
const MONITOR_SEM_RESPOSTA: &str = "este monitor não responde por DDC/CI; brilho indisponível neste monitor";
const NAO_CONFIRMADO: &str = "o brilho não pôde ser confirmado";
/// Drivers atualizam o brilho com atraso: até 1,5 s de releituras, em passos de 250 ms.
const PASSOS: u32 = 6;
const TOLERANCIA: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fonte {
    TelaInterna,
    MonitorExterno,
}

impl Fonte {
    fn texto(self) -> &'static str {
        match self {
            Self::TelaInterna => "tela_interna",
            Self::MonitorExterno => "monitor_externo",
        }
    }
}

/// Uma tela com brilho controlável. `definir` devolve `Ok(false)` quando a tela não responde.
pub(crate) trait Tela {
    fn fonte(&self) -> Fonte;
    fn definir(&self, nivel: u8) -> Result<bool, String>;
    /// `None` quando a leitura não vem.
    fn ler(&self) -> Option<u8>;
}

/// As telas do computador e a pausa entre leituras. Os testes usam um falso.
pub(crate) trait BrilhoBackend {
    /// A tela interna sozinha, se existir; senão os monitores DDC/CI, respondendo ou não.
    fn telas(&self) -> Result<Vec<Box<dyn Tela>>, String>;
    /// Espera entre duas leituras. Nos testes não dorme.
    fn pausa(&self);
}

pub fn tool() -> ToolDefinition {
    ToolDefinition {
        id: "brilho",
        description: "Define o brilho da tela interna do notebook, ou do monitor externo que responde por DDC/CI, de 0 a 100.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "nivel": { "type": "integer", "minimum": 0, "maximum": 100 }
            },
            "required": ["nivel"],
            "additionalProperties": false
        }),
        risk: RiskLevel::Low,
        timeout: Duration::from_secs(8),
        check_availability,
        dry_run,
        execute,
        verify,
        audit_summary,
    }
}

fn check_availability(ctx: &ToolContext) -> Availability {
    if ctx.has_capability("internal_display") || ctx.has_capability("ddc_display") {
        Availability::Available
    } else {
        Availability::Unavailable {
            reason: SEM_TELA.to_string(),
        }
    }
}

fn plano(args: &Value) -> Result<u8, ToolError> {
    args.get("nivel")
        .and_then(Value::as_u64)
        .filter(|n| *n <= 100)
        .map(|n| n as u8)
        .ok_or_else(|| ToolError::InvalidInput("brilho exige nivel de 0 a 100".into()))
}

fn dry_run(args: &Value) -> Result<String, ToolError> {
    Ok(format!("Definir o brilho da tela em {}%.", plano(args)?))
}

/// A leitura mais distante do pedido: com mais de um monitor, a conferência vale para todos.
fn pior_leitura(telas: &[Box<dyn Tela>], nivel: u8) -> Option<u8> {
    let mut pior: Option<u8> = None;
    for tela in telas {
        let lido = tela.ler()?;
        if pior.is_none_or(|p| lido.abs_diff(nivel) > p.abs_diff(nivel)) {
            pior = Some(lido);
        }
    }
    pior
}

/// Age na tela interna, ou nos monitores que respondem; relê até confirmar ou esgotar a espera.
pub(crate) fn run_with(backend: &dyn BrilhoBackend, nivel: u8) -> Result<Value, ToolError> {
    let telas = backend.telas().map_err(ToolError::ExecutionFailed)?;
    if telas.is_empty() {
        return Err(ToolError::PrerequisiteMissing(SEM_TELA.into()));
    }
    let (internas, externas): (Vec<_>, Vec<_>) = telas.into_iter().partition(|t| t.fonte() == Fonte::TelaInterna);
    let alvos = if internas.is_empty() { externas } else { internas };
    let Some(fonte) = alvos.first().map(|t| t.fonte()) else {
        return Err(ToolError::PrerequisiteMissing(SEM_TELA.into()));
    };

    let mut aceitas = Vec::new();
    for tela in alvos {
        if tela.definir(nivel).map_err(ToolError::ExecutionFailed)? {
            aceitas.push(tela);
        }
    }
    if aceitas.is_empty() {
        return Err(match fonte {
            Fonte::MonitorExterno => ToolError::PrerequisiteMissing(MONITOR_SEM_RESPOSTA.into()),
            Fonte::TelaInterna => ToolError::ExecutionFailed("a tela interna não aceitou o brilho".into()),
        });
    }

    let mut observado = pior_leitura(&aceitas, nivel);
    let mut passos = 0;
    while !observado.is_some_and(|v| v.abs_diff(nivel) <= TOLERANCIA) && passos < PASSOS {
        backend.pausa();
        passos += 1;
        observado = pior_leitura(&aceitas, nivel);
    }
    Ok(json!({
        "nivel": nivel,
        "observado": observado,
        "fonte": fonte.texto(),
        "pedido": { "nivel": nivel },
    }))
}

fn execute(args: Value, ctx: ExecutionContext) -> ToolFuture {
    Box::pin(async move {
        let nivel = plano(&args)?;
        if ctx.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        // COM e WMI nascem e morrem na thread do bloqueio e não atravessam `await`.
        tokio::task::spawn_blocking(move || run_os(nivel))
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
    })
}

#[cfg(all(windows, not(test)))]
fn run_os(nivel: u8) -> Result<Value, ToolError> {
    let _com = crate::mochi::tools::audio::win::Com::init();
    run_with(&win::Os, nivel)
}

#[cfg(not(all(windows, not(test))))]
fn run_os(nivel: u8) -> Result<Value, ToolError> {
    run_with(&SemTela, nivel)
}

/// Fora do Windows e nos testes: não há tela, e nada é tocado.
#[cfg(not(all(windows, not(test))))]
struct SemTela;

#[cfg(not(all(windows, not(test))))]
impl BrilhoBackend for SemTela {
    fn telas(&self) -> Result<Vec<Box<dyn Tela>>, String> {
        Ok(Vec::new())
    }

    fn pausa(&self) {}
}

/// Há tela interna com brilho? Sondagem para `host::context`; nunca roda nos testes.
#[cfg(all(windows, not(test)))]
pub(crate) fn internal_present() -> bool {
    let _com = crate::mochi::tools::audio::win::Com::init();
    win::interna_presente()
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn internal_present() -> bool {
    false
}

/// Algum monitor externo responde ao brilho por DDC/CI? Sondagem lenta, cacheada por `host`.
#[cfg(all(windows, not(test)))]
pub(crate) fn ddc_present() -> bool {
    win::ddc_responde()
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn ddc_present() -> bool {
    false
}

/// O VCP vai de 0 ao máximo que o monitor informa; o pedido é de 0 a 100.
fn vcp_de_percentual(nivel: u8, maximo: u32) -> u32 {
    ((u64::from(nivel) * u64::from(maximo) + 50) / 100) as u32
}

fn percentual_de_vcp(atual: u32, maximo: u32) -> u8 {
    ((u64::from(atual) * 100 + u64::from(maximo) / 2) / u64::from(maximo.max(1))).min(100) as u8
}

fn verify(_args: &Value, output: &Value) -> Result<(), ToolError> {
    let (Some(pedido), Some(observado)) = (output["pedido"]["nivel"].as_i64(), output["observado"].as_i64()) else {
        return Err(ToolError::VerificationFailed(NAO_CONFIRMADO.into()));
    };
    if (observado - pedido).abs() > i64::from(TOLERANCIA) {
        return Err(ToolError::VerificationFailed(format!(
            "o brilho ficou em {observado}%, e não em {pedido}%"
        )));
    }
    Ok(())
}

fn audit_summary(_args: &Value, result: &Result<Value, ToolError>) -> String {
    let Ok(out) = result else {
        return "falha no controle de brilho".to_string();
    };
    let fonte = if out["fonte"] == "monitor_externo" {
        "monitor externo"
    } else {
        "tela interna"
    };
    format!("brilho em {}% ({fonte})", out["nivel"].as_i64().unwrap_or(0))
}

#[cfg(all(windows, not(test)))]
mod win {
    use std::mem::ManuallyDrop;
    use std::time::Duration;

    use windows::core::{w, BSTR, PCWSTR};
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
        GetVCPFeatureAndVCPFeatureReply, SetVCPFeature, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
        RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Variant::{
        VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4, VT_UI1,
    };
    use windows::Win32::System::Wmi::{
        IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
        WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE,
    };

    use super::{percentual_de_vcp, vcp_de_percentual, BrilhoBackend, Fonte, Tela};

    const METODOS: &str = "SELECT * FROM WmiMonitorBrightnessMethods";
    /// VCP 0x10 é o brilho em DDC/CI.
    const VCP_BRILHO: u8 = 0x10;

    fn code(e: windows::core::Error) -> String {
        format!("erro do sistema {:#010x}", e.code().0)
    }

    fn bstr(texto: &str) -> BSTR {
        BSTR::from_wide(&texto.encode_utf16().collect::<Vec<_>>())
    }

    /// Monta o VARIANT inteiro: escrever um campo de união por referência não é permitido.
    fn variante(vt: VARENUM, dados: VARIANT_0_0_0) -> VARIANT {
        VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: dados,
                }),
            },
        }
    }

    fn inteiro(n: i32) -> VARIANT {
        variante(VT_I4, VARIANT_0_0_0 { lVal: n })
    }

    fn byte(n: u8) -> VARIANT {
        variante(VT_UI1, VARIANT_0_0_0 { bVal: n })
    }

    /// WMI em `root\wmi`, com a mesma conexão de `vitals.rs`.
    fn conexao() -> Result<IWbemServices, String> {
        unsafe {
            let localizador = CoCreateInstance::<_, IWbemLocator>(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(code)?;
            let vazio = BSTR::new();
            let servicos = localizador
                .ConnectServer(&bstr("root\\wmi"), &vazio, &vazio, &vazio, 0, &vazio, None)
                .map_err(code)?;
            CoSetProxyBlanket(
                &servicos,
                10,
                0,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .map_err(code)?;
            Ok(servicos)
        }
    }

    /// Primeira linha de uma consulta WQL, ou `None` quando não há nenhuma.
    fn primeira(servicos: &IWbemServices, wql: &str) -> Result<Option<IWbemClassObject>, String> {
        unsafe {
            let enumerador = servicos
                .ExecQuery(
                    &bstr("WQL"),
                    &bstr(wql),
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
                .map_err(code)?;
            let mut linha = [None];
            let mut quantos = 0u32;
            let _ = enumerador.Next(3_000, &mut linha, &mut quantos);
            Ok(if quantos == 0 { None } else { linha[0].take() })
        }
    }

    fn caminho(objeto: &IWbemClassObject) -> Result<BSTR, String> {
        let mut valor = VARIANT::default();
        unsafe { objeto.Get(w!("__PATH"), 0, &mut valor, None, None) }.map_err(code)?;
        BSTR::try_from(&valor).map_err(code)
    }

    /// Tela interna: o brilho vem do WMI, e o método `WmiSetBrightness` é chamado na instância.
    pub struct Interna(IWbemServices);

    impl Tela for Interna {
        fn fonte(&self) -> Fonte {
            Fonte::TelaInterna
        }

        fn definir(&self, nivel: u8) -> Result<bool, String> {
            let Some(instancia) = primeira(&self.0, METODOS)? else {
                return Ok(false);
            };
            // A assinatura do método vem da classe: pedida à instância, o WMI responde 0x8004101e (acesso negado).
            let mut classe = None;
            unsafe {
                self.0
                    .GetObject(&bstr("WmiMonitorBrightnessMethods"), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut classe), None)
            }
            .map_err(code)?;
            let classe = classe.ok_or_else(|| "classe de brilho ausente".to_string())?;
            let mut entrada = None;
            unsafe { classe.GetMethod(w!("WmiSetBrightness"), 0, &mut entrada, std::ptr::null_mut()) }
                .map_err(code)?;
            let entrada = entrada.ok_or_else(|| "método de brilho sem parâmetros".to_string())?;
            unsafe {
                let parametros = entrada.SpawnInstance(0).map_err(code)?;
                parametros.Put(w!("Timeout"), 0, &inteiro(1), 0).map_err(code)?;
                parametros.Put(w!("Brightness"), 0, &byte(nivel), 0).map_err(code)?;
                self.0
                    .ExecMethod(
                        &caminho(&instancia)?,
                        &bstr("WmiSetBrightness"),
                        WBEM_GENERIC_FLAG_TYPE(0),
                        None,
                        &parametros,
                        None,
                        None,
                    )
                    .map_err(code)?;
            }
            Ok(true)
        }

        fn ler(&self) -> Option<u8> {
            let objeto = primeira(&self.0, "SELECT CurrentBrightness FROM WmiMonitorBrightness").ok()??;
            let mut valor = VARIANT::default();
            unsafe {
                objeto.Get(w!("CurrentBrightness"), 0, &mut valor, None, None).ok()?;
                let v = &valor.Anonymous.Anonymous;
                (v.vt.0 == VT_UI1.0).then(|| v.Anonymous.bVal)
            }
        }
    }

    unsafe extern "system" fn recolhe(monitor: HMONITOR, _: HDC, _: *mut RECT, dados: LPARAM) -> windows::core::BOOL {
        unsafe {
            (*(dados.0 as *mut Vec<HMONITOR>)).push(monitor);
        }
        windows::core::BOOL(1)
    }

    /// Um monitor físico por DDC/CI. O handle volta ao sistema quando a tela sai de escopo.
    pub struct Ddc(HANDLE);

    impl Drop for Ddc {
        fn drop(&mut self) {
            let monitor = PHYSICAL_MONITOR { hPhysicalMonitor: self.0, ..Default::default() };
            let _ = unsafe { DestroyPhysicalMonitors(&[monitor]) };
        }
    }

    /// Brilho atual e máximo do monitor. `None` quando ele não responde.
    fn vcp(monitor: HANDLE) -> Option<(u32, u32)> {
        let (mut atual, mut maximo) = (0u32, 0u32);
        let ok = unsafe { GetVCPFeatureAndVCPFeatureReply(monitor, VCP_BRILHO, None, &mut atual, Some(&mut maximo)) } != 0;
        (ok && maximo > 0).then_some((atual, maximo))
    }

    impl Tela for Ddc {
        fn fonte(&self) -> Fonte {
            Fonte::MonitorExterno
        }

        fn definir(&self, nivel: u8) -> Result<bool, String> {
            let Some((_, maximo)) = vcp(self.0) else {
                return Ok(false);
            };
            Ok(unsafe { SetVCPFeature(self.0, VCP_BRILHO, vcp_de_percentual(nivel, maximo)) } != 0)
        }

        fn ler(&self) -> Option<u8> {
            vcp(self.0).map(|(atual, maximo)| percentual_de_vcp(atual, maximo))
        }
    }

    /// Todo monitor físico que o Windows expõe. Os que não respondem recusam em `definir`.
    fn monitores_ddc() -> Vec<Ddc> {
        let mut hmonitores: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(recolhe), LPARAM(&mut hmonitores as *mut _ as isize));
        }
        let mut out = Vec::new();
        for hm in hmonitores {
            let mut quantos = 0u32;
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hm, &mut quantos) }.is_err() {
                continue;
            }
            let mut fisicos: Vec<PHYSICAL_MONITOR> = (0..quantos).map(|_| PHYSICAL_MONITOR::default()).collect();
            if unsafe { GetPhysicalMonitorsFromHMONITOR(hm, &mut fisicos) }.is_err() {
                continue;
            }
            out.extend(fisicos.iter().map(|m| Ddc(m.hPhysicalMonitor)));
        }
        out
    }

    pub struct Os;

    impl BrilhoBackend for Os {
        fn telas(&self) -> Result<Vec<Box<dyn Tela>>, String> {
            // Sem WMI, a tela interna simplesmente não existe, e os monitores externos ainda podem responder.
            let interna = conexao()
                .ok()
                .and_then(|s| matches!(primeira(&s, METODOS), Ok(Some(_))).then_some(Interna(s)));
            Ok(match interna {
                Some(tela) => vec![Box::new(tela) as Box<dyn Tela>],
                None => monitores_ddc().into_iter().map(|m| Box::new(m) as Box<dyn Tela>).collect(),
            })
        }

        fn pausa(&self) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn interna_presente() -> bool {
        conexao().is_ok_and(|s| matches!(primeira(&s, METODOS), Ok(Some(_))))
    }

    pub fn ddc_responde() -> bool {
        monitores_ddc().iter().any(|m| m.ler().is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::mochi::registry::{validate_schema, ToolRegistry};

    /// Tela falsa. `aplica: false` ignora o pedido; `le: false` deixa a leitura ausente.
    #[derive(Clone)]
    struct Painel {
        fonte: Fonte,
        responde: bool,
        aplica: bool,
        le: bool,
        nivel: Rc<Cell<u8>>,
        chamadas: Rc<Cell<u32>>,
    }

    impl Painel {
        fn novo(fonte: Fonte, nivel: u8) -> Self {
            Self {
                fonte,
                responde: true,
                aplica: true,
                le: true,
                nivel: Rc::new(Cell::new(nivel)),
                chamadas: Rc::new(Cell::new(0)),
            }
        }
    }

    impl Tela for Painel {
        fn fonte(&self) -> Fonte {
            self.fonte
        }

        fn definir(&self, nivel: u8) -> Result<bool, String> {
            self.chamadas.set(self.chamadas.get() + 1);
            if self.responde && self.aplica {
                self.nivel.set(nivel);
            }
            Ok(self.responde)
        }

        fn ler(&self) -> Option<u8> {
            self.le.then(|| self.nivel.get())
        }
    }

    struct Falso {
        telas: Vec<Painel>,
        pausas: Cell<u32>,
    }

    fn falso(telas: Vec<Painel>) -> Falso {
        Falso { telas, pausas: Cell::new(0) }
    }

    impl BrilhoBackend for Falso {
        fn telas(&self) -> Result<Vec<Box<dyn Tela>>, String> {
            Ok(self.telas.iter().map(|p| Box::new(p.clone()) as Box<dyn Tela>).collect())
        }

        fn pausa(&self) {
            self.pausas.set(self.pausas.get() + 1);
        }
    }

    #[test]
    fn schema_accepts_0_and_100_and_refuses_the_rest() {
        let s = tool().input_schema;
        for ok in [json!({"nivel": 0}), json!({"nivel": 100})] {
            assert!(validate_schema(&s, &ok).is_ok(), "{ok}");
        }
        for bad in [
            json!({"nivel": -1}),
            json!({"nivel": 101}),
            json!({"nivel": 20.5}),
            json!({"nivel": "sessenta"}),
            json!({"nivel": 20, "cmd": "whoami"}),
            json!({}),
        ] {
            assert!(matches!(validate_schema(&s, &bad), Err(ToolError::InvalidInput(_))), "{bad}");
        }
    }

    #[test]
    fn dry_run_describes_the_level_and_refuses_bad_input() {
        assert_eq!(dry_run(&json!({"nivel": 60})).unwrap(), "Definir o brilho da tela em 60%.");
        assert!(matches!(dry_run(&json!({"nivel": 101})), Err(ToolError::InvalidInput(_))));
        assert!(matches!(dry_run(&json!({})), Err(ToolError::InvalidInput(_))));
    }

    #[test]
    fn the_internal_screen_wins_over_external_monitors() {
        let externo = Painel::novo(Fonte::MonitorExterno, 30);
        let falso = falso(vec![Painel::novo(Fonte::TelaInterna, 30), externo.clone()]);
        let out = run_with(&falso, 60).unwrap();
        assert_eq!(out["fonte"], "tela_interna");
        assert_eq!(out["observado"], 60);
        assert_eq!(externo.chamadas.get(), 0);
        assert_eq!(falso.pausas.get(), 0, "lido de primeira, sem espera");
    }

    #[test]
    fn without_any_screen_nothing_is_invented() {
        assert_eq!(
            run_with(&falso(vec![]), 60).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_TELA.into())
        );
    }

    #[test]
    fn a_monitor_that_does_not_answer_ddc_is_unavailable() {
        let mut mudo = Painel::novo(Fonte::MonitorExterno, 30);
        mudo.responde = false;
        assert_eq!(
            run_with(&falso(vec![mudo.clone()]), 60).unwrap_err(),
            ToolError::PrerequisiteMissing(MONITOR_SEM_RESPOSTA.into())
        );

        let out = run_with(&falso(vec![mudo, Painel::novo(Fonte::MonitorExterno, 30)]), 60).unwrap();
        assert_eq!(out["fonte"], "monitor_externo");
        assert_eq!(out["observado"], 60);
    }

    #[test]
    fn a_reading_that_does_not_follow_fails_after_the_bounded_wait() {
        let mut teimosa = Painel::novo(Fonte::TelaInterna, 30);
        teimosa.aplica = false;
        let falso = falso(vec![teimosa]);
        let out = run_with(&falso, 60).unwrap();
        assert_eq!(out["observado"], 30);
        assert_eq!(falso.pausas.get(), PASSOS);
        assert!(matches!(
            verify(&json!({"nivel": 60}), &out),
            Err(ToolError::VerificationFailed(_))
        ));
    }

    #[test]
    fn an_unreadable_screen_is_never_a_success() {
        let mut calada = Painel::novo(Fonte::TelaInterna, 30);
        calada.le = false;
        let out = run_with(&falso(vec![calada]), 60).unwrap();
        assert!(out["observado"].is_null());
        assert_eq!(
            verify(&json!({"nivel": 60}), &out),
            Err(ToolError::VerificationFailed(NAO_CONFIRMADO.into()))
        );
    }

    #[test]
    fn with_two_monitors_the_farthest_reading_decides() {
        let mut teimoso = Painel::novo(Fonte::MonitorExterno, 40);
        teimoso.aplica = false;
        let out = run_with(
            &falso(vec![Painel::novo(Fonte::MonitorExterno, 30), teimoso]),
            60,
        )
        .unwrap();
        assert_eq!(out["observado"], 40);
        assert!(verify(&json!({"nivel": 60}), &out).is_err());
    }

    #[test]
    fn verify_accepts_tolerance_two_and_refuses_more() {
        let args = json!({"nivel": 60});
        assert!(verify(&args, &json!({"observado": 60, "pedido": {"nivel": 60}})).is_ok());
        assert!(verify(&args, &json!({"observado": 62, "pedido": {"nivel": 60}})).is_ok());
        assert!(matches!(
            verify(&args, &json!({"observado": 63, "pedido": {"nivel": 60}})),
            Err(ToolError::VerificationFailed(_))
        ));
    }

    #[test]
    fn ddc_values_map_between_zero_to_one_hundred_and_the_monitor_range() {
        assert_eq!(vcp_de_percentual(0, 100), 0);
        assert_eq!(vcp_de_percentual(60, 100), 60);
        assert_eq!(vcp_de_percentual(100, 255), 255);
        assert_eq!(percentual_de_vcp(0, 255), 0);
        assert_eq!(percentual_de_vcp(128, 255), 50);
        assert_eq!(percentual_de_vcp(255, 255), 100);
        assert_eq!(percentual_de_vcp(300, 255), 100, "acima do máximo não passa de 100");
    }

    #[test]
    fn test_build_never_reaches_the_real_screen() {
        assert_eq!(
            run_os(60).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_TELA.into())
        );
        assert!(!internal_present());
        assert!(!ddc_present());
    }

    #[test]
    fn availability_needs_a_screen_capability() {
        let registry = ToolRegistry::with_default_tools();
        assert!(registry.is_enabled("brilho"));
        assert_eq!(
            registry.get_if_available("brilho", &ToolContext::new()).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_TELA.into())
        );
        for cap in ["internal_display", "ddc_display"] {
            let ctx = ToolContext::new().with_capability(cap);
            assert!(registry.get_if_available("brilho", &ctx).is_ok(), "{cap}");
        }
    }

    #[test]
    fn audit_summary_names_only_the_source_category() {
        let ok = |v: Value| -> Result<Value, ToolError> { Ok(v) };
        assert_eq!(
            audit_summary(&json!({}), &ok(json!({"nivel": 60, "fonte": "tela_interna"}))),
            "brilho em 60% (tela interna)"
        );
        assert_eq!(
            audit_summary(&json!({}), &ok(json!({"nivel": 60, "fonte": "monitor_externo"}))),
            "brilho em 60% (monitor externo)"
        );
        assert_eq!(
            audit_summary(&json!({}), &Err(ToolError::ExecutionFailed("x".into()))),
            "falha no controle de brilho"
        );
    }
}
