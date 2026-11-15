# Spec — L6.4 + Animação P3: Cancelamento de avisos cruzados, feedback de sync e gesto na Bússola

**Data:** 2026-11-15  
**Passo da ponte:** Passo 9 (`L6.4` — `docs/roadmap/fase-l-lumen.md` e `docs/lumen/PONTE.md`)  
**Critérios de aceite:** `AC-08` (aviso cancelado no outro dispositivo) e `T05` (reagendamento dispensa aviso anterior)  
**Contrato de referência:** `docs/lumen/CONTRATO-SYNC.md` §7.3  
**Branch prevista:** `feat/l64-cancelamento-avisos-p3`

---

## 1. Objetivo e escopo

Completar a fatia de tarefas da ponte no Lumen (Windows Desktop), garantindo:

1. **Dispensa exata de lembretes em mutações remotas (`should_dismiss_reminder_on_remote_change`):** quando o motor de sync processa itens puxados da caixa de correio (`apply_pulled_batch` em `windows/src-tauri/src/tasks/sync.rs`), qualquer alteração remota que conclua a tarefa (`done: true`), exclua (`deleted_at.is_some()`), remova o agendamento (`!new_item.has_alarm_time()`), adie (`snoozed_until` modificado) ou reagende a data/hora/fuso/alerta (`T05`: `due_date`, `due_time`, `time_zone`, `alert_minutes` alterados) deve disparar a dispensa do lembrete ativo no PC.
2. **Dispensa imediata em ações locais:** garantir que `dismiss_if_active` seja acionado em todas as mutações locais do usuário (`tasks_update`, `tasks_toggle_done`, `tasks_snooze`, `tasks_delete` e `handle_toast_action`), limpando o mapa `mgr.fired`, fechando o Toast do Windows (`clear_task_toast`) e emitindo o evento `"task-dismiss"` tanto para a Ilha (`crate::island::WINDOW_LABEL`) quanto para o Hub (`crate::HUB_LABEL`).
3. **Sinalização do estado de sincronização via evento Tauri (`"task-sync-state"`):** informar à interface o estado consolidado da sincronização (`"saved"` quando houver pendências locais na outbox ou no relay, vs. `"synced"` quando todas as alterações estiverem confirmadas e o peer estiver sincronizado), permitindo que a Ilha reaja em tempo real.
4. **Gesto visual P3 na Bússola (`triggerTaskSync`):** introduzir no motor de animação da Bússola (`windows/src/lumen/compass-engine.ts`) um gesto sutil com dois pontos de luz em órbita/pulso representando o computador e a Segunda Mente se conectando ou atingindo sincronia, com suporte estrito a `reducedMotion()`.
5. **Release local `windows-v0.7.3`:** após a conclusão dos testes sintéticos e empacotamento, gerar a release local seguindo o checklist de release do fork, mantendo o `Checkpoint L6` como pendente (`[ ]`) até a validação física conjunta com o Samsung Galaxy S25 Ultra.

### Fora de escopo

- Sincronização de entidades da versão 1.1 (notas, conversas, personas, preferências).
- Conexão LAN direta TLS / WebSocket ao vivo (previstas para fases posteriores da ponte).
- Validação física em hardware real (Galaxy S25 Ultra conectado), que segue explicitamente marcada como `NOT_RUN` até a sessão com o dispositivo físico.

---

## 2. Decisões técnicas no backend Rust (`windows/src-tauri/`)

### 2.1 Helper puro `should_dismiss_reminder_on_remote_change` (`tasks/sync.rs`)

Para unificar a lógica de dispensa nos três ramos de `apply_pulled_batch`:
- **Ramo 1:** item remoto novo criado ou sem cópia local prévia (`local_opt.is_none()`).
- **Ramo 2:** aplicação sequencial sem conflito concorrente (`envelope.base_version == local.version`).
- **Ramo 3:** reconciliação de 3 vias (*3-way merge*).

A função pura de decisão avalia:

