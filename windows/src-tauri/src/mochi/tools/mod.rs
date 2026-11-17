//! Ferramentas nativas do Windows da M2. Cada uma é registrada e habilitada aqui.

pub mod apps;
pub mod arrange;
pub mod audio;
pub mod brightness;

use super::registry::ToolRegistry;

pub fn register_m2(registry: &mut ToolRegistry) {
    registry
        .register_and_enable(arrange::tool())
        .expect("definição padrão de janelas deve ser válida");
    registry
        .register_and_enable(audio::tool())
        .expect("definição padrão de audio deve ser válida");
    registry
        .register_and_enable(brightness::tool())
        .expect("definição padrão de brilho deve ser válida");
    registry
        .register_and_enable(apps::tool())
        .expect("definição padrão de apps deve ser válida");
}
