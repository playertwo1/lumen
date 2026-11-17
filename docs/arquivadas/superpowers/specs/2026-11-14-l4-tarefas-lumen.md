# L4 — Tarefas no Lumen

**Data:** 2026-11-14  
**Fase:** [Fase L — Lumen + Segunda Mente](../../etapas/l4-tarefas.md) (`L4.1`, `L4.2`, `L4.3`, `L4.4` e animação P3)  
**Dependência:** F11-01 da Segunda Mente (`playertwo1/segundamente-app`), já concluída (`TodoItem` v2, `TodoProposal`, `TodoShortcutSettings`, `TodoScheduleCalculator`, `TodoProposalSkill`).

## Objetivo

Trazer para o Lumen desktop uma lista única e confiável de tarefas compatível com o modelo F11 da Segunda Mente (pronta para o sync da L6), com captura rápida por atalho/ditado/chat protegida por **cartão de confirmação explícita**, lembretes nativos do Windows com **Adiar / Concluir / Abrir**, e as animações P3 do personagem (sino, empurrar sino ao adiar e traçar check ao concluir).

## Arquitetura e Interfaces

### 1. Domínio, persistência e agenda em Rust (`windows/src-tauri/src/tasks/`)

- **`tasks/model.rs`** — Paridade direta com `todo_item.dart`, `todo_proposal.dart`, `todo_shortcut_settings.dart` e `todo_schedule_calculator.dart`:
  - Arquivo persistido em `%LOCALAPPDATA%\Coucou\Todo\todo.json` (e preferências em `%LOCALAPPDATA%\Coucou\Todo\shortcut_settings.json`), com escrita atômica (`temp` + rename), backup preventivo em corrupção (`todo.corrupted.<ts>.json`) e migração aditiva de listas legadas (`schema_version = 2`).
  - `TodoItem`: `id`, `text`, `due`, `due_date` (`YYYY-MM-DD`), `due_time` (`HH:mm`), `time_zone` (padrão `"America/Sao_Paulo"`), `alert_minutes` (padrão `20`), `priority` (`low | normal | high | urgent`), `subtasks: Vec<TodoSubtask>`, `recurrence: Option<TodoRecurrence>` (`daily | weekly | monthly`), `series_id`, `done`, `created_at` (RFC3339), `done_at`, `snoozed_until` (RFC3339), `source: Option<TodoSource>`, `source_stale`, `version` (`u32`, inicia em `1`), `deleted_at` (tombstone RFC3339).
  - `TodoDateShortcut`: `today`, `tomorrow`, `inThreeDays`, `nextMonday` (atalhos de hora padrão `[540, 840, 1200]` = `09:00`, `14:00`, `20:00`, alerta padrão `20` min, soneca padrão `10` min).
  - `TodoProposal`: `id`, `title`, `date_state` (`unspecified | explicit | ambiguous`), `due_date`, `date_phrase`, `time_state` (`unspecified | explicit | ambiguous`), `due_time`, `time_phrase`, `priority`. Invariantes idênticas às do celular: `due_date` só existe com `date_state == explicit`; `due_time` só existe com `time_state == explicit` e data explícita.
  - Classificação por lista única (`TodoBucket`): `overdue` (Atrasadas), `today` (Hoje), `upcoming` (Próximas), `noDate` (Sem data) e `done` (Concluídas), além de `search_todos` acento-insensível (título, `due`, `source.excerpt` e texto das subtarefas, incluindo concluídas).
  - Ao concluir uma tarefa recorrente aberta (`done: false -> true`), cria a próxima ocorrência aberta com novo `id`, `version: 1`, `series_id` preservado e próxima data futura via `next_upcoming_occurrence_date`, mantendo o histórico concluído (paridade com `TodoService.toggleDone`).
- **`tasks/proposal.rs`** — Extração estruturada (`propose_from_text`):
  - Usa `crate::chat::complete_oneshot` com o provedor ativo do Lumen (incluindo Plano ChatGPT OAuth) quando configurado, com prompt estrito do `TodoProposalSkill`: nunca inferir data/hora ambígua (`"sexta"`, `"semana que vem"`, `"de manhã"` → `ambiguous` preservando `date_phrase`/`time_phrase` e omitindo `due_date`/`due_time`).
  - Quando o provedor ativo falhar ou não estiver configurado, cai em analisador determinístico conservador em pt-BR que só preenche `explicit` para palavras exatas (`hoje`, `amanhã`, `HH:mm` / `HHh`) e marca expressões vagas como `ambiguous`, mantendo o título fiel sem gravar nada automaticamente.
