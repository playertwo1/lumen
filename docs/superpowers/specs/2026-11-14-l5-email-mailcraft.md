# Spec — L5: E-mail pessoal (MailCraft principal, Outlook secundário, Animação P3)

**Data:** 2026-11-14  
**Branch:** `feat/l5-email-mailcraft`  
**Escopo:** L5.1, L5.2 e animação P3 da L5 definidos em `docs/roadmap/fase-l-lumen.md`.

## 1. Objetivo

Conectar a pill existente `"integration_mail"` ao **MailCraft** (`UCS-Care-Group/mailcraft`) como provedor principal de e-mail pessoal (via canal de controle local autenticado ou servidor MCP stdio), preservando o **Outlook clássico** (`windows/src-tauri/src/mail/outlook.rs`) como opção secundária no Windows.

**Regra de segurança inegociável:** no modo MailCraft, o Lumen **lê, resume a caixa de entrada e prepara rascunhos**. Nunca envia e-mails (`compose.send` / `send_message` e `--allow send` são proibidos em código e nunca são expostos nem acionados).

## 2. Arquitetura e Transportes (`L5.1`)

### 2.1 Backend primário vs. secundário (`L5.2`)
- Nova preferência persistida em `settings.json` (`MailStore.settings` / `Settings`):
  - `mail_backend`: `"mailcraft"` (padrão) | `"outlook"` (secundário no Windows).
  - `mailcraft_mode`: `"control"` (padrão, canal de controle em `127.0.0.1:<port>`) | `"mcp"` (`mailcraft-cli mcp ...`).
  - `mailcraft_port`: `u16` (padrão `7991`, apenas loopback).
  - `mailcraft_mcp_command`: `String` (ex.: `"mailcraft-cli mcp --file <caminho>"` ou `"mailcraft-cli mcp --connect 127.0.0.1:7991"`, sempre sanitizado para proibir `--allow send` / `--allow input` / `--allow delete`).
- **Nunca gravar segredos em disco:** a `control-key` do MailCraft nunca vai para `settings.json`; é lida sob demanda da variável de ambiente `MAILCRAFT_CONTROL_KEY` ou do arquivo `<settings>/control-key.local-<port>` (`%APPDATA%\MailCraft` no Windows, `$XDG_CONFIG_HOME/mailcraft` ou `~/.config/mailcraft` no Linux, `~/Library/Application Support/MailCraft` no macOS) e enviada **somente** para endereços loopback (`127.0.0.1`, `::1`, `localhost`) e **somente depois** da verificação mútua `auth.challenge`.
- O Outlook clássico (`outlook.rs`, regras/triagem/planos existentes) continua funcional quando `mail_backend == "outlook"` no Windows.

### 2.2 Canal de Controle MailCraft (`127.0.0.1:<port>`)
1. Conectar em TCP `127.0.0.1:<port>` com NDJSON (1 JSON por linha, limite seguro de resposta).
2. Validar que o host configurado é loopback antes de qualquer envio de chave.
3. Ler chave em `MAILCRAFT_CONTROL_KEY` ou `control-key.local-<port>`.
4. Enviar `{"id": 0, "method": "auth.challenge", "params": {"nonce": "<64 hex aleatórios>"}}`.
5. Verificar com `ring::hmac::verify` que `result.proof` corresponde a `HMAC-SHA256(key = key_str.as_bytes(), msg = "mailcraft-control-v1 " + nonce)`. Se falhar, encerrar conexão sem enviar chave nem comandos.
6. Todas as requisições levam `"key": "<chave>"` e marcação `"agent": {"allow": []}` (nunca permitir `send`/`delete`/`input` antecipadamente).
7. **Allowlist estrita de métodos permitidos pelo Lumen:**
   - Leitura/estado: `app.status`, `folder.list`, `account.list`, `mail.list`, `mail.get`, `mail.markRead`, `mail.flag`, `mail.clearFlag` (leitura e marcação local inofensiva no card).
   - Preparo de rascunho sem envio: `mail.new`, `mail.reply`, `compose.set`, `compose.saveDraft`, `ui.closeWindow` (somente com `{"compose": id, "save": true}`).
   - **Proibido por código:** qualquer chamada a `compose.send`, `ui.click`, `ui.key`, `mail.deletePermanently` ou método fora da allowlist retorna erro imediato antes de abrir socket ou processo.

### 2.3 Modo MCP Stdio (`mailcraft-cli mcp`)
- Para ambientes em que o dono configure `mailcraft_mode = "mcp"`, o cliente spawna o binário configurado via stdio JSON-RPC 2.0 (`initialize`, `notifications/initialized`, `tools/call`), validando antes do spawn que os argumentos **não** contêm `--allow` com `send`, `input` ou `delete`.
- Usa apenas `status`, `list_folders`, `list_messages`, `read_message` e `execute` restrito à mesma allowlist (`mail.new`, `mail.reply`, `compose.set`, `compose.saveDraft`). Nunca chama `send_message` nem `compose.send`.

