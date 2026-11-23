// Comando de voz por atalho (M2b): uma tecla, uma frase curta, uma ferramenta.
//
// O atalho `voiceCommand` (shortcuts.rs) chama `voice_command`. O microfone fica aberto até a pessoa parar
// de falar; o modelo de ditado local escreve a frase, e `parse` a traduz por regras, sem IA, numa chamada
// das ferramentas da M2. A execução passa por `invoke_tool`, como os botões e o chat: pausa rápida,
// permissões, cartão de confirmação e registro local.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::{show, wake, VoiceView};
use crate::mochi::{approval_bridge::TauriApprovalRequester, host, MochiRuntime};
use crate::speech::{self, audio};

/// Quanto tempo se espera pela frase, contando de quando a tecla foi apertada.
const LISTEN_FOR: Duration = Duration::from_secs(8);

static BUSY: AtomicBool = AtomicBool::new(false);

/// Número de 0 a 100 dito por extenso.
fn word_value(w: &str) -> Option<i64> {
    Some(match w {
        "zero" => 0,
        "um" | "uma" => 1,
        "dois" | "duas" => 2,
        "tres" => 3,
        "quatro" => 4,
        "cinco" => 5,
        "seis" => 6,
        "sete" => 7,
        "oito" => 8,
        "nove" => 9,
        "dez" => 10,
        "onze" => 11,
        "doze" => 12,
        "treze" => 13,
        "catorze" | "quatorze" => 14,
        "quinze" => 15,
        "dezesseis" => 16,
        "dezessete" => 17,
        "dezoito" => 18,
        "dezenove" => 19,
        "vinte" => 20,
        "trinta" => 30,
        "quarenta" => 40,
        "cinquenta" => 50,
        "sessenta" => 60,
        "setenta" => 70,
        "oitenta" => 80,
        "noventa" => 90,
        "cem" | "cento" => 100,
        _ => return None,
    })
}

/// O primeiro número em `words`: dígitos ("20"), ou por extenso ("trinta e cinco").
fn number(words: &[&str]) -> Option<i64> {
    let mut total: Option<i64> = None;
    for w in words {
        if let Ok(n) = w.parse::<i64>() {
            return Some(total.unwrap_or(0) + n);
        }
        match (word_value(w), total) {
            (Some(v), t) => total = Some(t.unwrap_or(0) + v),
            (None, Some(_)) if *w == "e" => {}
            (None, Some(_)) => break,
            (None, None) => {}
        }
    }
    total
}

/// A frase ouvida → (ferramenta, argumentos). `None` quando não é um comando conhecido.
pub fn parse(heard: &str) -> Option<(&'static str, Value)> {
    let norm = wake::normalize(heard);
    let words: Vec<&str> = norm.split(' ').collect();
    // "birlho" e "brilo": como o modelo às vezes escreve "brilho".
    if let Some(at) = words.iter().position(|w| matches!(*w, "brilho" | "birlho" | "brilo" | "brillo")) {
        let nivel = number(&words[at + 1..])?.clamp(0, 100);
        return Some(("brilho", json!({ "nivel": nivel })));
    }
    let padded = format!(" {norm} ");
    let has = |phrases: &[&str]| phrases.iter().any(|p| padded.contains(&format!(" {p} ")));
    if has(&["volta o som", "voltar o som", "volta som", "voltar som", "liga o som", "ligar o som", "ligar som", "reativar o som", "reativar som", "reativa o som"]) {
        return Some(("audio", json!({ "acao": "reativar_som" })));
    }
    if has(&["tirar o som", "tira o som", "tirar som", "tira som", "silenciar", "silencia", "mudo", "sem som"]) {
        return Some(("audio", json!({ "acao": "silenciar" })));
    }
    None
}

/// A tecla do comando de voz foi apertada: escuta uma frase e a executa.
#[tauri::command]
pub fn voice_command(app: AppHandle) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("lumen-voice-command".into())
        .spawn(move || {
            let answer = listen_and_run(&app);
            crate::log::line(format!("voice command: {answer}"));
            show(&app, Some(VoiceView { phase: "speaking".into(), heard: String::new(), answer }));
            BUSY.store(false, Ordering::SeqCst);
            std::thread::sleep(Duration::from_secs(4));
            // Só some se nenhum outro comando começou nesse meio-tempo.
            if !BUSY.load(Ordering::SeqCst) {
                show(&app, None);
            }
        })
        .ok();
}

fn listen_and_run(app: &AppHandle) -> String {
    if !speech::dictation_ready() {
        return "Baixe o modelo de voz em Ajustes → Reconhecimento.".into();
    }
    speech::preload();
    let Some(samples) = listen(app) else {
        return "Não ouvi nenhum comando.".into();
    };
    show(app, Some(VoiceView { phase: "thinking".into(), heard: String::new(), answer: String::new() }));
    let heard = speech::transcribe_dictation(&samples).map(|t| t.text.trim().to_string()).unwrap_or_default();
    let Some((tool, args)) = parse(&heard) else {
        return format!("Não entendi: \"{heard}\"");
    };
    show(app, Some(VoiceView { phase: "thinking".into(), heard: heard.clone(), answer: String::new() }));
    tauri::async_runtime::block_on(run_tool(app, tool, args)).unwrap_or_else(|e| e)
}