- **`tasks/mod.rs`** — `TaskStore`, captura por voz (microfone apenas via `speech::audio` + `speech::transcribe`, sem gravar áudio do sistema nem sujar `Notes/`), scheduler de lembretes em segundo plano e comandos Tauri:
  - `tasks_state() -> TasksSnapshot` (tarefas ativas + pendentes + configurações de atalho + status de notificações Windows).
  - `tasks_propose_from_text(text: String) -> Result<TodoProposal, String>`
  - `tasks_voice_start() -> Result<(), String>` / `tasks_voice_stop() -> Result<Option<TodoProposal>, String>` / `tasks_voice_cancel()`
  - `tasks_confirm_proposal(input: ConfirmTaskInput) -> Result<TodoItem, String>` (única porta de gravação a partir de proposta)
  - `tasks_create(input: CreateTaskInput)` / `tasks_update(input: UpdateTaskInput)` / `tasks_toggle_done(id: String)` / `tasks_snooze(id: String, minutes: Option<i64>)` / `tasks_delete(id: String)` / `tasks_restore(id: String)`
  - `tasks_toggle_subtask(task_id: String, subtask_id: String)` / `tasks_add_subtask(task_id: String, text: String)` / `tasks_remove_subtask(task_id: String, subtask_id: String)`
  - `tasks_shortcut_settings_save(settings: TodoShortcutSettings)`
  - `tasks_trigger_reminder_test(id: String)` (auxiliar opcional somente para validar UI/toast sob demanda do usuário se desejar; lembretes reais usam o scheduler automático).

### 2. L4.4 — Aviso do Windows com Adiar, Concluir e Abrir (`permission_notifications.rs`)

- Manter **intactos** o `AUMID` (`fr.louisraille.coucou`), a chave `Software\Classes\AppUserModelId\fr.louisraille.coucou`, o `CLSID` (`{5A0C5EFA-C98F-4FA4-a337-012F9685BED3}`) e o atalho `Lumen.lnk` (L1.2), evitando qualquer colisão de servidor COM no processo.
- Estender `permission_notifications.rs` com `Command::ShowTaskReminder { task_id, title, subtitle }` no grupo `"task"`, usando `tag = "t_<hash>"` (até 16 chars / compatível com `History().RemoveGroupedTagWithId`) e argumentos `task:<task_id>:snooze`, `task:<task_id>:complete`, `task:<task_id>:open`.
- Botões do toast WinRT em pt-BR: **Adiar** (`snooze`, +10 min), **Concluir** (`complete`) e **Abrir** (`open`, abre o Hub na aba `"tasks"` focando a tarefa).
- Ao disparar o alarme no horário calculado (`due - alert_minutes`, ou `due` se o alerta já passou, ou `snoozed_until`), além do toast Windows, emite `"task-reminder"` para a Ilha (mostrando cartão rápido de lembrete caso o Windows esteja com toast indisponível ou Focus Assist ativo e disparando a animação P3 do sino). Ao adiar ou concluir (seja pelo Toast Windows ou pela UI), limpa o toast correspondente da Central de Ações, atualiza `todo.json`, emite `"tasks-changed"` e dispara `"task-snoozed"` ou `"task-completed"` na Ilha.

### 3. Interface TypeScript (`windows/src/`)

