//! Sondagem do computador antes de cada pedido, entregue às ferramentas como capacidades do `ToolContext`.
//!
//! As ferramentas só leem o contexto; quem sonda é este módulo. Fora do Windows, e nos testes, nenhuma
//! capacidade é declarada, para que o sistema real não seja tocado.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::registry::ToolContext;
use super::tools::{apps, arrange, audio, brightness, media, power};

/// Contexto de um pedido: a pausa rápida e as capacidades que este computador tem agora.
/// A sondagem fala com COM e WMI, então roda na thread de bloqueio, nunca no runtime assíncrono.
pub async fn context(paused: bool) -> ToolContext {
    tokio::task::spawn_blocking(move || {
        let mut ctx = ToolContext::new().with_paused(paused);
        if audio::output_present() {
            ctx = ctx.with_capability("audio_endpoint");
        }
        static INTERNA: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        static DDC: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        static MENU: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        if cached(&INTERNA, brightness::internal_present) {
            ctx = ctx.with_capability("internal_display");
        }
        if cached(&DDC, brightness::ddc_present) {
            ctx = ctx.with_capability("ddc_display");
        }
        if cached(&MENU, apps::start_menu_present) {
            ctx = ctx.with_capability("start_menu");
        }
        if arrange::display_present() {
            ctx = ctx.with_capability("display");
        }
        // Sem cache: o player muda a toda hora, e a consulta é rápida.
        if media::session_present() {
            ctx = ctx.with_capability("media_session");
        }
        static SUSPENSAO: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        static DESLIGAMENTO: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        if cached(&SUSPENSAO, power::sleep_supported) {
            ctx = ctx.with_capability("sleep_supported");
        }
        if cached(&DESLIGAMENTO, power::shutdown_privilege) {
            ctx = ctx.with_capability("shutdown_privilege");
        }
        ctx
    })
    .await
    .unwrap_or_else(|_| ToolContext::new().with_paused(paused))
}

/// WMI e DDC/CI são lentos e roda antes de cada mensagem do chat: o resultado vale por 30 s.
fn cached(cache: &Mutex<Option<(Instant, bool)>>, sonda: fn() -> bool) -> bool {
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    match *cache {
        Some((quando, presente)) if quando.elapsed() < Duration::from_secs(30) => presente,
        _ => {
            let presente = sonda();
            *cache = Some((Instant::now(), presente));
            presente
        }
    }
}
