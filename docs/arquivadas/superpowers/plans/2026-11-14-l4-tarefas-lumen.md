# Plano L4 — Tarefas no Lumen

**Spec:** [2026-11-14-l4-tarefas-lumen.md](../specs/2026-11-14-l4-tarefas-lumen.md)  
**Branch:** `feat/l4-tarefas` (a partir de `main` `e0ee926c`)

## Etapas de Implementação

### Passo 1 — Domínio, calendário, persistência e proposta em Rust (`windows/src-tauri/src/tasks/`)
1. Criar `windows/src-tauri/src/tasks/model.rs`:
   - Tipos `TodoPriority`, `TodoRepeatFrequency`, `TodoSubtask`, `TodoRecurrence`, `TodoSource`, `TodoItem`, `TodoProposal`, `TodoProposalDateState`, `TodoProposalTimeState`, `TodoDateShortcut`, `TodoShortcutSettings`, `TodoBucket`.
   - Calendário civil puro em Rust (algoritmo de Hinnant, sem dependências extras) com suporte a `America/Sao_Paulo` (`-03:00`), parsing de RFC3339 com offset, cálculo de `date_for_shortcut`, `alarm_timestamp_ms_for` (alerta 20 min antes, fallback para `due` quando o alerta já passou e `due` ainda é futuro, e suporte a `snoozed_until`), `next_upcoming_occurrence_date`, `bucket_for`, `search_todos` (acento-insensível) e `fold_pt`.
2. Criar `windows/src-tauri/src/tasks/store.rs`:
   - Leitura/escrita atômica de `%LOCALAPPDATA%/Coucou/Todo/todo.json` (`schema_version = 2`) e `shortcut_settings.json`, quarentena de JSON corrompido, CRUD completo (`create`, `update`, `toggle_done` com geração da próxima ocorrência recorrente, `snooze`, `delete` tombstone com restauração/`restore`, subtarefas).
3. Criar `windows/src-tauri/src/tasks/proposal.rs`:
   - Construção de prompt fiel ao `TodoProposalSkill` (nunca inferir data/hora ambígua), parsing robusto do JSON retornado pelo LLM (`chat::complete_oneshot`), e fallback determinístico seguro em pt-BR sem LLM.
4. Testes unitários Rust cobrindo modelo, fuso, recorrência, busca, quarentena de arquivo corrompido, invariantes de `TodoProposal` e parsing com mock/fallback.

### Passo 2 — Scheduler de alarmes, captura por voz e ativação Windows (`L4.3` + `L4.4`)
1. Estender `windows/src-tauri/src/permission_notifications.rs` (apenas em `#[cfg(windows)]` com helpers puros testáveis):
   - Suporte ao grupo `"task"` com botões **Adiar**, **Concluir** e **Abrir**, mantendo o mesmo `AUMID`, `CLSID` e servidor COM.
   - Roteamento de `task:<id>:snooze|complete|open` via canal dedicado / callback para `tasks::handle_toast_action`.
   - Testes unitários de XML escaping, parsing de ação `task:<id>:<acao>` e remoção de histórico.
2. Criar `windows/src-tauri/src/tasks/mod.rs`:
   - `TaskManager` gerenciado no Tauri (`app.manage`), captura de voz só microfone (`tasks_voice_start`, `tasks_voice_stop`, `tasks_voice_cancel`) usando `speech::audio` + `speech::transcribe`.
   - Worker de scheduler em segundo plano (tick ~10s + wake-up por canal ao mutar tarefas) disparando toast Windows + evento `"task-reminder"` para a Ilha, evitando duplicidade no mesmo ciclo de ocorrência.
   - Registrar módulo e comandos em `windows/src-tauri/src/lib.rs`, e adicionar ação global `captureTask` (`Ctrl+Alt+K`) em `windows/src-tauri/src/shortcuts.rs`.

### Passo 3 — Lógica pura e ponte TypeScript (`windows/src/core/`)
1. Criar `windows/src/core/tasks.ts`:
   - Tipos espelhados, classificação em `Atrasadas` / `Hoje` / `Próximas` / `Sem data` / `Concluídas`, atalhos de data/hora L4.2 (`resolveDateShortcut`, `formatMinutesLabel`, etc.), filtro acento-insensível e extrator `splitTodoProposal` para blocos ` ```lumen-todo ... ``` `.
2. Atualizar `windows/src/core/bridge.ts`, `windows/src/core/shortcuts.ts`, `windows/src/core/layout.ts` e `windows/src/i18n/extra.json` (chave `"Capture a task"` usada pelo atalho global / Wayland).

### Passo 4 — Animações P3 (`windows/src/lumen/compass-engine.ts`)
1. Implementar `triggerTaskReminder`, `triggerTaskSnooze` e `triggerTaskCheck` em `CompassEngine`, desenhando o sino oscilante, o empurrão do sino e o traçado do check, respeitando `reducedMotion()` e zerando `busy` após a duração.

### Passo 5 — UI do Hub, Ilha e Chat (`windows/src/hub/`, `windows/src/island/`, `windows/src/views/`)
1. Adicionar página `"tasks"` (`windows/src/hub/pages/tasks.ts`) no Hub (`shell.ts`, `pages.ts`, `hub.css`): lista única agrupada, busca, filtros, atalhos L4.2, subtarefas, prioridade, captura texto/voz com cartão de confirmação e aviso de privacidade.
2. Adicionar views da Ilha (`windows/src/views/tasks.ts`, `views.ts`, `island/island.ts`, `island/shortcuts.ts`, `style.css`):
   - `"taskCapture"` (campo + microfone via `Ctrl+Alt+K`).
   - `"taskConfirm"` (cartão de confirmação editável antes de gravar).
   - `"taskReminder"` (banner rápido com Adiar / Concluir / Abrir).
   - Ouvir `"task-reminder"`, `"task-snoozed"` e `"task-completed"` na Ilha para acionar P3 (`triggerTaskReminder`, `triggerTaskSnooze`, `triggerTaskCheck`).
3. Em `windows/src/views/chat.ts`: suportar renderização de ` ```lumen-todo ... ``` ` via cartão de confirmação `chatTodoProposalCard` que chama `TasksBridge.confirmProposal` apenas no clique explícito.

### Passo 6 — Testes sintéticos, revisão e documentação
1. Criar/atualizar testes Node: `windows/tests/tasks.test.mjs`, atualizar `windows/tests/shortcuts.test.mjs`, `windows/tests/hub.test.mjs`, `windows/tests/lumen-character-engine.test.mjs`.
2. Executar validação completa:
   - `windows/`: `npx tsc --noEmit`, `npm test`, `npm run build`
   - `windows/src-tauri/`: `cargo test --lib`
3. Atualizar `docs/arquivadas/etapas/l4-tarefas.md` e `docs/STATE-FORK.md` marcando o escopo sintético entregue e deixando o **Checkpoint L4** aberto até validação real no notebook.
