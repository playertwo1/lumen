# Spec — L6: Sync com a Segunda Mente (Contrato `F12-01` + L6.1 Caixa de Correio Persistente)

**Data:** 2026-11-14  
**Branch:** `feat/l6-sync-segunda-mente`  
**Contrato canônico referenciado:** `C:\Users\notefael\projetos\segundamente\docs\lumen\CONTRATO-SYNC.md` (`F12-01`)  
**Escopo desta spec:** Toda a visão da Fase L6 (`L6.1`–`L6.4` + animação P3) com implementação imediata restrita à **L6.1** (caixa de correio persistente no relay) enquanto o proprietário revisa o contrato `F12-01`.

---

## 1. Mapa de Entrega da Fase L6

- **L6.1 (implementado nesta etapa):** Caixa de correio persistente E2EE no Worker do link (`android/relay/` — Cloudflare Worker + Durable Object `Room` com SQLite já declarado em `new_sqlite_classes = ["Room"]`), mantendo o `relay/` do Mac/APNs intacto e o canal WebSocket live `/v1/room/<room>` retrocompatível (sem gravações quando só o canal live é usado).
- **L6.2 (após aprovação do contrato `F12-01`):** Pareamento no Lumen com a Segunda Mente Android (`ANDROID_LINK.md` / `RELAY_LINK.md`) + `K_mailbox` + revogação real chamando `/revoke`.
- **L6.3 (após aprovação de `F12-01`):** Outbox atômica em disco no Lumen, aplicação idempotente por `mutation_id`, tombstones, detecção de conflito explícito em texto livre e status visível `"salvo neste PC"` separado de `"sincronizado"`.
- **L6.4 + Animação P3 (após aprovação de `F12-01`):** Sync de tarefas (cancelamento cruzado de Toast/alarme), notas do Anotador, comentários/conversas, personas, preferências e memória resumida (`criar`/`corroborar`/`refinar`/`corrigir`) + animação P3 das duas bolinhas na bússola Lumen diferenciando `"salvo neste PC"` de `"sincronizado"`.

---

## 2. Arquitetura L6.1 — Caixa de Correio Persistente em `android/relay/`

O projeto já possui em `android/relay/` o Worker `coucou-link` com Durable Object SQLite (`Room`), autenticação `ACCESS_KEY`, limitador por IP (`CONNECT_LIMIT`) e derivação de `K_join` (`docs/RELAY_LINK.md`). A L6.1 estende exatamente esse Worker:

### 2.1 Rotas HTTP da Caixa de Correio (`/v1/mailbox/<room>/*`)

Todas as rotas `/v1/mailbox/<room>/...`:
1. Validam `room` (22 chars base64url) e método HTTP **antes** de contatar qualquer Durable Object (retornando `404`/`405` barato).
2. Aplicam `CONNECT_LIMIT` (quando presente) antes de comparar segredos (`429`).
3. Validam `Authorization: Bearer <ACCESS_KEY>` em tempo constante com SHA-256; falham fechado com `401` quando `ACCESS_KEY` não estiver configurada.
4. Validam `X-Lumen-Join: <43 chars base64url>` (`parseJoinProof` / `B64URL_32`), retornando `400` se ausente ou malformado.
5. Só então roteiam ao Durable Object `ROOM.idFromName(room)`.

Operações dentro do `Room` Durable Object (`ctx.storage.sql`):
- **Schema SQLite inicializado sob demanda** (somente ao receber requisição `/v1/mailbox/`; conexões exclusivas `/v1/room/` continuam sem tocar storage):
  - `mailbox_meta (id INTEGER PRIMARY KEY CHECK (id = 1), join_verifier TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0, next_seq INTEGER NOT NULL DEFAULT 1)`
  - `mailbox_messages (seq INTEGER PRIMARY KEY, recipient TEXT NOT NULL, msg_id TEXT, size_bytes INTEGER NOT NULL, created_at INTEGER NOT NULL, payload BLOB NOT NULL)` + índice único parcial `(recipient, msg_id)` quando `msg_id IS NOT NULL`.
  - `mailbox_daily_usage (day TEXT PRIMARY KEY, writes INTEGER NOT NULL DEFAULT 0, reads INTEGER NOT NULL DEFAULT 0, enqueued_bytes INTEGER NOT NULL DEFAULT 0)`
