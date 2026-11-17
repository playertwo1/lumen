# Plano — L6.4 + Animação P3: Cancelamento de avisos cruzados, feedback de sync e gesto na Bússola

**Spec de referência:** [2026-11-15-l64-cancelamento-avisos-p3.md](../specs/2026-11-15-l64-cancelamento-avisos-p3.md)  
**Passo da ponte:** Passo 9 (`docs/arquivadas/etapas/l6-sync-segunda-mente.md` e `docs/lumen/PONTE.md`)  
**Branch:** `feat/l64-cancelamento-avisos-p3`

---

## 1. Pré-condições e diretrizes

- Confirmar branch limpa e alinhada com as convenções de código do fork (`AGENTS.md`).
- Repositório público `playertwo1/lumen`: nunca expor caminhos locais absolutos do ambiente, credenciais, tokens ou dados pessoais em código, docs ou mensagens de commit.
- Respeitar a regra Ponytail: reaproveitar helpers existentes (`dismiss_if_active`, `clear_task_toast`, `run_sync_once_for_app`) sem criar camadas ou wrappers desnecessários.
- Preservar o `Checkpoint L6` em `docs/arquivadas/etapas/l6-sync-segunda-mente.md` como `[ ]` até a validação física conjunta com o Samsung Galaxy S25 Ultra.

---

## 2. Mapa de arquivos

### Arquivos modificados
- `windows/src-tauri/src/tasks/sync.rs` — introdução de `should_dismiss_reminder_on_remote_change` e aplicação uniforme nos três ramos de `apply_pulled_batch`.
- `windows/src-tauri/src/tasks/mod.rs` — emissão do evento `"task-sync-state"` (`"saved"` vs `"synced"`) e confirmação de `dismiss_if_active` em todas as mutações e ações de toast.
- `windows/src/lumen/compass-engine.ts` — implementação do método de animação P3 `triggerTaskSync("saved" | "synced")` e desenho dos dois pontos luminosos com respeito a `reducedMotion()`.
- `windows/src/island/island.ts` e `windows/src/main.ts` — recepção do evento `"task-sync-state"` e encaminhamento para a Bússola.
- `windows/src-tauri/tauri.conf.json`, `windows/src-tauri/Cargo.toml`, `windows/package.json` — atualização de versão para `0.7.3`.
- `docs/STATE-FORK.md` e `docs/arquivadas/etapas/l6-sync-segunda-mente.md` — atualização de status e registro da subfase.

---

## 3. Etapas de implementação

### Etapa 1 — Decisão de cancelamento no motor de sync Rust

**Arquivo:** `windows/src-tauri/src/tasks/sync.rs`

1. Implementar a função pura:
   ```rust
   pub fn should_dismiss_reminder_on_remote_change(
       old_item: Option<&TodoItem>,
       new_item: &TodoItem,
   ) -> bool
   ```
   cobrindo:
   - Conclusão (`new_item.done`) ou exclusão (`new_item.is_tombstoned()`);
   - Perda de horário de alarme (`!new_item.has_alarm_time()`);
   - Alteração de adiamento (`snoozed_until`);
   - Reagendamento (`T05`): mudança em `due_date`, `due_time`, `time_zone` ou `alert_minutes`.
2. Substituir a lógica manual nos 3 ramos de `apply_pulled_batch`:
   - Ramo 1 (`local_opt.is_none()`): avaliar `should_dismiss_reminder_on_remote_change(None, remote_item)`;
   - Ramo 2 (sequencial): avaliar `should_dismiss_reminder_on_remote_change(Some(&local), remote_item)`;
   - Ramo 3 (3-way merge): avaliar `should_dismiss_reminder_on_remote_change(Some(&local), &merge.item)`.
3. Escrever testes unitários em `sync.rs` para todos os cenários da função pura.

### Etapa 2 — Emissão de evento de estado de sincronização e chamadas locais

**Arquivo:** `windows/src-tauri/src/tasks/mod.rs`

1. Garantir que `dismiss_if_active(&app, &mgr, id)` seja invocado em `tasks_update`, `tasks_toggle_done`, `tasks_snooze`, `tasks_delete` e `handle_toast_action`.
2. Em `run_sync_once_for_app` (ou helper de sincronização), verificar o snapshot resultante:
   - Se `status.pending_outbox == 0 && status.pending_peer == Some(0) && status.state == TaskSyncState::Synced`: emitir `"task-sync-state"` com `"synced"`;
   - Caso contrário: emitir `"task-sync-state"` com `"saved"`.
3. Direcionar o evento para `crate::island::WINDOW_LABEL` e `crate::HUB_LABEL`.

### Etapa 3 — Gesto P3 na Bússola e escuta de eventos

**Arquivos:** `windows/src/lumen/compass-engine.ts`, `windows/src/island/island.ts`, `windows/src/main.ts`

1. Em `compass-engine.ts`:
   - Adicionar estado `taskSyncStart`, `taskSyncDuration` e `taskSyncMode: "saved" | "synced"`;
   - Adicionar método `triggerTaskSync(state: "saved" | "synced", duration = 1.5)`;
   - No método de desenho da Bússola, renderizar dois pontos orbitais suaves durante a animação;
   - Assegurar desativação imediata quando `reducedMotion()` for verdadeiro.
2. Em `island.ts`:
   - Expor método `taskSyncState(state: "saved" | "synced")` acionando `compass.triggerTaskSync(state)`.
3. Em `main.ts`:
   - Registrar o listener: `await onEvent<string>("task-sync-state", (state) => island.taskSyncState(state as any))`.

### Etapa 4 — Validação automatizada

1. Em `windows/src-tauri/`: executar `cargo test --lib`.
2. Em `windows/`: executar `npx tsc --noEmit` e `npm test`.

### Etapa 5 — Documentação e release local `windows-v0.7.3`

1. Atualizar versões nos manifestos (`tauri.conf.json`, `Cargo.toml`, `package.json`).
2. Atualizar `docs/STATE-FORK.md` e `docs/arquivadas/etapas/l6-sync-segunda-mente.md`, mantendo `Checkpoint L6` como `[ ]`.
3. Gerar a tag e release local de acordo com o checklist do fork.

---

## 4. Comandos de validação

```bash
# Testes do backend Rust do app
cd windows/src-tauri && cargo test --lib

# Verificação de tipos TypeScript e testes da interface
cd windows && npx tsc --noEmit && npm test
```