```rust
pub fn should_dismiss_reminder_on_remote_change(
    old_item: Option<&TodoItem>,
    new_item: &TodoItem,
) -> bool {
    if new_item.done || new_item.is_tombstoned() {
        return true;
    }
    if !new_item.has_alarm_time() {
        return old_item.is_some_and(|o| o.has_alarm_time());
    }
    match old_item {
        None => new_item.snoozed_until.is_some(),
        Some(old) => {
            let became_done = new_item.done && !old.done;
            let became_deleted = new_item.is_tombstoned() && !old.is_tombstoned();
            let snooze_changed = new_item.snoozed_until != old.snoozed_until;
            let schedule_changed = old.has_alarm_time()
                && (new_item.due_date != old.due_date
                    || new_item.due_time != old.due_time
                    || new_item.time_zone != old.time_zone
                    || new_item.alert_minutes != old.alert_minutes);
            became_done || became_deleted || snooze_changed || schedule_changed
        }
    }
}
```

Qualquer item cuja avaliação retorne `true` é adicionado a `dismiss_ids`, garantindo que o ciclo em `run_sync_once_for_app` execute `dismiss_if_active(app, mgr, id)` para cada um.

### 2.2 Evento `"task-sync-state"` (`tasks/mod.rs`)

Sempre que um ciclo de sincronização for concluído (ou ao atualizar o snapshot de tarefas), verificar o estado consolidado:
- Se `status.pending_outbox == 0 && status.pending_peer == Some(0) && status.state == TaskSyncState::Synced`:
  - Emitir evento `"task-sync-state"` com payload `"synced"` para `crate::island::WINDOW_LABEL` e `crate::HUB_LABEL`.
- Caso contrário:
  - Emitir `"task-sync-state"` com payload `"saved"`.

---

## 3. Decisões técnicas na interface TypeScript (`windows/src/`)

### 3.1 Motor da Bússola (`windows/src/lumen/compass-engine.ts`)

Adicionar método de animação P3:

```typescript
/** P3 (L6): dois pontos de luz em órbita/pulso sinalizando conexão e sincronia com a Segunda Mente. */
triggerTaskSync(state: "saved" | "synced", duration = 1.5): void {
  if (reducedMotion()) {
    this.taskSyncStart = -1;
    this.taskSyncDuration = 0;
    return;
  }
  const dur = Math.max(0.4, duration);
  this.taskSyncStart = this.elapsed;
  this.taskSyncDuration = dur;
  this.taskSyncMode = state;
  this.squash();
  this.blink();
  this.markBusyFor(dur);
}
```

No loop de renderização do Canvas da Bússola, desenhar dois pontos orbitais suaves em torno do centro quando `taskSyncStart >= 0`:
- Modo `"saved"`: cor âmbar suave / lavanda com pulso tranquilo;
- Modo `"synced"`: cor esmeralda / azul ciano com convergência ao centro.

### 3.2 Ilha e Main (`windows/src/island/island.ts` e `windows/src/main.ts`)

- Em `island.ts`: expor o método `taskSyncState(state: "saved" | "synced")` delegando para `compass.triggerTaskSync(state)`.
- Em `main.ts`: registrar o listener `await onEvent<string>("task-sync-state", (state) => island.taskSyncState(state as any))`.

---

## 4. Atualização de documentação e release

1. `docs/STATE-FORK.md`: registrar a entrega de L6.4 + P3, testes e a release local `windows-v0.7.3`.
2. `docs/roadmap/fase-l-lumen.md`: detalhar L6.4 e manter o `Checkpoint L6` como `[ ]` até a validação física no S25 + notebook.
3. Preparar a release local `windows-v0.7.3` atualizando versão em `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` e `package.json`.

---

## 5. Critérios de verificação automatizada

1. `cargo test --lib` em `windows/src-tauri/` sem falhas (incluindo testes de `should_dismiss_reminder_on_remote_change` e ciclo de sync).
2. `npx tsc --noEmit` em `windows/` com zero erros de compilação TypeScript.
3. `npm test` em `windows/` cobrindo a suíte de testes de interface e snapshot de tarefas.
4. Validação real em hardware (Galaxy S25 Ultra + notebook): **`NOT_RUN`** até a etapa de teste conjunto.
