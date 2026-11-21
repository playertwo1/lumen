//! Botões de comando da pílula (M2b): um clique chama a mesma ferramenta que o chat chamaria.
//!
//! A lista é declarada aqui, em Rust. A página só envia o `id` de um botão; nome da ferramenta e argumentos
//! nunca vêm dela. A execução passa por `MochiRuntime::invoke_tool`: pausa rápida, schema, permissões, cartão
//! de confirmação para risco médio e alto, e registro local.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::approval_bridge::TauriApprovalRequester;
use super::registry::{ToolContext, ToolError};
use super::{host, MochiRuntime};
use crate::flow::Flow;
use std::sync::Arc;
use crate::i18n::n_;

/// Como o pedido é montado: fixo, ou a partir de uma leitura feita antes pela mesma ferramenta.
enum Plan {
    Fixed(fn() -> Value),
    /// `read` e `then` passam pelo mesmo pipeline; `Err` vira a mensagem do botão, sem executar nada.
    Reads { read: fn() -> Value, then: fn(&Value) -> Result<Value, String> },
}

struct Spec {
    id: &'static str,
    /// Nome do ícone em `HUB_ICONS` (`src/hub/ui.ts`).
    icon: &'static str,
    /// Texto em inglês, traduzido pela página.
    label: &'static str,
    tool: &'static str,
    plan: Plan,
}

const VOLUME_STEP: i64 = 10;
/// A ferramenta `brilho` não lê o nível atual: os dois botões de brilho vão a níveis fixos.
const BRILHO_BAIXO: i64 = 30;
const BRILHO_CHEIO: i64 = 100;
/// Ordem da esquerda para a direita, a mesma do "organizar para programar" do chat.
const PROGRAMAR: [&str; 3] = ["codex", "claude", "antigravity"];

fn volume_step(read: &Value, delta: i64) -> Result<Value, String> {
    let atual = read["nivel"].as_i64().ok_or("não consegui ler o volume")?;
    Ok(json!({ "acao": "definir_volume", "nivel": (atual + delta).clamp(0, 100) }))
}

fn mute_toggle(read: &Value) -> Result<Value, String> {
    let mudo = read["mudo"].as_bool().ok_or("não consegui ler o estado do som")?;
    Ok(json!({ "acao": if mudo { "reativar_som" } else { "silenciar" } }))
}

/// Janelas já abertas de Codex, Claude Code e Antigravity, uma de cada, lado a lado. Não abre aplicativo
/// e não adivinha: com menos de duas dessas janelas abertas, recusa e diz o que falta.
fn arrange_side_by_side(read: &Value) -> Result<Value, String> {
    let janelas = read["janelas"].as_array().ok_or("não consegui listar as janelas")?;
    let alvos: Vec<i64> = PROGRAMAR
        .iter()
        .filter_map(|nome| {
            janelas.iter().find_map(|j| {
                let titulo = j["titulo"].as_str()?.to_lowercase();
                titulo.contains(nome).then(|| j["alvo"].as_i64()).flatten()
            })
        })
        .collect();
    if alvos.len() < 2 {
        return Err("abra ao menos duas janelas entre Codex, Claude Code e Antigravity".into());
    }
    Ok(json!({ "acao": "lado_a_lado", "alvos": alvos }))
}