/// Grava até o fim da primeira fala (o mesmo detector do « OK Lumen »), ou até `LISTEN_FOR`.
fn listen(app: &AppHandle) -> Option<Vec<f32>> {
    let mic = crate::flow::get(app).map(|f| f.store.lock().unwrap().prefs.mic.clone()).unwrap_or_default();
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let capture = audio::Capture::start(audio::Source::Mic(if mic.is_empty() { None } else { Some(mic) }), move |s| {
        let _ = tx.send(s.to_vec());
    });
    let mut capture = match capture {
        Ok(c) => c,
        Err(err) => {
            crate::log::line(format!("voice command: microphone: {err}"));
            return None;
        }
    };
    show(app, Some(VoiceView { phase: "listening".into(), heard: String::new(), answer: String::new() }));
    let mut vad = wake::Vad::default();
    // 1,2 s de silêncio encerram a frase, como as perguntas do « OK Lumen »: com 0,75 s, "brilho a … vinte" era cortado.
    vad.hangover = 40;
    let until = Instant::now() + LISTEN_FOR;
    let mut heard = None;
    let mut all = Vec::new();
    while heard.is_none() && Instant::now() < until {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
            all.extend_from_slice(&chunk);
            heard = vad.feed(&chunk).into_iter().find(|s| s.len() >= speech::SAMPLE_RATE as usize * 3 / 10);
        }
    }
    capture.stop();
    // O tempo acabou no meio de uma fala (ruído perto do limiar, frase longa): vale o que foi gravado.
    let peak = all.iter().fold(0f32, |m, s| m.max(s.abs()));
    crate::log::line(format!("voice command: {:.1} s gravados, pico {peak:.3}, fim de frase {}", all.len() as f32 / speech::SAMPLE_RATE as f32, heard.is_some()));
    let heard = heard.or_else(|| (peak >= 0.02).then_some(all));
    heard
}

/// O mesmo caminho do botão de comando (`mochi_button_run`), com os argumentos já montados.
async fn run_tool(app: &AppHandle, tool: &str, args: Value) -> Result<String, String> {
    let runtime: MochiRuntime = app.state::<MochiRuntime>().inner().clone();
    let (run_id, cancel_rx) = runtime.begin_run();
    let requester = TauriApprovalRequester::for_run(app.clone(), runtime.clone(), run_id);
    let ctx = host::context(crate::integrations::PAUSED.load(Ordering::Relaxed)).await;
    let result = runtime.invoke_tool(tool, args, &ctx, &requester, cancel_rx).await;
    runtime.finish_run(run_id);
    result.map(|ok| ok.audit_summary).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_with_digits_words_and_the_misheard_word() {
        assert_eq!(parse("Brilho a 20%."), Some(("brilho", json!({ "nivel": 20 }))));
        assert_eq!(parse("birlho a 20%"), Some(("brilho", json!({ "nivel": 20 }))));
        assert_eq!(parse("brilho em vinte por cento"), Some(("brilho", json!({ "nivel": 20 }))));
        assert_eq!(parse("põe o brilho em trinta e cinco"), Some(("brilho", json!({ "nivel": 35 }))));
        assert_eq!(parse("brilho cem"), Some(("brilho", json!({ "nivel": 100 }))));
        assert_eq!(parse("brilho 250"), Some(("brilho", json!({ "nivel": 100 }))));
        assert_eq!(parse("brilho"), None, "sem número, nada acontece");
    }

    #[test]
    fn mute_and_unmute() {
        assert_eq!(parse("Tirar som."), Some(("audio", json!({ "acao": "silenciar" }))));
        assert_eq!(parse("tira o som"), Some(("audio", json!({ "acao": "silenciar" }))));
        assert_eq!(parse("silenciar"), Some(("audio", json!({ "acao": "silenciar" }))));
        assert_eq!(parse("Volta o som"), Some(("audio", json!({ "acao": "reativar_som" }))));
    }

    #[test]
    fn anything_else_is_not_a_command() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("abre o navegador"), None);
        assert_eq!(parse("somente isso"), None, "\"som\" dentro de outra palavra não conta");
    }

    #[test]
    fn every_command_is_valid_for_its_tool() {
        let registry = crate::mochi::registry::ToolRegistry::with_default_tools();
        let caps = ["audio_endpoint", "internal_display", "display"]
            .into_iter()
            .fold(crate::mochi::registry::ToolContext::new(), |c, cap| c.with_capability(cap));
        for heard in ["brilho a 20%", "tirar som", "volta o som"] {
            let (tool, args) = parse(heard).unwrap();
            let def = registry.get_if_available(tool, &caps).unwrap();
            crate::mochi::validate_schema(&def.input_schema, &args).unwrap();
        }
    }
}

