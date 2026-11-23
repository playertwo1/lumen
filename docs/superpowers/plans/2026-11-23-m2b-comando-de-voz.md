# M2b — Comando de voz por atalho (plano)

Spec: [2026-11-23-m2b-comando-de-voz.md](../specs/2026-11-23-m2b-comando-de-voz.md)

1. `windows/src-tauri/src/voice/command.rs` (novo): `parse(texto) -> Option<(ferramenta, argumentos)>` com testes; comando Tauri `voice_command` que grava até o silêncio (`wake::Vad`), transcreve (`speech::transcribe_dictation`) e chama `invoke_tool`.
2. `windows/src-tauri/src/shortcuts.rs`: ação `voiceCommand` "Ctrl+Alt+V" e descrição; `lib.rs` registra o comando Tauri.
3. `windows/src/core/shortcuts.ts`: mesma ação e rótulo; `src/island/shortcuts.ts` chama `Bridge.voiceCommand()`; `src/core/bridge.ts` ganha a chamada; rótulo traduzido em `src/i18n/extra.json`.
4. Testes: `cargo test --lib voice::command`, `npm test` (`tests/shortcuts.test.mjs` atualizado), `npx tsc --noEmit`. *(Feito em 2026-11-23: 958 testes Rust e 774 TS passando.)*
5. Teste real: `npm run tauri dev`, Ctrl+Alt+V + "brilho a 20%" e "tirar som". *(2026-11-23, modo dev: com a fala sintética no lugar do microfone, a tecla levou o brilho de 50 a 20% e silenciou o som, conferido pelo Core Audio; "volta o som" reativou. A voz da caixa de som não serve de teste: o cancelamento de eco do microfone a apaga. **Fala humana no microfone: NOT_RUN**.)*

Rollback: remover a ação `voiceCommand` das duas tabelas. Uma tecla gravada em `settings.json` para uma ação desconhecida é ignorada.
