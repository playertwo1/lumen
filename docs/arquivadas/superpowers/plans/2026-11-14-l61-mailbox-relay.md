# Plano — L6.1: Caixa de Correio Persistente no Relay (`android/relay/`)

**Spec:** [2026-11-14-l6-sync-segunda-mente.md](../specs/2026-11-14-l6-sync-segunda-mente.md)  
**Branch:** `feat/l6-sync-segunda-mente`

## Passos

1. **Extrair regras puras e orçamento Free em `android/relay/src/protocol.ts` + `android/relay/src/mailbox.ts`:**
   - Parse de rotas `/v1/mailbox/<room>/(messages|ack|revoke|stats)` e validação de `to|for` (`pc|phone`), `X-Lumen-Join`, `X-Lumen-Msg-Id`, `after`, `limit` e JSON de `ack`.
   - Estrutura de medição contra o plano Free (`FREE_PLAN_DAILY_REQUESTS = 100_000`, `FREE_PLAN_DAILY_DO_WRITES = 100_000`, `MAX_ROOM_BYTES = 16 MiB`, `MAX_PENDING_PER_ROLE = 500`, `MESSAGE_TTL_MS = 30d`) e cálculo determinístico de percentual utilizado (`writesPct`, `requestsPct`).
   - Armazenamento compartilhável / SQLite helper em `android/relay/src/room.ts` via `ctx.storage.sql` inicializado de forma preguiçosa somente nas rotas `/v1/mailbox/`.
2. **Integrar roteamento no Worker (`android/relay/src/index.ts`) e no gêmeo Node (`android/relay/tools/dev-relay.ts`):**
   - Refusar rotas inválidas (`404`/`405`), aplicar `CONNECT_LIMIT` (`429`), `ACCESS_KEY` (`401`) e `X-Lumen-Join` (`400`) antes de tocar qualquer DO.
   - Espelhar as mesmas rotas HTTP no `dev-relay.ts` usando a mesma lógica pura de `src/protocol.ts` e `src/mailbox.ts` (sem SQLite, em `Map` em memória).
3. **Atualizar guardas e adicionar bateria completa de testes (`android/relay/tests/`):**
   - Ajustar `node.guards.test.ts` permitindo `.sql` apenas no armazenamento de mailbox em `room.ts`, mantendo proibidos `console.`, `KVNamespace`, `R2Bucket`, `D1Database` e `fetch("http...`) e `observability = false`.
   - Cobrir no `workerd` (`worker.relay.test.ts`) e no gêmeo Node (`node.relay.test.ts`) o ciclo completo da caixa de correio persistente, hibernação/evict do Durable Object com SQLite preservado, deduplicação por `X-Lumen-Msg-Id`, revogação `410`, cota do plano Free e isolamento de `/v1/room/`.
4. **Atualizar documentação (`docs/RELAY_LINK.md` + `docs/arquivadas/etapas/l6-sync-segunda-mente.md`):**
   - Documentar endpoints da mailbox L6.1 e registrar estado sintético da L6.1 (mantendo `Checkpoint L6` e `L6.2`–`L6.4` abertos aguardando revisão de `F12-01` e implementação completa).
5. **Validar tudo localmente:**
   - `cd android/relay && npm run typecheck && npm test`
   - Garantir que `windows/` não foi quebrado (`cd windows && npx tsc --noEmit`).
