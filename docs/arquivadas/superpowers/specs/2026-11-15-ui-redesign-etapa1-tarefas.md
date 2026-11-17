# Etapa 1 — Fundação Editorial do Hub, Tarefas (Master-Detail) e `chatTodoProposalCard`

**Data:** 2026-11-15  
**Base visual:** [PRODUCT.md](../../../../PRODUCT.md) · [DESIGN.md](../../../../DESIGN.md) · `.impeccable/design.json`  
**Decisões fechadas:** `P1 = A` (uso pessoal sem marketing), `P2 = A` (Ilha = captura rápida; Hub = área de trabalho real), `P3 = A + B` (fluxo diário fluido em janelas largas + paridade visual com a Segunda Mente), `D1 = A` (Diário de Mesa — `#000000` / `#0e0f11` / `#18191c`, `#F3EEE6`, acento único `#E99A3D`, `Inter`), `D2 = A` (Master-Detail responsivo), `D3 = A` (listas limpas sem selos repetitivos), `D4 = A` (barra de captura + painel inspetor/revisor desktop à direita), `D5` (preservar telas originais em ótimo estado e elevar adições do fork por etapas).

## Objetivo da Etapa 1

1. **Fundação de tokens e correção do shell no Hub** (`windows/src/hub/hub.css`, `windows/src/hub/hero.ts`):
   - Alinhar a paleta do Hub ao sistema da Segunda Mente (`#000000`, `#0e0f11`, `#141518`, `#18191c`, `#1f2024`, `#F3EEE6`, `#DED9D1`, `#AAA6A0`, `#706D68`, acento único `#E99A3D`, `--hub-green: #34D399`, `--hub-red: #FF5A4E`), fonte `Inter` com fallback nativo Windows (`Segoe UI Variable Text`, `Segoe UI`), `color-scheme: dark`, `tabular-nums` e seleção/foco consistentes.
   - Eliminar resquícios de roxo (`#6e56cf`) e azul (`#3b82f6`) em busca, glow de âncora, banners, foco e seleção ("The One Voice Rule").
   - Corrigir a rolagem e o rodapé da barra lateral (`.hub-side` com `.hub-groups` / `.hub-search-results` roláveis internamente para que `.hub-status` e `.hub-version` nunca sejam cortados no rodapé) e ampliar `.hub-page` para preencher janelas largas com equilíbrio (`max-width: 1280px`).
   - Registrar o tema `tasks` (`#E99A3D`, elenco de 6 personagens do Lumen) em `THEMES` (`hero.ts`).

2. **Tela `Tarefas` no Hub** (`windows/src/hub/pages/tasks.ts`, `windows/src/hub/hub.css`):
   - **Cabeçalho:** usar o hero compacto do Lumen (`ctx.hero("tasks", ..., true)` quando disponível, com fallback seguro para ambientes sem `ctx.hero`).
   - **Barra de captura e sincronia:** campo de texto amplo + botão `Ditar` + botão primário âmbar `Propor`, com nota discreta de armazenamento local/atalho (`Ctrl+Alt+K`) e status de sincronia (`Salvo neste PC` vs `Sincronizado`, preservando `data-testid="tasks-sync-now"` e todos os estados `L6.3`).
   - **Master-Detail responsivo (`.tasks-workspace`):**
     - Sem inspetor ativo: a lista ocupa toda a largura útil do painel sem metade direita vazia.
     - Com proposta (`draftProposal`) ou edição (`editingId`) ativa: abre o **Painel Inspetor Desktop** (`.tasks-inspector`, `340px–380px` à direita em janelas largas; empilhado acima em `<= 980px`) com controles dimensionados para desktop — **sem** reutilizar `createProposalEditor` comprimido da Ilha ("The Surface Boundary Rule").
     - Preserva estritamente **"IA propõe, usuário confirma"**: datas/horas marcadas como `ambiguous` ou `unspecified` nunca são preenchidas automaticamente; banner de ambiguidade orienta a escolha explícita antes do clique em `Confirmar tarefa`.
     - Edição de tarefa existente no inspetor inclui título, atalhos rápidos + inputs nativos de data/hora, prioridade, pré-alerta, repetição e subtarefas (adicionar/marcar/remover), além de exclusão com barra de desfazer.
   - **Lista editorial limpa ("The Quiet Row Rule"):**
     - Agrupadores únicos por bucket (`Atrasadas`, `Hoje`, `Próximas`, `Sem data`, `Concluídas`) com contadores em `tabular-nums`.
     - Omitir selos redundantes por linha (`Normal`, `Sem data`, `Sincronizado`). Selos em linha aparecem somente quando exigem atenção (`Alta` / `Urgente`, `Adiada`, conflito `pending_review`, ou `.tasks-item-sync.local` `"Salvo neste PC"` quando pareado com pendência real na sincronia).
     - Conflitos de texto livre (`tasks-conflict-card`) continuam visíveis na linha da tarefa para resolução explícita (`Manter texto deste PC` / `Usar texto do celular`).

3. **`chatTodoProposalCard` no Chat** (`windows/src/views/tasks.ts`, `windows/src/style.css`):
   - Elevar o cartão embutido em respostas ` ```lumen-todo ``` ` para a linguagem visual do Lumen (fundo `#141518`, borda sutil marfim, cabeçalho limpo `"Proposta de tarefa"` sem kicker azul em caixa alta, chips e foco em âmbar cobre `#E99A3D`, feedback de salva em `#34D399`), mantendo **estritamente isoladas** as regras da Ilha (`taskCapture`, `taskConfirm`, `taskReminder`) e todos os contratos dos testes existentes.

## Restrições e Contratos Preservados

- Nenhum evento IPC Tauri (`tasks-changed`, `tasks-voice-state`, `task-focus`, `task-dismiss`, `task-sync-state`) ou chamada `TasksBridge` é alterado.
- Layout e estilos dos cards compactos da Ilha (`VIEW_LAYOUTS`, `.task-island-body`, `.task-reminder-title`, `buildTaskReminderView`) permanecem intocados.
- Sem subagents, sem alterações fora do escopo (Anotador, Mail e Celular Android ficam para as etapas seguintes).

## Critérios de Aceite

1. `npx tsc --noEmit` sem erros em `windows/`.
2. `npm test` em `windows/` com 100% dos testes passando (incluindo atualização proporcional das asserções de `L6.3` em `tasks.test.mjs` para a regra de linha limpa e novos testes cobrindo o inspetor desktop de proposta/edição e preservação da regra de ambiguidade).
3. `npm run build` em `windows/` sem erros.
4. `.\.agent\skills\impeccable\scripts\impeccable.cmd detect --json <arquivos alterados>` sem violações bloqueantes.
5. Revisão visual em navegador (largura maximizada `1440×900` e janela estreita `860×740`) conferindo estado limpo, inspetor de proposta ambígua, inspetor de edição, conflitos, sidebar sem corte no rodapé e `chatTodoProposalCard`.