const SPECS: &[Spec] = &[
    Spec { id: "media_prev", icon: "prev", label: n_("Previous track"), tool: "midia",
        plan: Plan::Fixed(|| json!({ "acao": "anterior" })) },
    Spec { id: "media_play", icon: "play", label: n_("Play"), tool: "midia",
        plan: Plan::Fixed(|| json!({ "acao": "tocar" })) },
    Spec { id: "media_pause", icon: "pause", label: n_("Pause"), tool: "midia",
        plan: Plan::Fixed(|| json!({ "acao": "pausar" })) },
    Spec { id: "media_next", icon: "next", label: n_("Next track"), tool: "midia",
        plan: Plan::Fixed(|| json!({ "acao": "proxima" })) },
    Spec { id: "volume_down", icon: "volumeDown", label: n_("Volume down"), tool: "audio",
        plan: Plan::Reads { read: || json!({ "acao": "ler" }), then: |r| volume_step(r, -VOLUME_STEP) } },
    Spec { id: "volume_up", icon: "volumeUp", label: n_("Volume up"), tool: "audio",
        plan: Plan::Reads { read: || json!({ "acao": "ler" }), then: |r| volume_step(r, VOLUME_STEP) } },
    Spec { id: "mute", icon: "volumeMute", label: n_("Mute"), tool: "audio",
        plan: Plan::Reads { read: || json!({ "acao": "ler" }), then: mute_toggle } },
    Spec { id: "brightness_low", icon: "sunDim", label: n_("Dim screen"), tool: "brilho",
        plan: Plan::Fixed(|| json!({ "nivel": BRILHO_BAIXO })) },
    Spec { id: "brightness_full", icon: "sun", label: n_("Full brightness"), tool: "brilho",
        plan: Plan::Fixed(|| json!({ "nivel": BRILHO_CHEIO })) },
    Spec { id: "windows_side_by_side", icon: "columns", label: n_("Windows side by side"), tool: "janelas",
        plan: Plan::Reads { read: || json!({ "acao": "listar" }), then: arrange_side_by_side } },
    Spec { id: "windows_undo", icon: "undo", label: n_("Undo"), tool: "janelas",
        plan: Plan::Fixed(|| json!({ "acao": "desfazer" })) },
    Spec { id: "lock", icon: "lock", label: n_("Lock screen"), tool: "bloquear_tela",
        plan: Plan::Fixed(|| json!({})) },
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ButtonInfo {
    pub id: &'static str,
    pub icon: &'static str,
    pub label: &'static str,
    pub available: bool,
    /// Por que o botão está desligado (pausa rápida, ferramenta ausente ou sem o que ela precisa).
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ButtonResult {
    pub ok: bool,
    pub text: String,
}

fn info(runtime: &MochiRuntime, ctx: &ToolContext, spec: &Spec) -> ButtonInfo {
    let reason = if runtime.is_paused(ctx) {
        Some(ToolError::Paused.to_string())
    } else {
        runtime
            .registry()
            .get_if_available(spec.tool, ctx)
            .err()
            .map(|e| e.to_string())
    };
    ButtonInfo { id: spec.id, icon: spec.icon, label: spec.label, available: reason.is_none(), reason }
}

/// The buttons to show: all, in declared order, when nothing was chosen; else the chosen ids in the order
/// given. Unknown ids (a button a later version dropped) and repeats are skipped, never an error.
fn choose(chosen: Option<&[String]>) -> Vec<&'static Spec> {
    let Some(ids) = chosen else { return SPECS.iter().collect() };
    let mut seen = HashSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .filter_map(|id| SPECS.iter().find(|s| s.id == id.as_str()))
        .collect()
}

pub(crate) fn list_for(runtime: &MochiRuntime, ctx: &ToolContext, chosen: Option<&[String]>) -> Vec<ButtonInfo> {
    choose(chosen).into_iter().map(|s| info(runtime, ctx, s)).collect()
}

/// Cliques em andamento: um segundo clique no mesmo botão não executa de novo.
static RUNNING: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

struct Busy(&'static str);

impl Busy {
    fn take(id: &'static str) -> Option<Self> {
        let mut guard = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        // Preguiçoso de propósito: um `Busy` criado e descartado aqui trancaria o mutex de novo no `Drop`.
        guard.get_or_insert_with(HashSet::new).insert(id).then(|| Busy(id))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if let Some(set) = RUNNING.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            set.remove(self.0);
        }
    }
}

#[tauri::command]
pub async fn mochi_buttons(
    runtime: tauri::State<'_, MochiRuntime>,
    flow: tauri::State<'_, Arc<Flow>>,
) -> Result<Vec<ButtonInfo>, String> {
    let chosen = flow.store.lock().unwrap_or_else(PoisonError::into_inner).prefs.command_buttons.clone();
    let ctx = host::context(crate::integrations::PAUSED.load(std::sync::atomic::Ordering::Relaxed)).await;
    Ok(list_for(&runtime, &ctx, chosen.as_deref()))
}

/// Every button there is, for the Settings list; which are shown and in what order is a preference.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub icon: &'static str,
    pub label: &'static str,
}

