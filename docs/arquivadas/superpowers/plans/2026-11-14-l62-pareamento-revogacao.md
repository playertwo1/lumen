# Plano — L6.2: Pareamento QR + Revogação da Caixa de Correio no Lumen

**Spec:** [2026-11-14-l62-pareamento-revogacao.md](../specs/2026-11-14-l62-pareamento-revogacao.md)  
**Branch prevista:** `feat/l62-pareamento-revogacao` (criar apenas após aprovação do usuário)

---

## Pré-condição antes de codar

1. Aguardar aprovação explícita do dono sobre a spec e este plano.
2. Rodar `git status --short --branch` na cópia principal do repositório; se estiver limpo, criar `feat/l62-pareamento-revogacao` a partir da `main`.

---

## Etapas de implementação

### 1. Derivação `k_mailbox` (`windows/src-tauri/src/phone_link/relay_crypto.rs`)
- Adicionar `pub fn k_mailbox(k: &PairingKey) -> Secret32` usando `derive32(k, &[], "mailbox")` (`HKDF-SHA256(K, "", "coucou-relay/v1 mailbox")`).
- Nos testes de `relay_crypto.rs`, carregar `../../../../android/relay/test-vectors-mailbox.json` com `include_str!` e validar `k_mailbox(&PairingKey::from_bytes(...))` contra o campo `kMailbox` do vetor.

### 2. Revogação HTTP em `windows/src-tauri/src/phone_link/relay_client.rs`
- Adicionar `Target::http_base_url(&self) -> String` convertendo `wss` → `https` e `ws` (loopback) → `http`, omitindo portas padrão (`443` / `80`).
- Adicionar `pub(super) fn existing_credentials(store: &dyn SecretStore) -> Option<Credentials>` que apenas lê e valida `(ROOM_KEY, PAIRING_KEY, ACCESS_KEY)` já armazenados, sem criar sala nova se faltarem.
- Adicionar `pub(super) async fn revoke_mailbox(target: &Target, creds: &Credentials) -> Result<u16, String>` enviando `POST <http_base_url>/v1/mailbox/<room>/revoke` via `reqwest::Client` (timeout de 5 s) com:
  - `Authorization: Bearer <creds.access>`
  - `X-Lumen-Join: <crypto::join_proof(&creds.key)>`
- Preservar a regra do módulo: nenhum log nem retorno contendo `creds.room`, `creds.key` ou `creds.access`.

### 3. Fluxo `"Parear de novo"` e visibilidade do relay (`windows/src-tauri/src/phone_link/mod.rs`)
- Em `relay_status_of`: tornar `shown: true` por padrão (atualizar comentário e remover dependência exclusiva de `COUCOU_RELAY_SETTINGS=1`).
- Ajustar `renew_pairing` e `phone_link_new_pairing`:
  - Tornar `phone_link_new_pairing` `pub async fn` (compatível com Tauri e com `await Bridge.phoneLinkNewPairing()` no frontend).
  - Na sequência exata:
    1. `only_settings(&window)?` e checagem de `running`.
    2. `link.hub.kick_all("auth", "unpaired")` primeiro e incondicionalmente.
    3. Parar `link.relay` ativo, capturar `(url, old_creds)` via `existing_credentials` e `shared.settings`, soltar todos os `MutexGuard` antes do `.await`.
    4. Se houver `Target::parse(&url)` e `Some(old_creds)`, aguardar `relay_client::revoke_mailbox(&target, &old_creds).await` (tratando `200`/`410` como confirmação e não bloqueando a rotação local em caso de erro de rede/HTTP).
    5. Executar rotação local (`pairing::new_token` + `relay_client::new_pairing`) — mantendo `renew_pairing(&hub, &token, &store)` ou helper equivalente síncrono para preservar os invariantes testados em `tests.rs` quando o cofre recusa escrita.
    6. Gerar novo `Pairing` via `pairing_of` e chamar `link.sync_relay()`.
- Atualizar `windows/src/settings/phone.ts` apenas nos comentários se necessário (o UI já exibe QR, `Pair again`, URL e chave `password` assim que `status.shown === true`).

### 4. Testes (`windows/src-tauri/src/phone_link/tests.rs` + `windows/tests/phone-relay.test.mjs`)
- Em `phone_link/tests.rs`:
  - Teste com servidor HTTP loopback (`TcpListener` em `127.0.0.1:0`) provando que a chamada de revogação envia `POST /v1/mailbox/<old_room>/revoke` com `Authorization: Bearer <access>` e `X-Lumen-Join: <join_proof(&old_key)>`, e que o fluxo rotaciona para novos `room`, `K` e `token`.
  - Teste cobrindo resposta `410 Gone` e falha de rede (endereço fechado) garantindo que a rotação local de `room`, `K` e `token` conclui normalmente.
  - Teste cobrindo `pairing_link_with_relay` + `qr_svg` com formato da seção 2 de `docs/RELAY_LINK.md` e ausência de vazamento em `Debug`.
- Em `windows/tests/phone-relay.test.mjs`:
  - Atualizar o teste de visibilidade do bloco de relay para refletir `shown: true` por padrão, mantendo os testes do campo `password` e da não-exposição de `access`.

### 5. Roadmap e estado (`docs/arquivadas/etapas/l6-sync-segunda-mente.md` + `docs/STATE-FORK.md`)
- Corrigir `L6.2`–`L6.4` em `docs/arquivadas/etapas/l6-sync-segunda-mente.md`, removendo `(Aguardando aprovação de F12-01.)` e marcando `L6.2` conforme o resultado dos testes (e apontando para `segundamente/docs/lumen/PONTE.md`).
- Atualizar `docs/STATE-FORK.md` ao concluir o Passo 5 (`L6.2`).

---

## Verificação prevista (após implementação)

- `cargo test --manifest-path windows/src-tauri/Cargo.toml phone_link`
- `cd windows && npx tsc --noEmit`
- `cd windows && npm test`
- `cd android/relay && npm test`
