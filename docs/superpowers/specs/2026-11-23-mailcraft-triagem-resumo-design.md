# Spec — Reabertura pontual L5: Triagem completa, Resumo rápido legível e Master-Detail no MailCraft

**Data:** 2026-11-23  
**Escopo autorizado:** Somente triagem (`mail_classify` / `mailCraftTriageCard`) e resumo rápido (`mail_summarize_inbox` / `renderSummaryBody`) do MailCraft no Hub Windows (`integration_mail`). Outras etapas pausadas permanecem fechadas.

## 1. Problema

1. Aumentar apenas `limit` para `200` na chamada `mail.list` não garante análise de todas as mensagens (ex.: ~54 e-mails):
   - `mailcraft::message_detail` abria uma conexão TCP+HMAC (ou spawn `mailcraft-cli mcp`) por mensagem;
   - `mail_classify` enviava todos os itens pendentes num único prompt com `max_tokens = 1500` e `timeout = 60s`, truncando o JSON antes de `]` e fazendo `classify::parse` descartar todas as classificações da IA e cair no fallback local por palavras-chave;
   - `mail_summarize_inbox` lia corpo apenas para `idx < 8 || msg.unread` e limitava a resposta a `max_tokens = 700` e `45s`, gerando resumos incompletos ou cortados em caixas com 54+ mensagens.
