# Spec — Refino do Notch Aberto e Notch Compacto (2A + Centro Contextual)

**Data:** 2026-11-15  
**Escopo:** Melhorias visuais e funcionais na Ilha (Notch) no app Windows (`windows/`), nos modos aberto (`expanded`) e compacto (`compact`), mantendo compatibilidade total com `greeting`, L4 (`taskCapture` / `taskConfirm` / `taskReminder`) e fluxo seguro *"IA propõe, usuário confirma"*.

---

## 1. Objetivo

1. **Notch Aberto (`640px`):**
   - Adicionar acesso direto para **criar tarefa** na barra superior (`#header`) usando **apenas ícone** no mesmo padrão geométrico (`30×22`, `.tab` / `.tab.on`) das 4 abas já existentes (`overview`, `prompt`, `upload`, `launch`).
   - Polir o cartão direito (`.pills` / `.pill`) para dar respiro vertical e horizontal (`34px` de altura) aos mini-bots, evitando que encostem ou ultrapassem a borda da pílula.
   - Polir o cartão esquerdo (`.int-card`, `.int-actions`, `.link-btn`) e a tela de captura rápida (`taskCapture`) mantendo exclusivamente ações reais de cada integração.

2. **Notch Compacto / Fechado (**Opção 2A + Centro Contextual dinâmico**):**
   - **Modo 2A ocioso:** aumentar a altura compacta de `32px` para **`36px`** (`NOTCH_H = 36`, borda hairline sutil inferior/lateral) para dar respiro ao grid `2×2` e ao Lumen, e encolher a largura do notch quando não houver conteúdo central ativo, eliminando o vão preto vazio no centro.
   - **Centro Contextual dinâmico:** exibir no centro do modo compacto um resumo contextual vivo somente quando houver item relevante (ou `showClock` ligado nas preferências), animando a largura de volta para `COMPACT_W` (`288px`) enquanto ativo e recolhendo para a largura compacta 2A quando limpar.

---

## 2. Desenho detalhado

### 2.1 Notch Aberto (`#header`, `overview`, `taskCapture`)

- **5ª aba em `.tabs` (`windows/src/views/views.ts`, `windows/src/views/icons.ts`):**
  - Novo ícone `taskCheck` em `ICONS` (caixa arredondada + check, grade `24×24`, `stroke: 1.7`), botão `tabTasks` com `.tab`, `title="Nova tarefa"` e `aria-label="Nova tarefa"`.
  - Clique executa `actions.blip()` e `actions.setView("taskCapture")`.
  - Classe `.tab.on` ativa quando `State.view === "taskCapture" || State.view === "taskConfirm"`.
- **Captura rápida (`taskCapture`, `windows/src/views/tasks.ts`, `windows/src/style.css`):**
  - Título `"Nova tarefa rápida"` com badge `"Ctrl+Alt+K"`, barra de digitação + microfone + botão primário `"Criar proposta"`, linha inferior com `"Abrir Tarefas"`, `"Fechar"` e aviso `"Datas ambíguas sempre pedem confirmação"`.
  - Mantém integralmente a transição para `taskConfirm` sem gravar nada automaticamente antes do clique em confirmar.
  - Cabe confortavelmente dentro da geometria `taskCapture` sem corte em `640px`.
- **Overview (`.pills` e `.int-card`):**
  - `.pills`: `grid-auto-rows: 34px; gap: 6px; padding: 0 10px;`
  - `.pill`: `height: 34px;`, `.pill .mini { margin-left: 8px; }`, `.pill .lbl { padding: 0 10px 0 38px; }`
  - `.int-card`: alinhamento vertical centralizado e `.int-actions .link-btn` com acabamento de pílula discreta mantendo apenas os links reais já existentes.

### 2.2 Notch Compacto (2A + Centro Contextual)

- **Geometria (`windows/src/core/layout.ts`):**
  - `NOTCH_H = 36` (respiro para `#mini-grid` `29×29` e `#bot-canvas`), ajustando `botPosition("compact", ...)` para `cy = NOTCH_H / 2` (`18`).
  - Nova função pura `compactWidth(hasCenter: boolean, otherCount: number, hasPlanGauge: boolean): number`:
    - Com Centro Contextual ou relógio visível: `COMPACT_W` (`288`).
    - Sem conteúdo central (2A ocioso):
      - `otherCount > 0 && hasPlanGauge`: `188`
      - `otherCount > 0`: `136`
      - `otherCount === 0 && hasPlanGauge`: `138`
      - `otherCount === 0`: `92`
- **Resolução pura do Centro Contextual (`windows/src/core/compact.ts`):**
  - Função `compactCenter(input: CompactInputs): CompactCenterItem | null` avalia na ordem:
    1. **`approval` / `question` (`pendingApproval`):** ponto `#F5A524`, label curto (ex.: `"Aprovação · <agente>"` ou `"Pergunta · <agente>"`), ação `"approval"`.
    2. **Notificação / aviso efêmero contextual (`notice` não expirado):** ex. `"Nota salva"`, `"Backup concluído"`, `"Tarefa concluída"`, `"Resumo de e-mails pronto"`.
    3. **WhatsApp não lido (`whatsappUnread > 0`):** ponto `#22C55E`, ex.: `"WhatsApp · 2 não lidas"` (ou nome da conversa ativa quando houver 1 conversa não lida), ação `"whatsapp"`.
    4. **E-mail não lido (`mailUnread > 0`):** ponto `#38BDF8`, ex.: `"Mail · 3 não lidos"`, ação `"mail"`.
    5. **Próxima tarefa agendada dentro da janela (`todos`, hoje/atrasada até 2h à frente):** ponto `#F5A524` (se atrasada) ou `#22D3EE`, ex.: `"14:00 · <título>"`, ação `{ kind: "task", taskId }`.
    6. **Música tocando (`music`):** ícone `♪` verde `#1ED760`, `"Título · Artista"`, ação `"music"`.
    7. **Sem itens ativos:** retorna `null`. Se `State.settings.showClock` estiver ativo, `#island-clock` permanece no centro com `COMPACT_W`; caso contrário, **zero texto central** e a largura anima para o tamanho 2A enxuto.
- **Comportamento interativo no `Island` (`windows/src/island/island.ts`):**
  - Ouve `"tasks-changed"` (via `register` / `main.ts`) e consulta `TasksBridge.state()` no boot e a cada atualização de tarefas para manter as tarefas pendentes disponíveis no Centro Contextual.
  - Quando uma mensagem/aprovação/aviso aparece e `State.mode === "compact"` (ou ao revelar para `compact`), `#compact-center` exibe o item clicável, expandindo a largura suavemente para `COMPACT_W` e evitando sobreposição com `#mini-grid`, `#compact-plan` e `#island-clock`.
  - Clicar no `#compact-center` abre diretamente a view/integração correspondente (`approval`/`question`, foco no WhatsApp/Mail/Spotify no `overview`, ou `taskReminder`/página de Tarefas).

---

## 3. O que NÃO muda

- Modos `hidden` (`core/closed.ts`, `CLOSED_H = 24`) e constantes já validadas pelo `greeting.test.mjs` (`COMPACT_W = 288`, proporções do greeting).
- Lógica de confirmação editável de Tarefas (`createProposalEditor`) e regras de IA propõe -> usuário confirma.
- Integrações e links reais em `integrations.ts` (nenhuma ação falsa será adicionada).
