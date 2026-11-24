# Plano — Triagem completa, Resumo rápido e Master-Detail no MailCraft

**Data:** 2026-11-23  
**Spec:** [`docs/superpowers/specs/2026-11-23-mailcraft-triagem-resumo-design.md`](file:///C:/Users/notefael/projetos/coucou/docs/superpowers/specs/2026-11-23-mailcraft-triagem-resumo-design.md)

## Tarefa 1 — Transporte em lote no MailCraft (`mailcraft.rs`)
- **Arquivo:** [`windows/src-tauri/src/mail/mailcraft.rs`](file:///C:/Users/notefael/projetos/coucou/windows/src-tauri/src/mail/mailcraft.rs)
- **Alterações:**
  - Extrair helper `parse_detail_value(v: &Value, max_chars: usize) -> DetailData` para evitar duplicação entre `message_detail` e `message_details`.
  - Adicionar `message_details` e `message_details_with_key_dir(cfg, ids, max_chars, key_dir) -> Result<HashMap<String, DetailData>, String>` usando uma única sessão `Transport`.
  - Expor `list_inbox_with_meta` (ou struct auxiliar) reportando se algum `mail.list` indicou `total > msgs.len()`, preservando `list_inbox` compatível com chamadores atuais.
  - Adicionar teste unitário com servidor mock validando leitura de múltiplas mensagens numa única conexão TCP.

## Tarefa 2 — Parser resiliente e lotes na Triagem (`classify.rs` e `mod.rs`)
- **Arquivos:** [`windows/src-tauri/src/mail/classify.rs`](file:///C:/Users/notefael/projetos/coucou/windows/src-tauri/src/mail/classify.rs), [`windows/src-tauri/src/mail/mod.rs`](file:///C:/Users/notefael/projetos/coucou/windows/src-tauri/src/mail/mod.rs)
- **Alterações:**
  - Em `classify::parse`: limpar cerca ```json, tentar array completo e, se truncado, varrer objetos `{...}` balanceados respeitando strings/escapes para recuperar vereditos completos até o ponto de corte; aceitar `"n"` numérico ou string numérica.
  - Em `mod.rs`: implementar `InboxSnapshot` em `Runtime` (reuso entre triagem e resumo, invalidado em ações e mudanças de configuração); sincronizar `rt().classified` em `mail_act`.
  - Dividir `ask` em `mail_classify` em lotes de `25` itens (`max_tokens = 2800`, `90s`), somando itens que caíram em `guess` para preencher `note` com transparência caso algum lote falhe/trunque.
  - Em `mail_summarize_inbox` e `build_inbox_summary_prompt`: enriquecer todas as mensagens da caixa (trecho maior nas primeiras/não lidas, trecho compacto nas demais dentro de orçamento seguro), elevar saída para `max_tokens = 2000` e `90s`, e estruturar o prompt em seções Markdown claras em pt-BR.
  - Adicionar testes unitários em `classify.rs` e `mod.rs` (parser com JSON truncado, chunks/prompt de resumo completo).

## Tarefa 3 — UI Master-Detail, `typeset`, `polish` e `animate` (`mail.ts` e `hub.css`)
- **Pré-requisito obrigatório:** ler `reference/craft-floor.md` imediatamente antes de editar UI.
- **Arquivos:** [`windows/src/hub/pages/mail.ts`](file:///C:/Users/notefael/projetos/coucou/windows/src/hub/pages/mail.ts), [`windows/src/hub/hub.css`](file:///C:/Users/notefael/projetos/coucou/windows/src/hub/hub.css)
- **Alterações:**
  - Reescrever `renderSummaryBody` usando `parseMarkdown` / `parseInline` (normalizando `•`/`–`), agrupando `ul`/`ol`, sem links clicáveis de conteúdo externo, `aria-live` apenas em `summaryMeta` e horário de geração.
  - Reorganizar `mailOverviewPage` em barra `Conexão e provedor` (checagem inicial automática, seção `Avançado` recolhida, `redraw()` ao trocar provedor, porta salva no `change`/`blur`, atalhos para VIPs/Palavras/Regras) + Master-Detail (Triagem à esquerda com `"Todas (N)"`, grupos recolhíveis `Promoções`/`Spam` com `"Limpar grupo (N)"`, menu `"⋯"` por linha, foco restaurado em `Desfazer`, cancelamento em lote, remoção do timer de 4 s em `Apagar?`; e `Resumo rápido` + `Preparar rascunho` fixos à direita com `replyToId`, validação e `"Limpar"`).
  - Remover strings `"Coucou"` / inglês residuais da superfície Fase L, corrigir contraste em hover e `cursor: disabled`, e polir animações com suporte completo a `prefers-reduced-motion`.

## Tarefa 4 — Detector e inspeção final
- Rodar `impeccable.cmd detect --json --changed` uma vez após finalizar a UI.
- Conferir `git status` e `git diff`, preservando alterações preexistentes (`windows/design/personagens/whatsapp-propostas/prancha.html`, `.impeccable/config.json`, `windows/design/personagens/codex-nova-persona/`).
- Respeitar política de validação solicitada pelo usuário (não rodar build/testes automaticamente sem pedido/exigência aplicável, ou informar explicitamente o que não foi executado).
