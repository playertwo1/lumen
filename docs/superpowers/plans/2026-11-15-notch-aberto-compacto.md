# Plano — Refino do Notch Aberto e Notch Compacto (2A + Centro Contextual)

**Spec:** [2026-11-15-notch-aberto-compacto.md](../specs/2026-11-15-notch-aberto-compacto.md)

## Tarefas

1. **Geometria e lógica pura do Centro Contextual**
   - Atualizar `NOTCH_H = 36`, `botPosition("compact", ...)` (`cy = NOTCH_H / 2`) e adicionar `compactWidth(...)` em `windows/src/core/layout.ts`.
   - Criar `windows/src/core/compact.ts` com `compactCenter(...)` (prioridade: aprovação/pergunta → aviso efêmero → WhatsApp não lido → e-mail não lido → próxima tarefa próxima/atrasada → música tocando) e cálculo de largura central/posicionamento.
   - Validar `greetingPose` com `NOTCH_H = 36`.

2. **5ª aba icônica de Tarefas, captura rápida e overview polido**
   - Adicionar `taskCheck` em `windows/src/views/icons.ts`.
   - Adicionar `tabTasks` (`.tab`, `30×22`, só ícone) no `buildHeader` em `windows/src/views/views.ts` abrindo `taskCapture`, com `.on` ativo em `taskCapture` / `taskConfirm`.
   - Refinar visualmente `buildTaskCaptureView` em `windows/src/views/tasks.ts` (título `"Nova tarefa rápida"`, botão `"Criar proposta"`, mantendo fluxos e callbacks intactos).
   - Atualizar CSS em `windows/src/style.css` para `.pills`/`.pill` (`34px` com respiro), `.int-card`/`.int-actions .link-btn`, `.task-island-*`, `#island` (borda hairline sutil quando visível) e `#compact-center`.

3. **Integração no `Island` e fluxo ao vivo de Tarefas / Mensagens**
   - Armazenar cache leve de tarefas ativas e aviso efêmero no `Island` (`windows/src/island/island.ts`), alimentado no boot e no evento `"tasks-changed"` em `windows/src/main.ts` (via `refreshTasks()`).
   - Atualizar `targetSize()`, `applyGeometry()`, `syncDom()`, `updateClock()` e handlers de clique no `#compact-center` em `windows/src/island/island.ts`.
   - Ocultar `#island-clock` enquanto `#compact-center` estiver ativo e encolher para `compactWidth(...)` no modo 2A ocioso.

4. **Testes e validação completa**
   - Criar `windows/tests/notch-compact-open.test.mjs` cobrindo:
     - `compactWidth` (com e sem centro, mini-grid, gauge de plano);
     - `compactCenter` em todas as prioridades e retorno `null` quando ocioso;
     - 5ª aba icônica em `buildHeader` abrindo `taskCapture` e ativando `.on` em `taskCapture`/`taskConfirm`;
     - `Island` alternando largura 2A ↔ `COMPACT_W` quando chega mensagem/aprovação/tarefa e abrindo a ação ao clicar no `#compact-center`.
   - Rodar em `windows/`: `npx tsc --noEmit`, `npm test`, `npm run build` (e conferir especialmente `greeting.test.mjs`, `closed.test.mjs`, `tasks.test.mjs`, `i18n.test.mjs`).
