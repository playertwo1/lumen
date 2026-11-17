//! Ferramentas nativas do Windows da M2. Cada uma é registrada e habilitada aqui.

pub mod audio;

use super::registry::ToolRegistry;

pub fn register_m2(registry: &mut ToolRegistry) {
    registry
        .register_and_enable(audio::tool())
        .expect("definição padrão de audio deve ser válida");
}
