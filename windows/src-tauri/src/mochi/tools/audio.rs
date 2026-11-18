//! Ferramenta `audio` (M2.1): lê ou define o volume do endpoint padrão de saída, silencia ou reativa o som.
//!
//! `acao` é enum e `nivel` é inteiro validado. Nenhum texto do pedido chega ao sistema.

use std::time::Duration;

use serde_json::{json, Value};

use crate::mochi::registry::{
    Availability, ExecutionContext, RiskLevel, ToolContext, ToolDefinition, ToolError, ToolFuture,
};

const SEM_DISPOSITIVO: &str = "sem dispositivo de saída de áudio";
/// Arredondamento do endpoint do Windows gera diferença de 1 ponto.
const TOLERANCIA: i64 = 1;

/// Operação já validada: é o que `run_with` leva ao endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Ler,
    Volume(u8),
    Mudo(bool),
}

/// Estado do endpoint: volume de 0 a 100 e mudo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reading {
    pub nivel: u8,
    pub mudo: bool,
}

/// Endpoint de saída padrão. Os testes usam um falso; o real só existe no Windows fora de `cfg(test)`.
pub(crate) trait AudioBackend {
    /// `Ok(None)` quando não há dispositivo de saída padrão.
    fn read(&self) -> Result<Option<Reading>, String>;
    /// `Ok(false)` quando não há dispositivo de saída padrão.
    fn write(&self, op: Op) -> Result<bool, String>;
}

pub fn tool() -> ToolDefinition {
    ToolDefinition {
        id: "audio",
        description: "Lê o volume do sistema, define o volume de 0 a 100, silencia ou reativa o som.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "acao": { "type": "string", "enum": ["ler", "definir_volume", "silenciar", "reativar_som"] },
                "nivel": { "type": "integer", "minimum": 0, "maximum": 100 }
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
    if ctx.has_capability("audio_endpoint") {
        Availability::Available
    } else {
        Availability::Unavailable {
            reason: SEM_DISPOSITIVO.to_string(),
        }
    }
}

fn plano(args: &Value) -> Result<Op, ToolError> {
    let acao = args.get("acao").and_then(Value::as_str).unwrap_or_default();
    let nivel = args.get("nivel");
    if acao == "definir_volume" {
        let n = nivel
            .and_then(Value::as_u64)
            .filter(|n| *n <= 100)
            .ok_or_else(|| ToolError::InvalidInput("definir_volume exige nivel de 0 a 100".into()))?;
        return Ok(Op::Volume(n as u8));
    }
    // `nivel` solto (o modelo costuma mandar o volume atual) não muda nada nas outras ações: ignora.
    match acao {
        "ler" => Ok(Op::Ler),
        "silenciar" => Ok(Op::Mudo(true)),
        "reativar_som" => Ok(Op::Mudo(false)),
        _ => Err(ToolError::InvalidInput("ação de áudio desconhecida".into())),
    }
}

fn acao_do(op: Op) -> &'static str {
    match op {
        Op::Ler => "ler",
        Op::Volume(_) => "definir_volume",
        Op::Mudo(true) => "silenciar",
        Op::Mudo(false) => "reativar_som",
    }
}

fn pedido(op: Op) -> Value {
    match op {
        Op::Ler => json!({}),
        Op::Volume(n) => json!({ "nivel": n }),
        Op::Mudo(m) => json!({ "mudo": m }),
    }
}

fn dry_run(args: &Value) -> Result<String, ToolError> {
    Ok(match plano(args)? {
        Op::Ler => "Ler o volume e o estado de mudo do sistema.".to_string(),
        Op::Volume(n) => format!("Definir o volume do sistema em {n}%."),
        Op::Mudo(true) => "Silenciar o som do sistema.".to_string(),
        Op::Mudo(false) => "Reativar o som do sistema.".to_string(),
    })
}

