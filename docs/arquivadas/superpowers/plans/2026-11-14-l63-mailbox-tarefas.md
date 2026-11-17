# Plano — L6.3: Caixa `coucou-mailbox/v1`, outbox local e sync de tarefas

**Spec de referência:** [2026-11-14-l63-mailbox-tarefas.md](../specs/2026-11-14-l63-mailbox-tarefas.md)
**Branch:** `feat/l63-mailbox-tarefas`

## Pré-condição antes de codar

- Confirmar árvore limpa na branch `feat/l63-mailbox-tarefas` e sincronização com `upstream/main` (conforme regra do `AGENTS.md`).
- Ler as seções correspondentes da spec antes de editar cada módulo e executar cada etapa com testes antes de avançar.

## Etapas

### 1. Cifra `coucou-mailbox/v1` em `windows/src-tauri/src/phone_link/relay_crypto.rs`
- Finalizar o bloco `coucou-mailbox/v1` já esboçado no arquivo (`MailboxRole::{Pc = 1, Phone = 2}`, `MailboxError::{TooShort, BadVersion, BadTag, BadLength, BadPadding, BadJson, InvalidRoom}`, `mailbox_aad`, `pad_mailbox_plaintext`, `unpad_mailbox_plaintext`, `seal_mailbox`, `seal_mailbox_with_nonce`, `open_mailbox` recebendo `k_mailbox(&creds.key)`), removendo os `#[allow(dead_code)]` ao conectar nos módulos seguintes.
- Garantir recusa de `body.len() > MAX_FRAME` (`BadLength`) em `open_mailbox` e checar que a ordem de validação casa exatamente com os códigos `expect` dos vetores (`too_short`, `bad_version`, `bad_tag`, `bad_length`, `bad_padding`, `bad_json`).
- **Testes:** adicionar testes em `relay_crypto.rs` lendo `android/relay/test-vectors-mailbox.json` (match byte a byte dos 3 `messages` e recusa estrita dos 9 `mustRefuse` + roundtrip), mantendo verde `secrets_never_print_and_the_module_never_logs`.

### 2. Cliente HTTP da caixa em `windows/src-tauri/src/phone_link/relay_client.rs` e `phone_link/mod.rs`
- Em `phone_link/mod.rs`, expor `pub(crate) fn mailbox_credentials(app: &AppHandle) -> Option<(Target, Credentials)>` (sem adicionar comando Tauri mutante no módulo, preservando `every_command_that_changes_something_checks_the_calling_window` e as 6 chaves em `secrets::INTERNAL_KEYS`).
- Em `relay_client.rs`, criar tipos de resposta e `MailboxHttpError` cobrindo explicitamente `401`, `403`, `410`, `413`, `429` e `507`.
- Implementar `mailbox_post`, `mailbox_list` (com `limit` `1..=100` e decodificação base64 padrão), `mailbox_ack` e `mailbox_stats` usando `Authorization: Bearer` e `X-Lumen-Join` (mais `X-Lumen-Msg-Id` no `POST`).
- **Testes:** servidor HTTP loopback em `#[cfg(test)]` cobrindo `200`/`201`, headers, paginação `after`, erros `401`/`403`/`410`/`413`/`429`/`507` e guarda `the_source_never_logs_or_prints`.

### 3. Estado `Todo/sync.json` e motor de reconciliação em `windows/src-tauri/src/tasks/sync.rs`
- Declarar `tasks::sync` em `windows/src-tauri/src/tasks/mod.rs` e definir `SyncState` persistido atomicamente em `Todo/sync.json`: `device_id` UUID v4 (`ring::rand::SystemRandom`), `last_pc_seq`, `outbox`, `applied_mutations`, `base_copies`, `extra_fields`, `conflicts`, `quarantine`, `poison_count`, `pending_phone`, `revoked`, `last_sync_at`, `last_error`.
- Definir `SyncEnvelope` v1 exato (`contract_version`, `mutation_id`, `device_id`, `origin_role`, `entity_type`, `entity_id`, `base_version`, `new_version`, `updated_at`, `payload`).
- Implementar aplicação idempotente (`applied_mutations` + ignorar próprio `device_id`), quarentena (`contract_version > 1` ou `entity_type != "task"`), contagem de mensagens venenosas sem logar conteúdo e preservação de campos desconhecidos via `extra_fields`.
- Implementar reconciliação de 3 vias `(base, local, remote)`: tombstones (>= 90 dias) vencem, `done: true` (`done_at` menor), `snoozed_until` maior, subtarefas por `id`, LWW por `updated_at` UTC (`device_id` desempata) e registro de `TaskConflict` (`pending_review`) para conflitos de texto livre (`item.text` e `subtask.text`).
- **Testes:** validar leitura/aplicação de `tests/fixtures/contrato-sync-v1-task.json`, idempotência de `mutation_id`, quarentena, preservação de campos desconhecidos, tombstones, merge de subtarefas e conflito `pending_review` + resolução.