2. Na interface ([`windows/src/hub/pages/mail.ts`](file:///C:/Users/notefael/projetos/coucou/windows/src/hub/pages/mail.ts)):
   - `renderSummaryBody` renderizava `**...**` e `###` literais e desassociava parágrafos/títulos das listas ao empurrar todos os bullets para uma única `<ul>` no fim;
   - A página empilhava cards numa coluna esquerda longa (~3.800 px para 54 itens) deixando vazio morto na coluna direita, com `Abrir MailCraft` deslocado dentro do card de resumo, 2 CTAs `primary` concorrentes, excesso de botões por linha (`.mail-row-more`), perda de foco ao remover linha e resquícios de texto `"Coucou"` / inglês.

## 2. Desenho

### 2.1 Pipeline Rust ([`windows/src-tauri/src/mail/`](file:///C:/Users/notefael/projetos/coucou/windows/src-tauri/src/mail))

- **Sessão única para detalhes em lote (`mailcraft.rs`):**
  - Nova função `message_details` / `message_details_with_key_dir(cfg, ids, max_chars, key_dir)` que abre uma única sessão `Transport`, lê `mail.get` para cada ID válido (tolerando falhas pontuais sem abortar o lote) e encerra ao final.
  - Preserva integralmente a allowlist (`ALLOWED_COMMANDS`) e proibições de envio (`compose.send` / `send_message`).
- **Reaproveitamento da leitura em memória (`mod.rs`):**
  - Cache curto em `Runtime` (`inbox_cache`, TTL 90 s, vinculado ao `BackendConfig` ativo e `limit` pedido) armazenando `(messages, unread, total_known, details)`.
  - `mail_classify` e `mail_summarize_inbox` reaproveitam esse snapshot quando válido, evitando releituras repetidas quando o usuário aciona um após o outro. O cache é invalidado/atualizado em `mail_act` (para `delete`, `junk`, `undelete`, `unjunk`, `read`, `flag`, `unflag`), `mail_config_set` e `mail_refresh`.
  - `mail_act` no modo MailCraft também sincroniza `rt().classified` para não exibir itens já removidos ao revisitar a aba.
- **Triagem em lotes + parser resiliente (`classify.rs` + `mod.rs`):**
  - `mail_classify` processa `ask` em lotes de até `25` mensagens por chamada à IA (`max_tokens = 2800`, `timeout = 90s` por lote), mapeando índices locais `#1..#k` para os índices reais da lista. Se um lote falhar ou truncar, o resultado parcial é preservado e `note` informa quantos itens precisaram de palavras-chave locais.
  - `classify::parse` aceita cerca de código (` ```json `) e consegue extrair objetos `{...}` completos mesmo quando um array JSON truncou antes do `]` final.
- **Resumo completo da caixa (`mod.rs`):**
  - Inclui todas as mensagens listadas (até 200): trechos de até `400` caracteres para as 15 primeiras ou não lidas/sinalizadas e trechos compactos (`160` caracteres em linha única) para as demais, respeitando um teto seguro de prompt (`~48.000` caracteres).
  - Aumenta orçamento de saída para `max_tokens = 2000` e `timeout = 90s` e orienta estrutura clara em Markdown (`##` seções, listas `-`, negrito moderado apenas em remetente/assunto), sinalizando truncamento se `list.len()` atingir o teto de `200` com `total > 200`.

### 2.2 Interface Master-Detail, Tipografia, Polimento e Animação (`mail.ts` + `hub.css`)

- **Layout Master-Detail:**
  - Barra/card compacto `Conexão e provedor` no topo: verificação automática ao abrir no modo MailCraft, troca de provedor com `redraw()`, botão `"Abrir MailCraft"`, `"Testar conexão"` e seção `"Avançado"` recolhida (transporte, porta salva no `change`/`blur`, comando MCP), além de atalhos discretos com contagem para VIPs, Palavras proibidas e Regras.
  - Grade `.mail-workbench`: **esquerda** (`Triagem do MailCraft` quando ativo, com filtro `"Todas (N)"` + tiles existentes, grupos de baixo sinal `Promoções` e `Spam` recolhíveis por padrão com `"Limpar grupo (N)"` visível); **direita `sticky`** (`Resumo rápido da caixa` + `Preparar rascunho` com suporte a `replyToId`, validação de campos e botão `"Limpar"`). Apenas 1 botão `primary` sólido por área principal.
- **`typeset` (`renderSummaryBody`):**
  - Reaproveita [`parseMarkdown`](file:///C:/Users/notefael/projetos/coucou/windows/src/views/markdown.ts#L35) e [`parseInline`](file:///C:/Users/notefael/projetos/coucou/windows/src/views/markdown.ts#L131) normalizando bullets `•`/`–` para `-`, preservando a ordem exata de títulos (`h3`/`h4`), parágrafos (`45–64ch`), listas agrupadas (`ul`/`ol`), negrito/itálico/código em nós DOM (`textContent`, sem `innerHTML`, sem transformar links de e-mail em `<a>` clicáveis). `role="status"`/`aria-live="polite"` fica restrito a `summaryMeta` (que inclui horário de geração).
- **`polish` + `harden` + `animate`:**
  - Máximo de 2 controles em repouso por linha na triagem + menu `"⋯"` acessível (`Responder em rascunho`, `Abrir no MailCraft`, `Marcar lida`, `Importante`, `Mover para Spam`, `Apagar`).
  - Remove timeout de 4 s em `Apagar?`/`Spam?` (cancela com `Esc`/desfoco); restaura foco no botão `Desfazer` após remover linha; feedback visual imediato em `Marcar lida`/`Importante`; botão `"Cancelar"` durante exclusão em lote.
  - Remove textos `"Coucou"` / inglês residuais na superfície Fase L em pt-BR, corrige contraste de `.mail-triage-reason` em hover/foco e ajusta `cursor` de botões desabilitados.
  - Animações suaves respeitando `prefers-reduced-motion` (revelação contida do resumo, stagger curto nas linhas visíveis e saída suave sem salto).

## 3. Aceite e Rollback

- **Aceite sintético/código:** `classify::parse` recupera objetos parciais; `message_details` lê múltiplas mensagens numa única sessão; `mail_classify` divide acima de 25 mensagens sem perder dados; `renderSummaryBody` converte Markdown em DOM estruturado sem `**`/`###` literais e sem `innerHTML`; `impeccable detect --json --changed` sem achados bloqueantes.
- **Aceite real (pelo dono):** triagem e resumo rápido cobrem todas as ~54 mensagens reais da caixa de entrada do MailCraft sem fallback silencioso nem markdown quebrado.
- **Rollback:** alterações restritas a [`windows/src-tauri/src/mail/`](file:///C:/Users/notefael/projetos/coucou/windows/src-tauri/src/mail), [`windows/src/hub/pages/mail.ts`](file:///C:/Users/notefael/projetos/coucou/windows/src/hub/pages/mail.ts) e [`windows/src/hub/hub.css`](file:///C:/Users/notefael/projetos/coucou/windows/src/hub/hub.css), sem migração de banco ou alteração de IPC quebrando contratos existentes.
