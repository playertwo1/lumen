# Plano de Execução — Fase L7: Google no Lumen

**Spec de referência:** [2026-11-15-l7-google-lumen.md](../specs/2026-11-15-l7-google-lumen.md)

## Passos

1. **Segredos internos no Rust (`windows/src-tauri/src/secrets.rs`):**
   - Adicionar `GOOGLE_INTERNAL_KEYS` (sem alterar `INTERNAL_KEYS` para preservar o contrato testado por `phone_link`) e incluí-la em `is_internal()`.
   - Adicionar teste unitário garantindo que chaves Google passam em `is_internal` e não são expostas à webview.

2. **Helpers de recarga em disco (`tasks/mod.rs` e `notes/mod.rs`):**
   - Adicionar `reload_from_disk(app)` e `get_item(app, id)` em `tasks/mod.rs` para refletir restaurações sem reiniciar o app.
   - Expor `reload_from_disk(app)` em `notes/mod.rs` recarregando `Notes/<id>/note.json` com guarda contra recarga durante gravação ativa.

3. **Módulo `windows/src-tauri/src/google.rs` + registro em `lib.rs`:**
   - Implementar L7.1 (`google_save_client_config`, `google_clear_client_config`, `google_status`, `google_sign_in`, `google_sign_out`, refresh com `REFRESH_LOCK`).
   - Implementar L7.2 (`google_set_weekly_backup`, `google_backup_now`, `google_restore_backup`, scheduler semanal em `setup`/`shutdown`, evento `"google-backup-completed"`).
   - Implementar L7.3 (`google_calendar_list_events`, `google_calendar_create_from_task`).
   - Cobrir com testes unitários e de integração via servidor HTTP loopback sintético no próprio `google.rs`, incluindo verificação estática de que segredos não são logados.
   - Registrar os 10 comandos, `setup(&handle)` e `shutdown(app)` em `windows/src-tauri/src/lib.rs`.
   - Validar com `cargo test --lib` em `windows/src-tauri/`.

4. **Animação P3 da L7 (`compass-engine.ts`, `island.ts`, `main.ts`):**
   - Adicionar `triggerCloudBackup` em `CompassEngine` (nuvem guardando uma caixinha), limpa no ciclo `update(dt)`, considerada no getter `busy`, e inoperante sob `prefers-reduced-motion`.
   - Expor `island.cloudBackup()` e conectar `"google-backup-completed"` em `main.ts`.
   - Cobrir com testes em `windows/tests/lumen-character-engine.test.mjs`.

5. **Ponte e UI em Settings & Hub Tarefas (`bridge.ts`, `settings/main.ts`, `hub/pages/tasks.ts`):**
   - Adicionar tipos e `GoogleBridge` em `windows/src/core/bridge.ts`.
   - Adicionar seção Google em `Integrações` (`windows/src/settings/main.ts`) e entradas correspondentes em `search.ts` se aplicável: configuração de Client ID/JSON, Login/Logout, backup semanal, "Fazer backup agora" e "Restaurar backup" com confirmação explícita em dois passos.
   - Estender `windows/src/hub/pages/tasks.ts` (preservando integralmente as edições existentes do Redesign Etapa 1): fluxo de confirmação no inspetor (`"Agendar no Google Calendar"` -> preview -> `"Confirmar evento no Calendar"`) e painel resumido de agenda & backup Google na página de Tarefas.
   - Adicionar testes unitários frontend em `windows/tests/google-l7.test.mjs`.

6. **Validação completa e atualização do Roadmap:**
   - Executar `npx tsc --noEmit`, `npm test`, `npm run build` em `windows/` e `cargo test --lib` em `windows/src-tauri/`.
   - Atualizar `docs/roadmap/fase-l-lumen.md` e `docs/STATE-FORK.md` marcando `L7.1`, `L7.2`, `L7.3` e `P3` como implementados e validados com testes sintéticos, mantendo `Checkpoint L7` como `[ ]` (`NOT_RUN` para conta real Google).