- **`core/tasks.ts` + `core/bridge.ts`:** tipos espelhados do Rust, helpers puros de classificação (`Hoje`, `Próximas`, `Atrasadas`, `Sem data`, `Concluídas`), busca acento-insensível, cálculo de atalhos de dia (`Hoje`, `Amanhã`, `Daqui a 3 dias`, `Semana que vem`) e hora (`09:00`, `14:00`, `20:00`), e parser de blocos ` ```lumen-todo ... ``` ` no chat (para renderizar cartão de confirmação inline no chat quando a IA responder com proposta de tarefa).
- **`hub/pages/tasks.ts` + `hub/shell.ts` + `hub/pages.ts`:** nova aba **Tarefas** na seção principal do Hub (logo após Anotador):
  - Captura rápida no topo (campo texto + botão microfone/voz → abre cartão de confirmação editável antes de salvar).
  - Barra de busca + filtros (`Todas`, `Hoje`, `Próximas`, `Atrasadas`, `Concluídas`).
  - Lista única agrupada com prioridade visual, subtarefas inline (marcar/adicionar/remover), seletor rápido de dia/hora (L4.2) + alerta (`20 min` padrão, configurável) + repetição opcional, adiar/concluir/excluir com desfazer.
  - Aviso discreto caso a notificação nativa do Windows esteja indisponível/desativada (como na F11-04/F11-06).
  - Lembrete discreto de privacidade no rodapé da captura: *"Não registre dados de clientes, contas ou documentos internos do trabalho."*
- **Atalho global (`shortcuts.rs` + `core/shortcuts.ts` + `island/shortcuts.ts`):** nova ação `captureTask` com padrão `Ctrl+Alt+K` (livre de conflito AltGr nos 6 layouts testados em `shortcuts.test.mjs`). Ao pressionar, acorda a Ilha na view `"taskCapture"`.
- **Ilha (`views/views.ts` + `views/tasks.ts` + `core/layout.ts`):**
  - `"taskCapture"`: campo rápido + microfone para capturar por voz/texto; ao enviar ou finalizar voz, vai para `"taskConfirm"`.
  - `"taskConfirm"`: cartão de confirmação da proposta (`TodoProposal`) com atalhos de dia (`Hoje`, `Amanhã`, `+3 dias`, `Sem. que vem`), hora (`9h`, `14h`, `20h`), alerta (`20 min`), prioridade e botões **Confirmar** / **Descartar**.
  - `"taskReminder"`: exibição curta de lembrete disparado com **Adiar (10m)**, **Concluir** e **Abrir**, sincronizada com a P3.
- **Chat (`views/chat.ts`):** instrução discreta opcional ou reconhecimento de bloco ` ```lumen-todo ... ``` ` para renderizar cartão de confirmação `todoProposalCard` direto na resposta do chat sem gravar sozinho.

### 4. Animação P3 (`windows/src/lumen/compass-engine.ts`)

- `triggerTaskReminder(duration = 2.0)`: desenha um sino animado oscilando suavemente ao lado do personagem durante o lembrete.
- `triggerTaskSnooze(duration = 1.35)`: o personagem se inclina para a direita e empurra o sino para longe com traços de impulso.
- `triggerTaskCheck(duration = 1.4)`: um selo circular com check (✓) sendo traçado progressivamente em verde esmeralda (`#34D399`).
- Todos respeitam `reducedMotion()` (`prefers-reduced-motion`) e garantem retorno a `busy === false` ao término.

## Migração e Rollback

- Os dados novos ficam isolados em `%LOCALAPPDATA%\Coucou\Todo\todo.json` e `%LOCALAPPDATA%\Coucou\Todo\shortcut_settings.json`. Nenhuma tabela de `Notes/` ou `settings.json` legado é removida.
- Caso o usuário faça downgrade, `Notes/` e `settings.json` permanecem intactos (campos novos de `shortcuts` são ignorados com segurança pelo `#[serde(default)]`); `todo.json` corrompido é movido para quarentena com timestamp antes de recriar arquivo limpo, sem perda silenciosa.

## Aceite

- **Sintético:**
  - `npx tsc --noEmit`, `npm test` (incluindo testes novos para `tasks`, `shortcuts`, `compass-engine` P3 e bloco `lumen-todo`), `npm run build`.
  - `cargo test --lib` em `windows/src-tauri` (incluindo testes de `tasks::model`, `tasks::proposal`, `tasks::scheduler`, `tasks::store` com recorrência/tombstone/migração/quarentena, e `permission_notifications` com parsing e geração XML de toast de tarefa com Adiar/Concluir/Abrir).
- **Real (Checkpoint L4, pendente após publicação/instalação no notebook):**
  - Capturar uma tarefa por voz (`Ctrl+Alt+K` ou botão de microfone), confirmar no cartão com horário próximo/atalho e verificar que o Toast nativo do Windows dispara no horário certo (20 min antes ou no horário vencendo) com **Adiar**, **Concluir** e **Abrir** funcionais e animações P3 na ilha.
