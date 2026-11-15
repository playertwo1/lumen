# Spec — L6.3: Caixa `coucou-mailbox/v1`, outbox local e sync de tarefas

**Data:** 2026-11-14
**Branch prevista:** `feat/l63-mailbox-tarefas`
**Referência na ponte:** Passo 7 de `PONTE.md` (`segundamente/docs/lumen/CONTRATO-SYNC.md` seções 1–8 e `android/relay/test-vectors-mailbox.json`)

## 1. Objetivo e escopo

Implementar no app Windows a sincronização ponta a ponta **apenas de tarefas (`entity_type = "task"`)** pela caixa de correio HTTP do Worker Cloudflare (`/v1/mailbox/<room>/*`), com persistência local garantida antes de qualquer rede (`outbox`), aplicação idempotente, reconciliação de 3 vias com resolução manual de conflitos de texto livre e separação inequívoca na UI entre **"Salvo neste PC"** e **"Sincronizado"**.

**Fora de escopo:**
- Seção 9 (`v1.1`) do contrato: notas, conversas, personas, preferências ou anexos.
- Sincronização de tarefas por LAN TLS ou WebSocket ao vivo (P3 / animação da bússola e L6.4 permanecem para etapas próprias).

## 2. O que já existe vs. o que falta

- **Já existe:** derivação `k_mailbox(K)` e constantes `MAX_FRAME`/`MAX_LINE`/`BLOCK`/`TAG` em `windows/src-tauri/src/phone_link/relay_crypto.rs` (além de rascunho local da cifra `coucou-mailbox/v1` no working tree); `Target`, `Credentials`, `existing_credentials` e `revoke_mailbox` em `relay_client.rs`; vetor oficial `android/relay/test-vectors-mailbox.json`; `TodoItem`, `TaskStore`, `TaskManager` e `clear_task_toast` em `windows/src-tauri/src/tasks/`; UI em `windows/src/hub/pages/tasks.ts`.
- **Falta:** concluir/testar a cifra `coucou-mailbox/v1` contra `test-vectors-mailbox.json`, cliente HTTP da caixa (`/messages`, `/ack`, `/stats`), `Todo/sync.json` + motor em `tasks/sync.rs`, disparo de `outbox` em toda mutação local, dispensa de lembrete ativo em mudanças remotas, exposição no snapshot TypeScript e UI de status/conflitos.

## 3. Cifra `coucou-mailbox/v1` (`windows/src-tauri/src/phone_link/relay_crypto.rs`)

- **Tipos:** `MailboxRole::{Pc = 1, Phone = 2}` e `MailboxError` (`InvalidRoom`, `TooShort`, `BadVersion`, `BadTag`, `BadLength`, `BadPadding`, `BadJson`) com `.code()` (`"invalid_room"`, `"too_short"`, `"bad_version"`, `"bad_tag"`, `"bad_length"`, `"bad_padding"`, `"bad_json"`) e `Display` fixo, sem incluir chave, sala, payload ou texto cifrado.
- **Formato do corpo (`ver || nonce || ciphertext || tag`):**
  - `ver`: `0x01` (1 B); `nonce`: 12 B aleatórios (`SystemRandom`, sem repetição); `tag`: 16 B AES-256-GCM.
  - Chave: `k_mailbox(&creds.key)` (`Secret32`, HKDF-SHA256 com `info = "coucou-mailbox/v1"`, removendo `#[allow(dead_code)]` ao plugar).
  - AAD (41 B quando `room` tem 22 bytes ASCII base64url de 16 B): `"coucou-mailbox/v1"` (17 B) || `room` (22 B) || `to as u8` (`0x01` para PC, `0x02` para celular) || `0x01` (1 B).
  - Padding antes da cifra (`pad_mailbox_plaintext` / `unpad_mailbox_plaintext`): `u32_be(json.len()) || json || 0x00...` até o menor múltiplo de 128 B >= `4 + json.len()` (mínimo 128 B). Limite do JSON: `MAILBOX_MAX_JSON` / `MAX_LINE` (`65_536` B); limite do corpo: `MAX_FRAME` (`66_000` B; mínimo `MAILBOX_MIN_BODY` = `157` B).
