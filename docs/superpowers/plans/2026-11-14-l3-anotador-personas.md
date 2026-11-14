# Plano — L3 Anotador de pensamentos/ideias e comentários de personas

## Arquivos e interfaces

- `windows/src-tauri/src/chatgpt.rs` e `windows/src-tauri/src/chat.rs`:
  - Extrair helper compartilhado de leitura SSE em `chatgpt.rs` e expor `pub async fn complete_oneshot(model: &str, system: &str, user: &str) -> Result<String, String>` (sem tocar no histórico do chat).
  - Expor em `chat.rs` `pub async fn complete_oneshot(settings: &Settings, system: &str, user: &str, max_tokens: u32, timeout: Duration) -> Result<String, String>` roteando pelo `chat_provider` ativo (`chatgpt`, `anthropic`, `openai_compat`, `local_chat`, ou CLI quando aplicável).
- `windows/src-tauri/src/notes/personas.rs` (novo módulo):
  - Structs `PersonaProfile` (`id`, `name`, `tags`, `enabled`, `is_primary_companion`, `interest_filter`, `identity`, `user_profile`, `rules`, `persona`, `system_prompt_override`, `post_history_instructions`, `mes_example`), `CommentSettings` (`enable_character_comment`, `max_comment_characters`), `PersonasStore` (`personas`, `settings`) com load/save atômicos em `<local_dir>/Personas/`.
  - `parse_import_json(raw: &str) -> Result<Vec<PersonaProfile>, String>` aceitando JSON de 3 partes (`identity`/`userProfile`/`rules`), JSON único ou lista / `{ characters: [...] }` da Segunda Mente.
  - Seleção (`extract_mention`, `select_single_character`, `score_affinity`, `select_multiple_characters`, `resolve_reply_target_character`, `remaining_persona_turns`) e montagem de prompt (`build_comment_system_prompt`, `build_comment_user_prompt`, `parse_comment_reply` tratando `[SKIP]`).
  - Testes unitários Rust (importação, afinidade, `@menção`, roteamento multi-persona com servidor loopback mock, anti-duplicação, parser `[SKIP]`).
- `windows/src-tauri/src/notes/mod.rs` e `windows/src-tauri/src/notes/builder.rs`:
  - Adicionar `NoteComment` (`id`, `author_id`, `author_name`, `is_ai`, `reply_to_id`, `text`, `timestamp`) e campos retrocompatíveis em `Note` (`comments: Vec<NoteComment>`, `comment_hash: Option<String>`).
  - Atualizar `notes_start` para modo anotador (voz do dono, microfone apenas) e ajustar rótulos/Markdown exportado para incluir comentários.
  - Adicionar trava `commenting: Mutex<HashSet<String>>`, geração automática anti-duplicada após transcrição/criação de nota (`maybe_generate_auto_comments`) e eventos `"note-created"` para o overlay.
  - Expor comandos Tauri: `notes_create_text`, `notes_add_comment`, `notes_generate_comments`, `personas_state`, `personas_import`, `personas_toggle`, `personas_delete`, `personas_set_primary`, `personas_set_comment_settings`.
- `windows/src-tauri/src/lib.rs`: registrar os novos comandos no `generate_handler!`.
- `windows/src/core/flow.ts`, `windows/src/hub/pages/notes.ts`, `windows/src/hub/shell.ts`, `windows/src/hub/search.ts`:
  - Tipos TS para `NoteComment`, `PersonaProfile`, `CommentSettings`, `PersonasState` e chamadas no `FlowBridge`.
  - Interface pt-BR do **Anotador**: captura rápida em texto, gravação de ideia por voz, seção de **Comentários das personas** + campo de resposta do dono (`replyToId` / `@menção`), gerenciamento inline/drawer simples de personas importadas e configurações de comentários.
- `windows/src/lumen/compass-engine.ts`, `windows/src/island/island.ts`, `windows/src/main.ts` e `windows/tests/`:
  - P3 `triggerNoteWrite()` em `CompassEngine` + disparo via evento `"note-created"`.
  - Testes Node cobrindo helper de filtro/compatibilidade de notas e animação P3 (`triggerNoteWrite`).

## Passos

1. Implementar `complete_oneshot` compartilhado em `chatgpt.rs` e `chat.rs` reaproveitando o stream SSE existente com zero duplicação.
2. Criar `windows/src-tauri/src/notes/personas.rs` com modelo em 3 partes, importador compatível com Segunda Mente, seleção, prompt em pt-BR e testes unitários.
3. Integrar `personas.rs` e `complete_oneshot` em `windows/src-tauri/src/notes/mod.rs` (`notes_start` como anotador por voz, `notes_create_text`, `notes_add_comment`, `notes_generate_comments`, trava anti-duplicação + `comment_hash`, export Markdown com comentários, evento `"note-created"`), registrar em `lib.rs` e adicionar testes de antiduplicação.
4. Implementar animação P3 `triggerNoteWrite()` em `compass-engine.ts` e ligar evento `"note-created"` em `main.ts` / `island.ts`.
5. Atualizar `windows/src/core/flow.ts` e `windows/src/hub/pages/notes.ts` (+ textos pt-BR) com captura rápida, thread de comentários de personas e importação/controle de personas.
6. Executar validação completa:
   - `npx tsc --noEmit`
   - `npm test`
   - `npm run build`
   - `cargo test --lib` (em `windows/src-tauri/`)
   - `cargo test` (em `windows/hook/`)
7. Registrar L3 concluída sinteticamente em `docs/roadmap/fase-l-lumen.md` e `docs/STATE-FORK.md` (mantendo Checkpoint real L3 pendente da validação no uso real), commitar na branch `feat/l3-anotador-personas`, integrar na `main` e enviar para `origin/main`.

## Rollback

- Campos adicionados a `Note` (`comments`, `commentHash`) usam `#[serde(default)]`, preservando leitura de arquivos `note.json` anteriores; o armazenamento de personas fica isolado em `<local_dir>/Personas/`, sem alterar outros módulos nem segredos existentes.