#[tauri::command]
pub fn mochi_button_catalog() -> Vec<CatalogEntry> {
    SPECS.iter().map(|s| CatalogEntry { id: s.id, icon: s.icon, label: s.label }).collect()
}

#[tauri::command]
pub async fn mochi_button_run(
    app: AppHandle,
    runtime: tauri::State<'_, MochiRuntime>,
    id: String,
) -> Result<ButtonResult, String> {
    let spec = SPECS.iter().find(|s| s.id == id).ok_or_else(|| "botão desconhecido".to_string())?;
    let Some(_busy) = Busy::take(spec.id) else {
        return Ok(ButtonResult { ok: false, text: "já em andamento".into() });
    };
    let runtime: MochiRuntime = runtime.inner().clone();
    let (run_id, cancel_rx) = runtime.begin_run();
    let requester = TauriApprovalRequester::for_run(app, runtime.clone(), run_id);
    let ctx = host::context(crate::integrations::PAUSED.load(std::sync::atomic::Ordering::Relaxed)).await;
    let result = run_plan(&runtime, &requester, &ctx, spec, cancel_rx).await;
    runtime.finish_run(run_id);
    Ok(match result {
        Ok(text) => ButtonResult { ok: true, text },
        Err(text) => ButtonResult { ok: false, text },
    })
}

