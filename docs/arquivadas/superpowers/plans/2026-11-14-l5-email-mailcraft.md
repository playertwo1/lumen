# Plano de Implementação — L5: E-mail pessoal (MailCraft + Outlook secundário + P3)

**Data:** 2026-11-14  
**Spec:** [docs/arquivadas/superpowers/specs/2026-11-14-l5-email-mailcraft.md](../specs/2026-11-14-l5-email-mailcraft.md)

## Tarefa 1 — Cliente MailCraft em Rust (`windows/src-tauri/src/mail/mailcraft.rs`)
- Estruturar configuração segura (`MailCraftConfig`: `mode: "control" | "mcp"`, `port: u16`, `mcp_command: String`) e status (`MailCraftStatus`).
- Implementar descoberta de chave `MAILCRAFT_CONTROL_KEY` ou `<settings>/control-key.local-<port>` (`%APPDATA%\MailCraft` / `$XDG_CONFIG_HOME/mailcraft` / `~/.config/mailcraft` / macOS).
- Implementar validação estrita de loopback (`is_loopback_host`) e prova mútua `auth.challenge` usando `ring::hmac` (`HMAC_SHA256`, `"mailcraft-control-v1 " + nonce`, comparação em tempo constante via `ring::hmac::verify`).
- Implementar allowlist estrita de comandos (`is_allowed_command`): permite leitura/listagem/marcação e preparo de rascunho sem envio (`mail.new`, `mail.reply`, `compose.set`, `compose.saveDraft`), bloqueando incondicionalmente `compose.send`, `send_message`, `ui.click`, `ui.key`, `--allow send`, `--allow input`, `--allow delete`.
- Implementar cliente síncrono/async Tokio para Control Channel (NDJSON loopback) e MCP stdio (`mailcraft-cli mcp`, parse JSON-RPC 2.0 `initialize` + `tools/call`).
- Normalizar mensagens (`from`, `subject`, `date`, `read`, `flag`, `attachments`, `snippet`, `body`, `warnings`) removendo frames `<<untrusted ...>>` na leitura estruturada e re-encapsulando em `<mail_data>` quando enviado ao LLM.
- Testes unitários Rust com servidor TCP loopback simulado:
  1. Rejeição quando `auth.challenge` falha (chave nunca é enviada no fio).
  2. Handshake válido + `mail.list` + `mail.get`.
  3. Criação e salvamento de rascunho (`mail.new` / `mail.reply` + `compose.set` + `compose.saveDraft`) sem jamais chamar envio.
  4. Bloqueio incondicional de `compose.send` e de flags `--allow send` no comando MCP.

## Tarefa 2 — Unificação do backend em `windows/src-tauri/src/mail/` (`L5.1` + `L5.2`) e `lib.rs`
- Estender `Store` / `Settings` de `mail/store.rs` e `mail/mod.rs` com `backend` (`"mailcraft"` padrão vs `"outlook"`), `mailcraft` (`mode`, `port`, `mcp_command`) mantendo leitura retrocompatível via `#[serde(default)]`.
- Atualizar `poll`, `mail_refresh`, `mail_search`, `mail_detail`, `mail_act` para rotearem pelo backend ativo (`mailcraft` primário, `outlook` secundário).
- Adicionar comandos Tauri:
  - `mail_config_get` / `mail_config_set`
  - `mail_test_connection`
  - `mail_summarize_inbox` (lê inbox pelo backend ativo, resume com `chat::complete_oneshot` com framing `<mail_data>` e instrução pt-BR, emite evento `"mail-inbox-summarized"`)
  - `mail_prepare_draft` (cria/salva rascunho no MailCraft sem enviar; no modo Outlook secundário no Windows abre janela de rascunho sem enviar)
- Atualizar `mail::decorate` para usar as mensagens do backend ativo (mantendo aviso de somente-rascunho quando em MailCraft).
- Registrar os novos comandos em `windows/src-tauri/src/lib.rs`.
- Testes unitários Rust adicionais cobrindo troca de backend primário/secundário, framing `<mail_data>` anti-injeção e preservação de configurações existentes.

## Tarefa 3 — UI TypeScript (`L5.1`, `L5.2`) e Pills
- Atualizar `windows/src/core/pills.ts`: `"integration_mail"` passa a ter `support: "yes"` (MailCraft funciona em canal local; Outlook permanece indicado como secundário Windows).
- Atualizar `windows/src/core/mail.ts`: tipos `MailBackend`, `MailCraftMode`, `MailCraftConfig`, `MailSummaryResult`, `PreparedDraftResult`, `MailBridge` para os novos comandos (`configGet`, `configSet`, `testConnection`, `summarizeInbox`, `prepareDraft`).
- Atualizar `windows/src/views/mail.ts` (card da ilha): badge discreto do backend (`MailCraft` / `Outlook secundário`), ação **"Resumir caixa"** com exibição do resumo, e modal/painel compacto **"Preparar rascunho"** (novo ou resposta sem enviar).
- Atualizar `windows/src/hub/pages/mail.ts`: painel principal **MailCraft (principal)** (modo controle/MCP, porta, comando MCP, testar conexão, seletor de provedor principal/secundário, resumo da caixa e preparo de rascunho), mantendo triagem automática/regras/VIPs do **Outlook clássico (secundário)** abaixo quando relevante ou selecionado.

## Tarefa 4 — Animação P3 (`CompassEngine`) e ligação de evento
- Atualizar `windows/src/lumen/compass-engine.ts`:
  - `triggerMailInboxSummary(duration = 1.65)` e renderizador `drawMailInHandGesture(ctx, compact)`.
  - Carta pousando na mão aberta (`t: 0 -> 1` descendo suavemente até o repouso na palma, nunca partindo da mão nem sugerindo envio).
  - Respeito a `prefersReducedMotion` e atualização de `busy` / `update(dt)`.
- Conectar `"mail-inbox-summarized"` em `windows/src/main.ts` e `island.mailInboxSummarized()` em `windows/src/island/island.ts`.
- Criar `windows/tests/mailcraft.test.mjs` e ampliar `windows/tests/lumen-character-engine.test.mjs` para cobrir a animação P3 da L5 (`triggerMailInboxSummary`, `busy === false` no fim e sob `reducedMotion`, canvas limpo).

## Tarefa 5 — Validação, Roadmap e Commit
- Rodar `cargo test --lib` em `windows/src-tauri/`.
- Rodar `npx tsc --noEmit`, `npm test` e `npm run build` em `windows/`.
- Atualizar `docs/arquivadas/etapas/l5-email-pessoal.md` e `docs/STATE-FORK.md` (deixando Checkpoint L5 aberto para o teste real do dono com o MailCraft no notebook).
- Commit limpo na branch `feat/l5-email-mailcraft` com `[skip ci]`.
