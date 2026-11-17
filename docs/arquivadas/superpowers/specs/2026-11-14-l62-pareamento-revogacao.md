# Spec — L6.2: Pareamento QR + Revogação da Caixa de Correio no Lumen

**Data:** 2026-11-14  
**Branch prevista:** `feat/l62-pareamento-revogacao` (a partir da `main`)  
**Passo na PONTE:** Passo 5 (`segundamente/docs/lumen/PONTE.md`)  
**Referências canônicas:** `docs/RELAY_LINK.md` (seções 2 e 10), `docs/ANDROID_LINK.md`, `segundamente/docs/lumen/CONTRATO-SYNC.md` (v1)

---

## 1. Objetivo

Completar no Lumen desktop (`windows/`) o pareamento via QR code e a revogação remota na caixa de correio persistente entregue na L6.1 (`android/relay/`):

1. Exibir o QR de pareamento no formato completo da seção 2 de `docs/RELAY_LINK.md` (`relay`, `room`, `key`, `access`) e guardar os segredos exclusivamente no cofre do Windows (`Credential Manager` via `windows/src-tauri/src/secrets.rs`).
2. Ao acionar **"Parear de novo"** (`phone_link_new_pairing`), revogar a sala anterior na caixa de correio via `POST /v1/mailbox/<room>/revoke` antes de gerar novos `room` e `K`.
3. Disponibilizar `k_mailbox(k)` em `relay_crypto.rs` alinhado ao vetor `kMailbox` de `android/relay/test-vectors-mailbox.json`.
4. Atualizar as linhas `L6.2`–`L6.4` em `docs/arquivadas/etapas/l6-sync-segunda-mente.md` que ainda dizem `(Aguardando aprovação de F12-01.)`.

**Fora de escopo neste passo:** envio/aplicação de mutações (`outbox`, tarefas via mailbox, toast cruzado e `device_id` do envelope de sync), que pertencem ao Passo 7 (`L6.3`) e Passo 9 (`L6.4`). Seção 9 (`v1.1`) do contrato continua fora do escopo da ponte.

---

## 2. O que já existe e o que falta

| Item | Estado atual em `windows/src-tauri/src/phone_link/` | Lacuna a fechar na L6.2 |
|---|---|---|
| Formato do QR/link (`coucou://pair?v=1&…&relay=…&room=…&key=…&access=…`) | Já implementado em `pairing::pairing_link_with_relay` + `pairing::qr_svg` e chamado por `pairing_of` quando `phone_relay = true`, URL válida e credenciais no cofre | Garantir cobertura em teste unitário do formato completo com `relay`, `room`, `key`, `access` |
| Cofre de segredos | `phone-relay-room`, `phone-relay-key`, `phone-relay-access`, `phone-link-token`, `phone-link-cert`, `phone-link-key` já registrados em `secrets::INTERNAL_KEYS` (6 chaves) e protegidos em `tests.rs` | Manter exatamente essas chaves apenas no Credential Manager, sem gravação em `settings.json` ou logs |
| Bloco de relay na UI (`Configurações → Celular Android`) | `relay_status_of` (`mod.rs`) só coloca `shown: true` quando `COUCOU_RELAY_SETTINGS=1` ou `s.phone_relay == true` | Expor `shown: true` por padrão para o usuário do Lumen configurar URL e `access` sem variável de ambiente oculta |
| Derivação `K_mailbox` | `relay_crypto.rs` tem `k_auth` e `k_join` (`join_proof`), sem `k_mailbox` | Adicionar `k_mailbox(k: &PairingKey) -> Secret32` (`HKDF-SHA256(K, "", "coucou-relay/v1 mailbox")`) e validar contra `android/relay/test-vectors-mailbox.json` |
| Revogação em `"Parear de novo"` | `phone_link_new_pairing` / `renew_pairing` derruba clientes LAN e gera novo `token`, `room` e `K`, mas **não** chama o Worker | Antes de sobrescrever `room` e `K`, enviar `POST /v1/mailbox/<old_room>/revoke` quando houver URL e credenciais anteriores |

---

## 3. Fluxo detalhado de revogação e pareamento

### 3.1 Pareamento QR (`phone_link_pairing` / `pairing_of`)
- Mantém `pairing_link_with_relay` (seção 2 de `docs/RELAY_LINK.md`):
  `coucou://pair?v=1&host=<lan>&port=<port>&fp=<sha256>&token=<token>&name=<pc>&relay=<wss URL percent-encoded>&room=<22 chars>&key=<43 chars>&access=<43 chars>`