/// Age, relê o endpoint e devolve o pedido ao lado do estado observado.
pub(crate) fn run_with(backend: &dyn AudioBackend, op: Op) -> Result<Value, ToolError> {
    if op != Op::Ler {
        match backend.write(op) {
            Ok(true) => {}
            Ok(false) => return Err(ToolError::PrerequisiteMissing(SEM_DISPOSITIVO.to_string())),
            Err(motivo) => return Err(ToolError::ExecutionFailed(motivo)),
        }
    }
    let lido = backend
        .read()
        .map_err(ToolError::ExecutionFailed)?
        .ok_or_else(|| ToolError::PrerequisiteMissing(SEM_DISPOSITIVO.to_string()))?;
    Ok(json!({
        "acao": acao_do(op),
        "nivel": lido.nivel,
        "mudo": lido.mudo,
        "pedido": pedido(op),
    }))
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
    let _com = win::Com::init();
    run_with(&win::Os, op)
}

#[cfg(not(all(windows, not(test))))]
fn run_os(op: Op) -> Result<Value, ToolError> {
    run_with(&SemDispositivo, op)
}

/// Fora do Windows e nos testes: não há endpoint, e nada é tocado.
struct SemDispositivo;

impl AudioBackend for SemDispositivo {
    fn read(&self) -> Result<Option<Reading>, String> {
        Ok(None)
    }

    fn write(&self, _op: Op) -> Result<bool, String> {
        Ok(false)
    }
}

/// Há saída de áudio padrão? Sondagem para `host::context`; nunca roda nos testes.
#[cfg(all(windows, not(test)))]
pub(crate) fn output_present() -> bool {
    let _com = win::Com::init();
    // Guarda o resultado antes do COM sair de escopo: o objeto liberado depois do CoUninitialize quebra.
    let present = matches!(win::endpoint(), Ok(Some(_)));
    present
}

#[cfg(not(all(windows, not(test))))]
pub(crate) fn output_present() -> bool {
    false
}

fn verify(_args: &Value, output: &Value) -> Result<(), ToolError> {
    let pedido = &output["pedido"];
    if let Some(n) = pedido.get("nivel").and_then(Value::as_i64) {
        let observado = output
            .get("nivel")
            .and_then(Value::as_i64)
            .ok_or_else(|| ToolError::VerificationFailed("o volume não pôde ser confirmado".into()))?;
        if (observado - n).abs() > TOLERANCIA {
            return Err(ToolError::VerificationFailed(format!(
                "o volume ficou em {observado}%, e não em {n}%"
            )));
        }
    }
    if let Some(m) = pedido.get("mudo").and_then(Value::as_bool) {
        if output.get("mudo").and_then(Value::as_bool) != Some(m) {
            return Err(ToolError::VerificationFailed(
                "o estado de mudo não foi confirmado".into(),
            ));
        }
    }
    Ok(())
}

fn audit_summary(_args: &Value, result: &Result<Value, ToolError>) -> String {
    let Ok(out) = result else {
        return "falha no controle de volume".to_string();
    };
    match out["acao"].as_str() {
        Some("ler") => "leitura de volume".to_string(),
        Some("definir_volume") => format!("volume em {}%", out["nivel"].as_i64().unwrap_or(0)),
        Some("silenciar") => "som silenciado".to_string(),
        Some("reativar_som") => "som reativado".to_string(),
        _ => "controle de volume".to_string(),
    }
}

#[cfg(all(windows, not(test)))]
pub(crate) mod win {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    use super::{AudioBackend, Op, Reading};

    /// COM da thread atual. Desfaz só o que este código inicializou.
    pub struct Com(bool);

    impl Com {
        pub fn init() -> Self {
            Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn code(e: windows::core::Error) -> String {
        format!("erro do sistema {:#010x}", e.code().0)
    }

    /// Abre o endpoint de saída padrão a cada chamada: o dispositivo padrão pode ser trocado a qualquer hora.
    pub fn endpoint() -> Result<Option<IAudioEndpointVolume>, String> {
        unsafe {
            let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(code)?;
            let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) else {
                return Ok(None);
            };
            device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .map(Some)
                .map_err(code)
        }
    }

    pub struct Os;

    impl AudioBackend for Os {
        fn read(&self) -> Result<Option<Reading>, String> {
            let Some(vol) = endpoint()? else {
                return Ok(None);
            };
            unsafe {
                let escalar = vol.GetMasterVolumeLevelScalar().map_err(code)?;
                let mudo = vol.GetMute().map_err(code)?.as_bool();
                Ok(Some(Reading {
                    nivel: (escalar * 100.0).round().clamp(0.0, 100.0) as u8,
                    mudo,
                }))
            }
        }