- **Verificação de sala (`join_verifier` & `revoked`):**
  - Na primeira operação `/v1/mailbox/` da sala (exceto `POST`), se ainda não há metadados e for `POST /messages`, grava `join_verifier = SHA-256(K_join)`.
  - Se a sala já estiver marcada `revoked = 1`, responde `410 Gone`.
  - Se `join_verifier` salvo diferir do `SHA-256(K_join)` atual, responde `403 Forbidden` (`join proof`).
- **Endpoints:**
  - `POST /v1/mailbox/<room>/messages?to=pc|phone`
    - Corpo bruto binário `application/octet-stream` opaco (mín. 1 byte, máx. `MAX_FRAME = 66_000` bytes; excesso -> `413`).
    - Header opcional `X-Lumen-Msg-Id` (`[A-Za-z0-9_-]{8,64}`): se já existir para aquele `recipient`, devolve `200 OK` com o `seq` já gravado sem gastar escrita duplicada nem duplicar entrega.
    - Faz limpeza preguiçosa (`DELETE` por TTL > 30 dias) apenas quando necessário antes de checar limites: máx. `500` mensagens pendentes por destinatário e máx. `16 MiB` totais na sala (excesso -> `507 Insufficient Storage`).
    - Insere mensagem, avança `next_seq` e soma no `mailbox_daily_usage` na mesma transação (`201 Created` com `{ seq, id, createdAt, storedMessages, storedBytes, freePlan }`).
  - `GET /v1/mailbox/<room>/messages?for=pc|phone&after=<seq>&limit=<1..50>`
    - Retorna `{ messages: [{ seq, id, createdAt, sizeBytes, payloadB64 }], latestSeq, pendingCount, freePlan }`.
    - Se a sala ainda não tiver sido inicializada (nunca recebeu `POST`), responde `200` com lista vazia **sem** gravar nada no SQLite (0 escritas).
  - `POST /v1/mailbox/<room>/ack?for=pc|phone`
    - Corpo JSON `{ "uptoSeq": number }` (`seq >= 0`).
    - Remove mensagens daquele `recipient` com `seq <= uptoSeq`, atualiza uso diário e retorna `200` com `{ ackedUpto, deleted, remaining, freePlan }`.
  - `POST /v1/mailbox/<room>/revoke`
    - Apaga todas as mensagens da sala, marca `revoked = 1` (ou fixa `join_verifier` revogado na sala), fecha WebSockets ativos (`1008 revoked`) e retorna `200 { revoked: true }`. Chamadas subsequentes retornam `410`.
  - `GET /v1/mailbox/<room>/stats`
    - Retorna contadores da sala (`pendingPc`, `pendingPhone`, `storedBytes`, `revoked`) + medidor diário UTC contra os tetos do plano Free (`DAILY_REQ_LIMIT = 100_000`, `DAILY_DO_WRITE_LIMIT = 100_000`, `ROOM_BYTES_LIMIT = 16 MiB`).

### 2.2 Segurança e Privacidade (Regras Inegociáveis)

- Nenhum `console.log` ou log de payload/chave/IP (`observability.enabled = false`).
- Nenhuma chamada externa `fetch("http...")`.
- Canal live WebSocket `/v1/room/<room>` mantém comportamento exato de `RELAY_LINK.md` (exceto fechar com `1008 revoked` se a sala tiver sido explicitamente revogada).
- Gêmeo Node (`android/relay/tools/dev-relay.ts`) espelha as mesmas rotas e respostas `/v1/mailbox/*` em memória usando `src/protocol.ts` e `src/mailbox.ts` para testes locais e E2E futuros do Lumen/Android.

---

## 3. Verificação e Rollback (`L6.1`)

- Testes em `android/relay/`:
  - `npm run typecheck` (`tsc --noEmit`).
  - `npm test`: suite `workerd` (`vitest.config.ts`) com SQLite real de Durable Objects + suite Node (`vitest.node.config.ts`) + guardas de código-fonte (`node.guards.test.ts`), cobrindo: envio com par offline -> reconexão e entrega única -> `ACK` -> replay idempotente por `X-Lumen-Msg-Id` -> isolamento entre salas e rejeição de `X-Lumen-Join` errado -> revogação (`410`) -> limite de tamanho/sala cheia -> TTL 30d -> medição contra os 100k limites do plano Free -> canal live `/v1/room/` sem escritas em storage.
- **Rollback:** Como L6.1 mantém `/v1/room/<room>` intacto e não toca o código do app Windows ainda, basta reverter o commit em `android/relay/` caso o dono deseje mudar o endpoint antes da L6.2.