async fn run_plan(
    runtime: &MochiRuntime,
    requester: &TauriApprovalRequester,
    ctx: &ToolContext,
    spec: &Spec,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<String, String> {
    let args = match &spec.plan {
        Plan::Fixed(args) => args(),
        Plan::Reads { read, then } => {
            let lido = runtime
                .invoke_tool(spec.tool, read(), ctx, requester, cancel_rx.clone())
                .await
                .map_err(|e| e.to_string())?;
            then(&lido.output)?
        }
    };
    runtime
        .invoke_tool(spec.tool, args, ctx, requester, cancel_rx)
        .await
        .map(|ok| ok.audit_summary)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mochi::registry::ToolRegistry;
    use std::sync::Arc;

    fn runtime() -> MochiRuntime {
        MochiRuntime::new_with_registry(Arc::new(ToolRegistry::with_default_tools()))
    }

    fn all_caps() -> ToolContext {
        ["audio_endpoint", "internal_display", "media_session", "display"]
            .into_iter()
            .fold(ToolContext::new(), |c, cap| c.with_capability(cap))
    }

    /// Uma leitura plausível de cada ferramenta, para exercitar o segundo passo dos botões que leem antes.
    fn sample(tool: &str) -> Value {
        match tool {
            "audio" => json!({ "nivel": 40, "mudo": false }),
            _ => json!({ "janelas": [
                { "alvo": 0, "titulo": "Explorer" },
                { "alvo": 1, "titulo": "Antigravity - projeto" },
                { "alvo": 2, "titulo": "Codex" },
                { "alvo": 3, "titulo": "Claude Code" },
            ] }),
        }
    }

    #[test]
    fn every_button_points_at_a_registered_tool_with_valid_arguments() {
        let rt = runtime();
        for s in SPECS {
            let def = rt.registry().get_if_available(s.tool, &all_caps());
            // `bloquear_tela` só existe quando o sistema oferece; o resto tem de estar sempre registrado.
            let Ok(def) = def else {
                assert_eq!(s.tool, "bloquear_tela", "{}", s.id);
                continue;
            };
            let validate = |v: &Value| crate::mochi::validate_schema(&def.input_schema, v).unwrap_or_else(|e| panic!("{}: {e}", s.id));
            match &s.plan {
                Plan::Fixed(args) => validate(&args()),
                Plan::Reads { read, then } => {
                    validate(&read());
                    validate(&then(&sample(s.tool)).unwrap_or_else(|e| panic!("{}: {e}", s.id)));
                }
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        let ids: HashSet<_> = SPECS.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), SPECS.len());
    }

    #[test]
    fn volume_steps_stay_inside_zero_to_one_hundred() {
        assert_eq!(volume_step(&json!({ "nivel": 40 }), 10).unwrap()["nivel"], 50);
        assert_eq!(volume_step(&json!({ "nivel": 5 }), -10).unwrap()["nivel"], 0);
        assert_eq!(volume_step(&json!({ "nivel": 98 }), 10).unwrap()["nivel"], 100);
        assert!(volume_step(&json!({}), 10).is_err());
    }

    #[test]
    fn mute_toggles_from_the_state_just_read() {
        assert_eq!(mute_toggle(&json!({ "mudo": false })).unwrap()["acao"], "silenciar");
        assert_eq!(mute_toggle(&json!({ "mudo": true })).unwrap()["acao"], "reativar_som");
        assert!(mute_toggle(&json!({})).is_err());
    }

    #[test]
    fn side_by_side_picks_the_three_windows_in_order_and_refuses_with_fewer_than_two() {
        let todas = arrange_side_by_side(&sample("janelas")).unwrap();
        assert_eq!(todas["alvos"], json!([2, 3, 1]), "Codex, Claude, Antigravity");
        let duas = json!({ "janelas": [{ "alvo": 7, "titulo": "Claude" }, { "alvo": 9, "titulo": "Codex" }] });
        assert_eq!(arrange_side_by_side(&duas).unwrap()["alvos"], json!([9, 7]));
        let uma = json!({ "janelas": [{ "alvo": 0, "titulo": "Codex" }, { "alvo": 1, "titulo": "Notepad" }] });
        assert!(arrange_side_by_side(&uma).is_err());
        assert!(arrange_side_by_side(&json!({})).is_err());
    }

    #[test]
    fn without_the_capability_the_button_is_off_with_a_reason() {
        let rt = runtime();
        let off = list_for(&rt, &ToolContext::new(), None);
        let media = off.iter().find(|b| b.id == "media_next").unwrap();
        assert!(!media.available);
        assert!(media.reason.is_some());
        let on = list_for(&rt, &all_caps(), None);
        assert!(on.iter().find(|b| b.id == "media_next").unwrap().available);
    }

    #[test]
    fn paused_turns_every_button_off() {
        let rt = runtime();
        let ctx = ToolContext::new().with_capability("media_session").with_paused(true);
        assert!(list_for(&rt, &ctx, None).iter().all(|b| !b.available));
    }

    #[test]
    fn a_second_click_while_running_is_refused() {
        let first = Busy::take("teste").expect("first");
        assert!(Busy::take("teste").is_none());
        drop(first);
        assert!(Busy::take("teste").is_some());
    }

    #[test]
    fn the_choice_orders_filters_and_forgives() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let all: Vec<_> = choose(None).iter().map(|s| s.id).collect();
        assert_eq!(all.len(), SPECS.len(), "nothing chosen: every button");
        assert!(choose(Some(&[])).is_empty(), "an empty choice: none");
        let picked = ids(&["lock", "media_next", "lock", "gone", "mute"]);
        let got: Vec<_> = choose(Some(&picked)).iter().map(|s| s.id).collect();
        assert_eq!(got, ["lock", "media_next", "mute"], "order kept, repeat and unknown id dropped");
    }
}