        fn write(&self, op: Op) -> Result<bool, String> {
            let Some(vol) = endpoint()? else {
                return Ok(false);
            };
            let feito = unsafe {
                match op {
                    Op::Volume(n) => vol.SetMasterVolumeLevelScalar(f32::from(n) / 100.0, std::ptr::null()),
                    Op::Mudo(m) => vol.SetMute(m, std::ptr::null()),
                    Op::Ler => Ok(()),
                }
            };
            feito.map_err(code)?;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::mochi::registry::{validate_schema, ToolRegistry};

    /// Endpoint falso: `aplica: false` aceita a escrita mas não muda o estado, o caso que `verify` pega.
    struct Falso {
        estado: Cell<Reading>,
        presente: bool,
        aplica: bool,
        falha: bool,
    }

    impl Falso {
        fn com(nivel: u8, mudo: bool) -> Self {
            Self {
                estado: Cell::new(Reading { nivel, mudo }),
                presente: true,
                aplica: true,
                falha: false,
            }
        }
    }

    impl AudioBackend for Falso {
        fn read(&self) -> Result<Option<Reading>, String> {
            if !self.presente {
                return Ok(None);
            }
            if self.falha {
                return Err("falha simulada".to_string());
            }
            Ok(Some(self.estado.get()))
        }

        fn write(&self, op: Op) -> Result<bool, String> {
            if !self.presente {
                return Ok(false);
            }
            if self.falha {
                return Err("falha simulada".to_string());
            }
            if self.aplica {
                let mut e = self.estado.get();
                match op {
                    Op::Volume(n) => e.nivel = n,
                    Op::Mudo(m) => e.mudo = m,
                    Op::Ler => {}
                }
                self.estado.set(e);
            }
            Ok(true)
        }
    }

    fn schema() -> Value {
        tool().input_schema
    }

    #[test]
    fn schema_accepts_bounds_and_rejects_bad_input() {
        let s = schema();
        for ok in [json!({"acao": "definir_volume", "nivel": 0}), json!({"acao": "definir_volume", "nivel": 100})] {
            assert!(validate_schema(&s, &ok).is_ok());
        }
        for bad in [
            json!({"acao": "definir_volume", "nivel": -1}),
            json!({"acao": "definir_volume", "nivel": 101}),
            json!({"acao": "definir_volume", "nivel": 20.5}),
            json!({"acao": "definir_volume", "nivel": "vinte"}),
            json!({"acao": "definir_volume", "nivel": 20, "cmd": "whoami"}),
            json!({"acao": "shell"}),
        ] {
            assert!(matches!(validate_schema(&s, &bad), Err(ToolError::InvalidInput(_))), "{bad}");
        }
    }

    #[test]
    fn dry_run_refuses_invalid_combinations() {
        assert!(matches!(
            dry_run(&json!({"acao": "definir_volume"})),
            Err(ToolError::InvalidInput(_))
        ));
        // nivel solto é ignorado, não rejeitado: o modelo repetia o erro até estourar o limite de passos
        assert!(dry_run(&json!({"acao": "silenciar", "nivel": 10})).is_ok());
        assert!(dry_run(&json!({"acao": "ler", "nivel": 10})).is_ok());
        assert_eq!(plano(&json!({"acao": "silenciar", "nivel": 10})).unwrap(), Op::Mudo(true));
        assert!(matches!(
            dry_run(&json!({"acao": "definir_volume", "nivel": 101})),
            Err(ToolError::InvalidInput(_))
        ));
        assert_eq!(
            dry_run(&json!({"acao": "definir_volume", "nivel": 20})).unwrap(),
            "Definir o volume do sistema em 20%."
        );
    }

    #[test]
    fn verify_uses_tolerance_one_for_volume_and_exact_mute() {
        let pedido = json!({"acao": "definir_volume", "nivel": 20});
        assert!(verify(&pedido, &json!({"nivel": 20, "mudo": false, "pedido": {"nivel": 20}})).is_ok());
        assert!(verify(&pedido, &json!({"nivel": 21, "mudo": false, "pedido": {"nivel": 20}})).is_ok());
        assert!(matches!(
            verify(&pedido, &json!({"nivel": 22, "mudo": false, "pedido": {"nivel": 20}})),
            Err(ToolError::VerificationFailed(_))
        ));

        let mudo = json!({"acao": "silenciar"});
        assert!(verify(&mudo, &json!({"nivel": 20, "mudo": true, "pedido": {"mudo": true}})).is_ok());
        assert!(matches!(
            verify(&mudo, &json!({"nivel": 20, "mudo": false, "pedido": {"mudo": true}})),
            Err(ToolError::VerificationFailed(_))
        ));

        assert!(verify(&json!({"acao": "ler"}), &json!({"nivel": 7, "mudo": false, "pedido": {}})).is_ok());
    }

    #[test]
    fn run_with_without_endpoint_is_prerequisite_missing() {
        let ausente = Falso { presente: false, ..Falso::com(0, false) };
        assert_eq!(
            run_with(&ausente, Op::Volume(20)).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_DISPOSITIVO.to_string())
        );
        assert_eq!(
            run_with(&SemDispositivo, Op::Ler).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_DISPOSITIVO.to_string())
        );
    }