- **Ordem estrita de validação em `open_mailbox(k_mailbox, room, expected_to, body)`:**
  1. `room` válida (`22` B ASCII base64url de `16` B -> `InvalidRoom`); `body.len() >= 157` e `(body.len() - 29) % 128 == 0` (`TooShort`) e `body.len() <= MAX_FRAME` (`BadLength`);
  2. `body[0] == 0x01` (`BadVersion`);
  3. AES-256-GCM open com `k_mailbox` e AAD (`BadTag` para tag/chave/room/role/nonce/ciphertext inválidos);
  4. `plain.len() >= 128 && plain.len() % 128 == 0` (`TooShort` / `BadPadding`);
  5. `declared = u32::from_be_bytes(plain[..4]) as usize`; exigir `declared <= 65_536 && 4 + declared <= plain.len()` (`BadLength`);
  6. todos os bytes em `plain[4 + declared..]` iguais a `0x00` (`BadPadding`);
  7. UTF-8 e JSON sintaticamente válidos via `serde_json::from_str::<serde_json::Value>` (`BadJson`).
- **Conformidade:** `open_mailbox`, `seal_mailbox` e `seal_mailbox_with_nonce` devem passar nos 3 `messages` e recusar exatamente todos os 9 `mustRefuse` de `android/relay/test-vectors-mailbox.json`. O módulo preserva integralmente a guarda `secrets_never_print_and_the_module_never_logs`.

## 4. Cliente HTTP da caixa (`windows/src-tauri/src/phone_link/relay_client.rs` + `phone_link/mod.rs`)

- **Acesso às credenciais:** adicionar função interna não-comando `pub(crate) fn mailbox_credentials(app: &AppHandle) -> Option<(Target, Credentials)>` em `phone_link/mod.rs` lendo `Shared.settings` (`phone_relay`, `phone_relay_url`) e `existing_credentials(&Keystore)`. Nenhum comando Tauri mutante novo vai em `phone_link/mod.rs` (preserva o guarda `every_command_that_changes_something_checks_the_calling_window` e as 6 chaves em `secrets::INTERNAL_KEYS`).
- **Rotas implementadas em `relay_client.rs`** (timeouts explícitos via `reqwest`, headers `Authorization: Bearer <access>` e `X-Lumen-Join: <room>`, sem jamais logar cabeçalhos, token ou payloads — preservando `the_source_never_logs_or_prints`):
  - `mailbox_post(target, creds, msg_id, sealed_bytes)` → `POST /v1/mailbox/<room>/messages?to=phone` com `Content-Type: application/octet-stream` e `X-Lumen-Msg-Id: <msg_id>`. Aceita `200`/`201`.
  - `mailbox_list(target, creds, after_seq, limit)` → `GET /v1/mailbox/<room>/messages?for=pc&after=<seq>&limit=100` (clamp `1..=100`). Decodifica `payloadB64` (base64 padrão).
  - `mailbox_ack(target, creds, upto_seq)` → `POST /v1/mailbox/<room>/ack?for=pc` com JSON `{"uptoSeq": upto_seq}`.
  - `mailbox_stats(target, creds)` → `GET /v1/mailbox/<room>/stats`, retornando `pending_pc`, `pending_phone`, `stored_bytes`, `revoked`, `free_plan`.
- **Tratamento explícito de status HTTP (`MailboxHttpError`):**
  - `401` (credencial/Bearer inválido), `403` (`X-Lumen-Join` incompatível), `410` (sala revogada — interrompe envio automático e marca estado revogado), `413` (envelope excede 66.000 B — marca erro sem retry infinito), `429` (rate limit — respeita backoff exponencial), `507` (`mailbox_full` — pausa envio e expõe aviso claro ao usuário), além de erro de rede/timeout.

## 5. Estado e motor de sync (`windows/src-tauri/src/tasks/sync.rs` + `Todo/sync.json`)

### 5.1 Persistência local atômica (`Todo/sync.json`)
- Armazenado via `write_atomic` ao lado de `Todo/todo.json`. Nunca guarda `K`, `access` ou `room`.
- Campos principais:
  - `schema_version: u32` (`1`)
  - `device_id: String`: UUID v4 gerado uma única vez via `ring::rand::SystemRandom` na primeira inicialização e mantido estável entre reinícios.
  - `last_pc_seq: u64`: último `seq` já aplicado/quarentenado e persistido em disco.
  - `outbox: Vec<OutboxEntry>`: fila FIFO ordenada (`envelope: SyncEnvelope`, `queued_at`, `attempts`).
  - `applied_mutations: Vec<AppliedMutation>`: registro por `mutation_id` para deduplicação idempotente.
  - `base_copies: BTreeMap<String, TodoItem>` + `extra_fields: BTreeMap<String, BTreeMap<String, Value>>`: cópia-base comum para merge de 3 vias e preservação de campos desconhecidos de outros clientes sem quebrar os literais de `TodoItem`.
  - `conflicts: BTreeMap<String, TaskConflict>`: conflitos pendentes de revisão humana (`pending_review`).
  - `quarantine: Vec<QuarantinedEnvelope>`: envelopes com `contract_version > 1` ou `entity_type != "task"` salvos localmente e confirmados com `ack`.
  - `poison_count: u32`, `pending_phone: Option<u64>`, `revoked: bool`, `last_sync_at: Option<String>`, `last_error: Option<String>`.

