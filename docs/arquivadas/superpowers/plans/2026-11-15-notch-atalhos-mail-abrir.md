# Plano — Refino do Notch Aberto (Atalhos, Limpar Avisos e Seta Abrir)

1. **Ícones e Atalhos (`windows/src/views/icons.ts`, `views.ts`, `core/bridge.ts`, `island/island.ts`, `src-tauri/src/lib.rs`)**
   - Adicionar `notepad`, `envelope` e `bellSlash` em `ICONS` (traço consistente `24×24`, `stroke: 1.7`).
   - Adicionar `tabNotes` e `tabMail` em `buildHeader`, estendendo `openSettingsWindow(page?: string)` e garantindo que `show_hub` aplique `page` mesmo se a janela do Hub for criada sob demanda.
2. **Limpeza de avisos do Mail + Polimento do detalhe (`windows/src/core/compact.ts`, `core/state.ts`, `views/mail.ts`, `island/island.ts`, `style.css`)**
   - Exportar `effectiveMailUnread(rawUnread, dismissedAt)` em `compact.ts` e armazenar `mailUnreadDismissedAt` em `State` com persistência em `localStorage`.
   - Adicionar botão `Limpar` na lista e no detalhe do card do Mail (`views/mail.ts`), atualizando o badge e o Centro Contextual imediatamente ao clicar.
   - Ajustar `.mail-detail`, `.mail-detail-subject`, `.mail-actions` e `.mail-clear-btn` em `style.css`.
3. **Seta `↗` Abrir (`windows/src-tauri/src/platform/windows.rs`, `platform/linux.rs`, `mail/mod.rs`, `lib.rs`, `core/bridge.ts`, `island/island.ts`, `views/views.ts`)**
   - Implementar `open_focus_target` em Rust cobrindo `integration_mail`, `agent_codex`, `integration_claude`, `agent_claude-desktop` e `agent_antigravity`, além de helper puro `focusTargetTitle(task, mailBackend)` para o tooltip.
4. **Validação**
   - Estender `windows/tests/notch-compact-open.test.mjs`, rodar `npx tsc --noEmit`, `npm test`, `npm run build` / `npx vite build` e `impeccable.cmd detect`.
