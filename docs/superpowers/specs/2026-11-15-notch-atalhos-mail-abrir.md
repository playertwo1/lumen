# Refino do Notch Aberto — Atalhos de Anotador/E-mail, Limpeza de Avisos e Seta Abrir

## Objetivo
Resolver três fricções apontadas no uso diário da ilha (Notch Aberto):
1. **Dois novos ícones no cabeçalho** ao lado de `Nova tarefa`:
   - **Anotador** (`notes`): abre o Hub diretamente na página Anotador.
   - **E-mail · Visão geral** (`mailOverview`): abre o Hub diretamente em E-mail → Visão geral.
2. **Limpar notificações de e-mails não lidos (`Mail · 357 não lidos`)** diretamente do Notch Aberto, de forma segura e não destrutiva:
   - Botão **Limpar** visível no card do Mail (cabeçalho da lista ao lado do badge numérico e cabeçalho do detalhe da mensagem).
   - Registra a marca de e-mails já vistos (`mailUnreadDismissedAt`), recalculando o aviso do notch via `effectiveMailUnread(rawUnread, dismissedAt)`: silencia o acumulado atual (voltando a avisar apenas se chegarem e-mails novos acima da marca) sem alterar nem marcar mensagens no servidor.
   - Polimento vertical do detalhe do Mail para eliminar corte no assunto e manter ações em linha única.
3. **Seta `↗` ("Abrir") funcional para o alvo em foco**:
   - Comando `open_focus_target` no backend Rust + roteamento completo no frontend para **Mail** (`integration_mail`), **Codex** (`agent_codex`), **Claude Code** (`integration_claude`), **Claude Desktop** (`agent_claude-desktop`) e **Antigravity** (`agent_antigravity`), priorizando trazer a janela já aberta para frente ou abrir o app/sessão correspondente.
   - *Tooltip* dinâmico indicando o alvo real em foco (`Abrir MailCraft`, `Abrir Codex`, `Abrir Antigravity`, etc.).