### 5.2 `SyncEnvelope` v1 e enfileiramento na `outbox`
- Estrutura exata do contrato:
  `contract_version: 1`, `mutation_id` (UUID v4 preservado entre reenvios e enviado em `X-Lumen-Msg-Id`), `device_id`, `origin_role: "pc"`, `entity_type: "task"`, `entity_id`, `base_version`, `new_version`, `updated_at` (RFC 3339 UTC com `Z`), `payload` (espelho de `TodoItem` v1 + eventuais campos desconhecidos em `extra_fields`).
- **Garantia local-first:** toda mutação local em `TaskStore` (`create`, `update`, `toggle_done` — inclusive emitindo 2 envelopes quando gerar `next_occurrence` —, `snooze`, `delete`, `restore`, `add_subtask`, `toggle_subtask`, `remove_subtask`, além das ações de toast em `handle_toast_action`) incrementa `item.version`, grava `todo.json` **e** enfileira o `SyncEnvelope` em `Todo/sync.json` sob o mesmo lock antes de acordar o worker de rede. A UI nunca bloqueia esperando rede.

### 5.3 Ciclo de sincronização (`pull` → aplicação → `ack` → `push` → `stats`)
1. Se não houver relay ativo (`!phone_relay`, URL inválida, sem pareamento ou `revoked`), o ciclo não faz HTTP; tudo permanece salvo em `todo.json` + `outbox`.
2. **Pull:** `mailbox_list` a partir de `last_pc_seq` em páginas de até `100` mensagens.
3. **Abertura e aplicação:**
   - Mensagem que falha em `open_mailbox` ou JSON inválido do `SyncEnvelope` é contabilizada em `poison_count` (sem logar conteúdo ou payload) e entra no `ack` para não trancar a fila.
   - `contract_version > 1` ou `entity_type != "task"` vai para `quarantine` em `sync.json` e entra no `ack`.
   - `mutation_id` já presente em `applied_mutations` ou originado do próprio `device_id` é ignorado de forma idempotente.
   - Aplicação e atualização de `base_copies`/`applied_mutations`/`conflicts`/`todo.json` acontecem de forma atômica antes do `ack`.
4. **Ack contíguo:** `mailbox_ack` é enviado somente até o maior `seq` contíguo já persistido com sucesso.
5. **Push:** envia itens da `outbox` em ordem FIFO com `X-Lumen-Msg-Id = envelope.mutation_id`. Remove da `outbox` somente após `200`/`201`. Erros `429`/rede param o lote e aplicam backoff; `401`/`403`/`410` pausam o sync automático e sinalizam na UI.
6. **Stats:** após esvaziar a `outbox` (ou ao final do ciclo com rede), consulta `mailbox_stats` para atualizar `pending_phone`. Somente com `outbox.is_empty()` **e** `pending_phone == Some(0)` **e** ausência de erro/revogação é que o estado pode ir para `"Sincronizado"`.

### 5.4 Reconciliação de 3 vias, conflitos, alarmes e recorrência
- **Sequencial:** quando a tarefa não existe localmente, cria e define `base_copy = remote`. Quando `remote.base_version == local.version` (sem alteração local concorrente), aplica `remote` e atualiza `base_copy = remote`.
- **Concorrente (`remote.base_version != local.version`):** compara `(base, local, remote)`:
  - Tombstones (`deleted_at.is_some()`, mantidos >= 90 dias): exclusão vence sobre edição (`deleted_at` menor desempata se ambos excluíram); restauração (`deleted_at: None` com versão mais nova após o mesmo `deleted_at`) é respeitada.
  - `done: true` vence sobre `false`; se ambos `true`, menor `done_at` vence.
  - `snoozed_until`: maior timestamp vence.
  - `subtasks`: reconciliadas por `id` (adição/remoção em apenas um lado frente a `base` é preservada; `done` verdadeiro vence; alteração concorrente no mesmo `text` de subtarefa segue regra de conflito).
  - Campos escalares não textuais (`due`, `due_date`, `due_time`, `time_zone`, `alert_minutes`, `priority`, `recurrence`, `series_id`): LWW por `updated_at` UTC da envelope (`device_id` lexicograficamente maior desempata empate exato).
  - **Texto livre (`item.text` ou `subtask.text` editados dos dois lados de modo diferente vs. `base`):** LWW define o vencedor imediato no `TodoItem`, mas registra `TaskConflict` (`conflict_state: "pending_review"`, mantendo valor local e remoto + timestamps) em `sync.json` para escolha explícita do usuário.
  - Quando a fusão produz um estado diferente do remoto recém-chegado, um novo `SyncEnvelope` com `base_version = remote.new_version` e `new_version = max(local.version, remote.new_version) + 1` é enfileirado na `outbox`.