    #[test]
    fn run_with_endpoint_that_ignores_write_fails_verification() {
        let teimoso = Falso { aplica: false, ..Falso::com(50, false) };
        let out = run_with(&teimoso, Op::Volume(20)).unwrap();
        assert_eq!(out["nivel"], 50);
        assert!(matches!(
            verify(&json!({"acao": "definir_volume", "nivel": 20}), &out),
            Err(ToolError::VerificationFailed(_))
        ));
    }

    #[test]
    fn run_with_applied_value_succeeds_and_verifies() {
        let fake = Falso::com(50, false);
        let out = run_with(&fake, Op::Volume(20)).unwrap();
        assert_eq!(out["acao"], "definir_volume");
        assert_eq!(out["nivel"], 20);
        assert_eq!(out["mudo"], false);
        assert!(verify(&json!({"acao": "definir_volume", "nivel": 20}), &out).is_ok());

        let out = run_with(&fake, Op::Mudo(true)).unwrap();
        assert_eq!(out["mudo"], true);
        assert!(verify(&json!({"acao": "silenciar"}), &out).is_ok());
    }

    #[test]
    fn run_with_backend_error_is_execution_failed() {
        let quebrado = Falso { falha: true, ..Falso::com(50, false) };
        assert!(matches!(
            run_with(&quebrado, Op::Mudo(true)),
            Err(ToolError::ExecutionFailed(_))
        ));
    }

    #[test]
    fn test_build_never_reaches_the_real_endpoint() {
        // Em cfg(test) a execução usa o falso sem dispositivo; nenhuma chamada ao Windows acontece.
        assert_eq!(
            run_os(Op::Volume(20)).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_DISPOSITIVO.to_string())
        );
        assert!(!output_present());
    }

    #[test]
    fn availability_depends_only_on_the_context_capability() {
        let registry = ToolRegistry::with_default_tools();
        assert!(registry.is_enabled("audio"));
        assert_eq!(
            registry.get_if_available("audio", &ToolContext::new()).unwrap_err(),
            ToolError::PrerequisiteMissing(SEM_DISPOSITIVO.to_string())
        );
        let ctx = ToolContext::new().with_capability("audio_endpoint");
        assert!(registry.get_if_available("audio", &ctx).is_ok());
    }

    #[test]
    fn audit_summary_is_fixed_text_without_device_names() {
        let ok = |v: Value| -> Result<Value, ToolError> { Ok(v) };
        assert_eq!(
            audit_summary(&json!({}), &ok(json!({"acao": "definir_volume", "nivel": 20}))),
            "volume em 20%"
        );
        assert_eq!(audit_summary(&json!({}), &ok(json!({"acao": "silenciar"}))), "som silenciado");
        assert_eq!(audit_summary(&json!({}), &ok(json!({"acao": "reativar_som"}))), "som reativado");
        assert_eq!(audit_summary(&json!({}), &ok(json!({"acao": "ler"}))), "leitura de volume");
        assert_eq!(
            audit_summary(&json!({}), &Err(ToolError::ExecutionFailed("x".into()))),
            "falha no controle de volume"
        );
    }
}