- Se o usuário ainda não habilitou o bloco "Longe do Wi-Fi de casa" ou não salvou URL + chave `access`, o QR continua funcionando em modo LAN e a UI de configurações mostra os campos de URL (`wss://...`) e `access` (`type="password"`, gravação apenas no cofre via `phone_relay_set_access`).

### 3.2 Leitura não-destrutiva de credenciais antigas (`existing_credentials`)
- `relay_client::credentials(store)` cria uma nova sala se `ROOM_KEY` ou `PAIRING_KEY` não existirem ainda.
- Para revogar em `"Parear de novo"`, usar uma função auxiliar `existing_credentials(store) -> Option<Credentials>` que apenas lê e valida `(ROOM_KEY, PAIRING_KEY, ACCESS_KEY)` existentes — evitando gerar e revogar uma sala fantasma caso nunca tenha havido pareamento com relay.

### 3.3 Ordem de execução em `"Parear de novo"` (`phone_link_new_pairing`)
1. Validar origem (`only_settings(&window)`) e garantir que o link está ativo (`running`).
2. Desconectar imediatamente todos os clientes LAN ativos (`hub.kick_all("auth", "unpaired")`) — antes de qualquer I/O de rede ou escrita no cofre.
3. Parar o cliente WebSocket de relay ativo (`link.relay.lock().unwrap().take().stop()`), evitando reconexão/estado espúrio (`RoomTaken` ao fechar com `1008 revoked`).
4. Extrair `phone_relay_url` de `Shared.settings` e `existing_credentials(&*link.store)` (soltando todos os locks de mutex antes de qualquer `.await`).
5. Se existir `Target::parse(&url)` válida e `Some(old_creds)`:
   - Enviar `POST <target.http_base_url()>/v1/mailbox/<old_creds.room>/revoke` com `reqwest` (timeout curto, ex.: 5 s) e cabeçalhos:
     - `Authorization: Bearer <old_creds.access>`
     - `X-Lumen-Join: <relay_crypto::join_proof(&old_creds.key)>`
   - `Target::http_base_url()`: converte `wss://` → `https://` e `ws://` (permitido apenas para `localhost`/`127.0.0.1`/`[::1]`) → `http://`, mantendo porta quando não padrão.
   - `200 OK` ou `410 Gone` encerram a revogação com sucesso.
   - Falha de rede ou outro status HTTP **não** bloqueia a rotação local (o usuário nunca fica preso à chave antiga se estiver sem internet ou com Worker temporariamente fora do ar); registrar apenas mensagem genérica em log sem segredos, sala ou URL.
6. Gerar e salvar novo `token` LAN (`pairing::new_token`) e novos `room` e `K` (`relay_client::new_pairing`). Caso o cofre recuse escrita, retornar erro (mantendo a garantia de que os clientes já foram desconectados).
7. Reconectar o relay (`link.sync_relay()`) com as novas credenciais e retornar o novo `Pairing` (link + `qr_svg`).

---

## 4. Segurança, Privacidade e Testes

- **Nenhum segredo em arquivo ou log:** `K`, `access`, `room`, `token` e `join_proof` nunca aparecem em `settings.json`, logs ou `Debug` (`Credentials(..)` e `PairingKey(..)` permanecem redigidos).
- **Testes exigidos no mesmo commit (regra do projeto):**
  - Rust (`relay_crypto.rs`): `k_mailbox` bate byte a byte com `kMailbox` de `android/relay/test-vectors-mailbox.json`.
  - Rust (`relay_client.rs` / `phone_link/tests.rs`):
    - `Target::http_base_url()` cobre `wss://host`, `wss://host:8443` e `ws://127.0.0.1:8787`.
    - Servidor HTTP loopback (`127.0.0.1`) comprova envio de `POST /v1/mailbox/<old_room>/revoke` com `Authorization: Bearer <access>` e `X-Lumen-Join: <join_proof(&old_key)>`, antes da gravação dos novos `room` e `K`.
    - Resposta `410 Gone` e falha de conexão (porta fechada) ainda concluem a rotação local de `token`, `room` e `K`.
    - `pairing_link_with_relay` com dados sintéticos produz URL válida com `v=1`, `host`, `port`, `fp`, `token`, `name`, `relay`, `room`, `key`, `access`, além de SVG QR não vazio.
  - Node (`windows/tests/phone-relay.test.mjs`): atualizar o teste de visibilidade do bloco de relay (removendo a exigência de `COUCOU_RELAY_SETTINGS=1` para `shown`) mantendo as travas de campo `password` e ausência de retorno de `access`.
  - Regressão completa: `android/relay` (`npm test`), `windows/` (`npx tsc --noEmit` + `npm test`), `windows/src-tauri` (`cargo test --lib phone_link`).
