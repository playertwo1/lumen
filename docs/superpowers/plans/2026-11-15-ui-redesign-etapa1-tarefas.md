# Plano — Etapa 1: Fundação do Hub, Tarefas (Master-Detail) e `chatTodoProposalCard`

**Spec:** [2026-11-15-ui-redesign-etapa1-tarefas.md](../specs/2026-11-15-ui-redesign-etapa1-tarefas.md)  
**Branch:** `feat/ui-redesign-etapa1-tarefas`

## Passos

1. **Fundação de tokens e shell (`windows/src/hub/hub.css`, `windows/src/hub/hero.ts`):**
   - Definir `--font` com `Inter` + fallbacks Windows e `--mono` no `:root` de `hub.css`, atualizar variáveis `--hub-*` para a paleta de `DESIGN.md` (`#000000`, `#0e0f11`, `#141518`, `#18191c`, `#1f2024`, `#F3EEE6`, `#DED9D1`, `#AAA6A0`, `#706D68`, `#E99A3D`, `#34D399`, `#FF5A4E`), `color-scheme: dark` e `::selection`.
   - Substituir focos/glows roxos e azuis por âmbar cobre (`--hub-accent`), padronizar `:focus-visible`, raios (`10px` em controles, `14px` em cards/inspetor, `22px` no painel), `tabular-nums` em contagens/horários.
   - Corrigir `.hub-side` para que `.hub-groups` e `.hub-search-results` tenham rolagem própria (`min-height: 0; overflow-y: auto`), mantendo `.hub-status` e `.hub-version` ancorados sem corte no rodapé; ajustar `.hub-page` para `max-width: 1280px; width: 100%; margin: 0 auto;`.
   - Adicionar `tasks` em `THEMES` (`hero.ts`) com cor `#E99A3D` e 6 personagens.

2. **Tela `Tarefas` Master-Detail (`windows/src/hub/pages/tasks.ts` + `windows/src/hub/hub.css`):**
   - Substituir o uso de `createProposalEditor` da Ilha por um construtor dedicado de inspetor desktop (`renderProposalInspector` / `renderEditInspector`), mantendo 100% da lógica de segurança (`date_state === "explicit"`, `time_state === "explicit"`, banner de ambiguidade, `TasksBridge.confirmProposal`, `TasksBridge.update`, subtarefas e recorrência).
   - Estruturar `.tasks-workspace` com alternância fluida entre coluna única equilibrada (sem inspetor) e Master-Detail (`minmax(0, 1fr) minmax(340px, 380px)`) quando `draftProposal` ou `editingId` estiver ativo; empilhar com o inspetor visível primeiro em `<= 980px`.
   - Limpar linhas da lista (`.tasks-item`): remover selos padrão redundantes (`Normal`, `Sem data`, `Sincronizado`), manter `.tasks-item-sync.local` quando pareado com item pendente e destacar linha selecionada/focada com `--hub-accent-soft`.
   - Preservar todos os fluxos de voz, busca sem acento, filtros, `+10m`, `Testar aviso`, desfazer exclusão e resolução inline de conflitos (`tasks-conflict-card`).

3. **Elevação do `chatTodoProposalCard` (`windows/src/views/tasks.ts` + `windows/src/style.css`):**
   - Atualizar o cabeçalho de `chatTodoProposalCard` removendo o kicker azul em caixa alta em favor de título editorial limpo (`.chat-todo-title`).
   - Escopar o novo estilo do cartão do chat em `.chat-todo-card ...` dentro de `windows/src/style.css` com a paleta marfim/âmbar, sem afetar as classes `.task-island-*` da Ilha.

4. **Testes e validação (`windows/tests/tasks.test.mjs`):**
   - Atualizar asserções de `L6.3` para refletir a supressão de selos redundantes em itens já sincronizados e em `local_only`, mantendo verificação explícita de `"Salvo neste PC"` em pendências reais quando pareado.
   - Cobrir abertura do inspetor de proposta no Hub (preservando ambiguidade sem gravar sozinho), abertura do inspetor de edição para tarefa existente e atualização de subtarefas/campos.
   - Executar `npx tsc --noEmit`, `npm test`, `npm run build` em `windows/`, rodar `impeccable.cmd detect --json` e fazer inspeção visual em navegador (`1440×900` e `860×740`).
