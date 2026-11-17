//! Sondagem do computador antes de cada pedido, entregue às ferramentas como capacidades do `ToolContext`.
//!
//! As ferramentas só leem o contexto; quem sonda é este módulo. Fora do Windows, e nos testes, nenhuma
//! capacidade é declarada, para que o sistema real não seja tocado.

use super::registry::ToolContext;
use super::tools::audio;

/// Contexto de um pedido: a pausa rápida e as capacidades que este computador tem agora.
pub fn context(paused: bool) -> ToolContext {
    let mut ctx = ToolContext::new().with_paused(paused);
    if audio::output_present() {
        ctx = ctx.with_capability("audio_endpoint");
    }
    ctx
}