### 4. Integração no ciclo de `tasks/{store,mod}.rs` e `lib.rs`
- Conectar `SyncState` ao `TaskManager`/`TaskStore` para que toda mutação local (`create`, `update`, `toggle_done` [emitindo `item` + `next_occurrence`], `snooze`, `delete`, `restore`, `add_subtask`, `toggle_subtask`, `remove_subtask` e `handle_toast_action`) grave `todo.json` e enfileire na `outbox` em `Todo/sync.json` antes de despertar a rede.
- Garantir na aplicação remota que conclusão (`done: true`) **não** cria `next_occurrence` e que `done`, exclusão ou adiamento remoto chama `dismiss_if_active` (limpa `fired`, `clear_task_toast` e emite `"task-dismiss"`).
- Implementar worker em segundo plano (`pull` → aplicação atômica → `ack` contíguo → `push` FIFO → `stats`), com backoff em `429`/erro de rede e pausa automática em `401`/`403`/`410`, encerrado no `tasks::shutdown`.
- Expor `tasks_sync_now` e `tasks_resolve_conflict` em `tasks/mod.rs` e registrá-los em `windows/src-tauri/src/lib.rs` (`tauri::generate_handler![]`).
- **Testes:** teste de integração em memória com servidor mailbox loopback verificando fluxo completo: mutação offline → status `"Salvo neste PC"` → sync com `pending_phone > 0` ainda `"Salvo neste PC"` → `pending_phone == 0` vira `"Sincronizado"`, mais aplicação remota com dispensa de lembrete.

### 5. UI TypeScript e testes (`windows/src/` e `windows/tests/tasks.test.mjs`)
- Estender `TasksSnapshot` e `TasksBridge` (`windows/src/core/tasks.ts` e `core/bridge.ts`) com `sync: TaskSyncSnapshot`, `syncNow()` e `resolveConflict(id, choice)`.
- Atualizar `windows/src/hub/pages/tasks.ts` (e `windows/src/hub/hub.css` / `windows/src/i18n/extra.json` se necessário) para exibir sempre `"Salvo neste PC"` vs `"Sincronizado"` sem ambiguidade, botão `"Sincronizar agora"`, avisos de erro/quarentena/revogação e cartão inline de resolução de conflito.
- Tratar `"task-dismiss"` em `windows/src/views/tasks.ts` / `windows/src/main.ts` para fechar o card de lembrete ativo quando dispensado remotamente.
- **Testes:** expandir `windows/tests/tasks.test.mjs` cobrindo transição `"Salvo neste PC"` → `"Sincronizado"` (incluindo `pendingLocal > 0` e `pendingPhone > 0`), acionamento de `tasks_sync_now` e resolução inline via `tasks_resolve_conflict`.

### 6. Verificação final e encerramento da subfase
- Atualizar `docs/STATE-FORK.md` e `docs/arquivadas/etapas/l6-sync-segunda-mente.md` marcando L6.3 somente após todas as checagens passarem.

## Verificação prevista (após implementação)

- Em `windows/src-tauri/`: `cargo test --lib`
- Em `windows/`: `npx tsc --noEmit` e `npm test`
- Em `android/relay/`: `npm test`
- Teste real contra Worker Cloudflare e Galaxy S25 Ultra: **`NOT_RUN`** nesta etapa (programado no checkpoint após L6.4 + F12-04).
