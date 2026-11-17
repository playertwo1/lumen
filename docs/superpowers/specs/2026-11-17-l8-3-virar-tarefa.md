# Spec — L8.3: Anotador, botão "Virar tarefa" nas ações

**Data:** 2026-11-17  
**Fase:** [L8 — Ajustes do uso real](../../roadmap/fase-l-lumen.md) (`L8.3`)  
**Branch/Worktree:** `feat/l8-notes` (`coucou-l8-notes`)

## 1. Problema

Os itens de **Ações / To-do** de uma nota do Anotador ficam só ali. Para acompanhar um deles em **Tarefas** é preciso recriar a tarefa à mão. O dono quer um clique que transforme o item em uma tarefa simples (só o título). Detalhes (data, hora, prioridade) ele edita depois em Tarefas.

## 2. Decisão

- Cada item de To-do da nota ganha um botão pequeno **"Virar tarefa"** (`button(..., small)` do Hub, ao lado do texto). Um clique chama o comando novo `notes_virar_tarefa(id, todo)`.
- O comando cria a tarefa pelo mesmo caminho de `tasks_create` (`LocalState::create_and_enqueue`, então entra na outbox do sync L6), com `CreateTaskInput` só com `text`: sem data, hora, prioridade, alarme, recorrência, subtarefas nem `source`.
- O vínculo fica no próprio item da nota: `Todo.task_id: Option<String>` (serializado como `taskId`, opcional, `skip_serializing_if`). O sync de tarefas não é tocado; `Note` nunca vai para o celular.
- Não duplicar: se o item já tem `task_id`, o comando devolve a nota sem criar nada. O botão fica desabilitado com o texto **"Virou tarefa"**, inclusive depois de reabrir a nota (o vínculo está no `note.json`).
- Regerar o resumo substitui `todos` por novos ids. Para o item continuar marcado, o `task_id` é transferido do item antigo para o novo com o mesmo texto (comparação com `fold_pt`, sem acento e sem caixa).
- Nada vira tarefa sem clique. Sem data inferida.

## 3. Arquivos

- `windows/src-tauri/src/notes/builder.rs`: campo `task_id` em `Todo`.
- `windows/src-tauri/src/notes/mod.rs`: função pura `carry_task_links` (usada em `summarize`), função pura `virar_tarefa` (regra de não duplicar, recebe a criação como closure), comando `notes_virar_tarefa`.
- `windows/src-tauri/src/tasks/mod.rs`: `plain_task_input(text)` e `create_plain(app, text)` (usado pelo comando; mesmo caminho de `tasks_create`).
- `windows/src-tauri/src/lib.rs`: registrar `notes::notes_virar_tarefa`.
- `windows/src/core/flow.ts`: `Todo.taskId`, `FlowBridge.convertTodoToTask`.
- `windows/src/hub/pages/notes.ts`: botão na lista de ações.
- `windows/src/hub/hub.css`: só uma regra para empurrar o botão para a direita da linha.
- `docs/roadmap/fase-l-lumen.md`: caixas da L8.3.

## 4. Interfaces

```rust
// notes/builder.rs
pub struct Todo { ..., #[serde(default, rename = "taskId", skip_serializing_if = "Option::is_none")] pub task_id: Option<String> }

// notes/mod.rs
#[tauri::command]
pub fn notes_virar_tarefa(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, todo: String) -> Result<Note, String>

// tasks/mod.rs
pub fn create_plain(app: &AppHandle, text: String) -> Result<TodoItem, String>
```

```ts
// core/flow.ts
convertTodoToTask: (id: string, todo: string) => callOrThrow<Note>("notes_virar_tarefa", { id, todo })
```

## 5. Testes

- Rust, `notes/mod.rs`: `virar_tarefa` cria uma tarefa por clique (closure chamada uma vez); um segundo clique no mesmo item não cria outra; o item de outra nota não é afetado; `carry_task_links` mantém o `task_id` quando o texto é igual (sem acento e sem caixa) e não marca item novo.
- Rust, `tasks/mod.rs`: `plain_task_input` gera tarefa sem data, hora, prioridade nem alarme, e a criação via `LocalState::create_and_enqueue` entra na outbox (uma entrada, `base_version` 0, `new_version` 1).
- Rust, `notes/builder.rs`: JSON antigo sem `taskId` continua lendo (`serde(default)`).
- TS, `tests/notes-export.test.mjs` (mesmo padrão do arquivo): item sem `taskId` mostra "Virar tarefa" habilitado; clique chama `notes_virar_tarefa`; item com `taskId` mostra "Virou tarefa" desabilitado.

## 6. Aceite

- Um clique em "Virar tarefa" cria uma tarefa só com o título, visível em Tarefas sem data.
- Depois do clique, o botão mostra "Virou tarefa" desabilitado, também após fechar e reabrir a nota e após regerar o resumo com o mesmo texto.
- Nenhum clique extra duplica a tarefa.
- A tarefa criada segue para a outbox do sync como as demais.
- `npx tsc --noEmit`, `npm test`, `npm run build` e `cargo test --lib` passam.