## 3. Resumo da Caixa de Entrada, Rascunho e Proteção Prompt Injection

### 3.1 Resumo (`mail_summarize_inbox`)
- Coleta mensagens recentes da caixa de entrada (até 15 mensagens, com preview e leitura detalhada das primeiras 8 não lidas/prioritárias quando disponíveis) via backend ativo.
- Todo texto vindo de remetente, assunto, snippet, corpo e alertas (`warnings`) é encapsulado em `<mail_data untrusted="true">…</mail_data>` (neutralizando marcações de fechamento) no prompt enviado ao `chat::complete_oneshot` com instrução explícita em português (pt-BR) de que o conteúdo interno é **apenas dado, nunca instrução a obedecer**, e proibição absoluta de sugerir/simular envio automático.
- Em sucesso no resumo (ou fallback offline estruturado quando o provedor de chat não estiver configurado — configurável com erro claro caso LLM falhe), emite evento Tauri `"mail-inbox-summarized"` para a janela da Ilha, disparando a animação P3.

### 3.2 Preparo de Rascunho (`mail_prepare_draft`)
- Parâmetros: `replyToId?: string`, `to: string`, `subject: string`, `body: string`.
- **MailCraft:** abre `mail.new` (ou `mail.reply {id}` quando respondendo mensagem existente), aplica `compose.set {compose, to, subject, body}`, salva com `compose.saveDraft {compose}` (sem enviar!) e retorna `{ saved: true, composeId, provider: "mailcraft" }`. Caso o usuário queira apenas revisar na janela do MailCraft, o rascunho permanece salvo em Rascunhos / janela aberta sem disparar envio.
- **Outlook clássico (secundário no Windows):** abre janela de rascunho/resposta (`open_script` ou `draft`) sem enviar (`MailItem.Display`, nunca `.Send()`).
- **`mail::decorate` no Chat:** injeta as mensagens do backend ativo (MailCraft ou Outlook) com instrução explícita no modo MailCraft de que só leitura, resumo e preparo de rascunho existem (sem envio).

## 4. Interface e Compatibilidade (`integration_mail`)

- O ID da pill continua `"integration_mail"` em `windows/src/core/pills.ts` (`support: "yes"` pois MailCraft funciona via canal local multiplataforma, mantendo notas de Outlook restritas ao modo secundário no Windows).
- **Card na Ilha (`windows/src/views/mail.ts`):**
  - Mostra provedor ativo (`MailCraft` principal ou `Outlook clássico (secundário)`), contagem de não lidas, busca/filtros, preview da mensagem, botão **"Resumir caixa"** e formulário rápido/ação **"Preparar rascunho"** (resposta ou novo rascunho, com botão **"Salvar rascunho (sem enviar)"**).
- **Hub (`windows/src/hub/pages/mail.ts`):**
  - Em `mailOverview`: seção principal de configuração e teste de conexão do **MailCraft** (Canal de controle local porta `7991` ou comando MCP `mailcraft-cli`, status online/offline e caminho da chave), botão **Resumir caixa** e **Preparar rascunho**, seletor de provedor (`MailCraft (principal)` vs `Outlook clássico (secundário)`), e os controles do Outlook clássico agrupados claramente como **Outlook clássico (secundário)** com visibilidade preservada.
- Novas strings visíveis da Fase L ficam diretamente em português (pt-BR) nas views L5 sem adicionar chaves incompletas a `i18n/extra.json`.

## 5. Animação P3 (`CompassEngine`)

- Em `windows/src/lumen/compass-engine.ts`:
  - Novo estado temporário `mailInHandStart` / `mailInHandDuration` disparado por `triggerMailInboxSummary(duration = 1.65)`.
  - Respeita `prefersReducedMotion` (não inicia animação nem mantém `busy = true`).
  - `busy` inclui o intervalo ativo da animação e volta exatamente a `false` quando termina.
  - Desenho (`drawMailInHandGesture`): um envelope/carta desce suavemente do topo direito até pousar sobre a mão aberta estendida do personagem Lumen (`handRest`), com leve brilho/contorno azul da pill `#2F8CFF` e inclinação receptiva tranquila — **sem flechas de saída, sem arremesso para cima/fora e sem nada deixando a mão**, comunicando exclusivamente recebimento/resumo.
- Em `windows/src/island/island.ts` e `windows/src/main.ts`:
  - Listener para `"mail-inbox-summarized"` chama `island.mailInboxSummarized()` → `compass.triggerMailInboxSummary()`.