- **Resolução (`tasks_resolve_conflict`):** comando Tauri recebe `task_id` e escolha (`"local" | "remote"`), atualiza `TodoItem`, remove o `TaskConflict` e enfileira uma mutação normal de `update` na `outbox`.
- **Regras de alarme e recorrência (seção 7.3 do contrato):**
  - Conclusão recebida do celular **nunca** gera `next_occurrence` (apenas o dispositivo que concluiu gera a próxima ocorrência).
  - Sempre que uma mutação remota marcar `done = true`, definir `deleted_at` ou alterar/definir `snoozed_until`, chamar `dismiss_if_active(app, task_id)`: limpa entrada em `TaskManager.fired`, fecha toast via `clear_task_toast` e emite evento `"task-dismiss"` (com `task_id`) para dispensar qualquer overlay de lembrete aberto na Ilha (`views/tasks.ts`).

## 6. UI TypeScript e comandos (`windows/src/`)

- **Comandos Tauri em `tasks/mod.rs` (registrados em `lib.rs`):**
  - `tasks_sync_now`: acorda/executa ciclo imediato e retorna `TasksSnapshot` atualizado.
  - `tasks_resolve_conflict(id, choice)`: resolve conflito pendente e retorna `TasksSnapshot` atualizado.
- **Contrato TS (`core/tasks.ts` e `core/bridge.ts`):**
  - Estender `TasksSnapshot` com `sync: TaskSyncSnapshot` (`status: "local_only" | "pending_sync" | "waiting_phone" | "synced" | "error" | "revoked"`, `pendingLocal`, `pendingPhone`, `lastSyncAt`, `lastError`, `quarantinedCount`, `poisonCount`, `conflicts`).
- **Regras visuais em `hub/pages/tasks.ts`:**
  - Exibir pílula de status inequívoca ao lado do botão **"Sincronizar agora"**:
    - `"Sincronizado"` **somente** quando `status === "synced"` (`pendingLocal === 0 && pendingPhone === 0`).
    - `"Salvo neste PC"` em todos os demais casos, detalhando motivo quando útil (ex.: sem relay, pendências na outbox local, alterações aguardando o celular no relay, erro de rede ou sala revogada).
  - Bloco/cartão inline para conflitos em `pending_review` exibindo texto deste PC vs. texto do celular com botões de escolha que chamam `tasks_resolve_conflict`.
  - Frases novas em pt-BR (usando `windows/src/i18n/extra.json` se integradas ao i18n existente, ou diretas conforme padrão da página `tasks.ts`).

## 7. Segurança, privacidade e critérios de verificação

- Segredos permanecem exclusivamente no Credential Manager (`secrets.rs`); `sync.json` armazena apenas `device_id`, fila cifrável, cópias-base, metadados de versão e conflitos.
- Nenhum log, `println!`, `dbg!` ou mensagem de erro na UI expõe texto da tarefa, token, `K`, ou conteúdo da caixa de correio.
- **Critérios de verificação da entrega L6.3 (todos verdes na execução do plano):**
  1. `cargo test --lib` em `windows/src-tauri/` (incluindo vetores de `test-vectors-mailbox.json`, envelopes de `contrato-sync-v1-task.json`, servidor fake loopback HTTP e guardas de não-logging).
  2. `npx tsc --noEmit` em `windows/`.
  3. `npm test` em `windows/` (incluindo `windows/tests/tasks.test.mjs`).
  4. `npm test` em `android/relay/`.
  5. Validação contra Worker Cloudflare real e Galaxy S25 Ultra fica explicitamente marcada como **`NOT_RUN`** nesta etapa da ponte (ocorre após L6.4 + F12-04 no checkpoint de integração real).
