> Parte do roadmap do fork. Índice, regras gerais e ordem das fases: [ROADMAP-FORK.md](../ROADMAP-FORK.md).

## Fase L — Lumen + Segunda Mente

Origem: decisões D-050 a D-055 do projeto Segunda Mente (`playertwo1/segundamente-app`, `docs/DECISIONS.md`). As perguntas e respostas completas estão em `docs/questionario/2026-11-13-lumen-desktop-e-todo.md` daquele repositório. Elas mandam sobre esta fase.

Objetivo: este fork vira o **Lumen**, o lado desktop da Segunda Mente. É um produto só com o app Android: as mesmas tarefas, notas, personas e conversas, trocadas pelo sync. O Flutter fica só no celular.

<!-- ARQUIVADO (2026-11-17): as etapas L0 a L7 estão concluídas, com os checkpoints reais fechados em 2026-11-13/15 (L0 inventário, L1 identidade e migração, L2 login ChatGPT, L3 Anotador, L4 Tarefas, L5 E-mail MailCraft, L6 sync com a Segunda Mente, L7 Google). O texto integral, as evidências e os links para specs e planos estão em arquivo/fase-l-l0-l7-concluida.md. -->
> **Concluído e arquivado:** L0 a L7 (inventário, identidade, ChatGPT, Anotador, Tarefas, E-mail, Sync e Google). Texto integral, evidências e links em [arquivo/fase-l-l0-l7-concluida.md](arquivo/fase-l-l0-l7-concluida.md). Também arquivados: a [Fase M0](arquivo/m0-descoberta.md) (concluída) e a [Fase 3](arquivo/fase-3-identidade.md) (decidida dentro da L1).

Regras específicas desta fase, além das do índice:
- **Idioma:** o Lumen é só em pt-BR (D-053.7). Frases novas desta fase vão só em português. A regra das 13 línguas não vale aqui, e as traduções que vieram do upstream ficam como estão.
- **Conteúdo do trabalho:** entra só a agenda, as tarefas e as ideias do dono. Dados de clientes, contas, valores ou documentos internos do trabalho não entram. O e-mail é só a conta pessoal.
- **Nada é enviado sozinho:** e-mail, mensagem ou evento com convidados.
- **A IA propõe e o dono confirma:** tarefa ou data ambígua nunca é gravada sem confirmação.

### L8 — Ajustes do uso real (2026-11-17) — **a fazer agora, antes das fases M**

Origem: o proprietário usou as versões `0.7.8`–`0.7.10` no notebook e listou o que incomodou. Ordem de execução: uma subetapa por vez, cada uma com spec curta em `docs/superpowers/specs/`, testes sintéticos, versão publicada e **validação real do proprietário** antes de passar à próxima. Sintético nunca substitui o relato real.

- [ ] **L8.1 — Ditado "segure uma tecla" inconsistente (M).** "Tem hora que funciona, tem hora que não" (Alt da esquerda, Windows 11). Primeiro achar a causa, depois corrigir.
  - [ ] Reproduzir e registrar: acrescentar linhas de diagnóstico em `coucou.log` (sem texto ditado nem nome de janela) para cada `Down`/`Up`/`Other`/`Escape`, decisão do `KeyLogic`, abertura do microfone, `thinking` e saída (`Pasted`/`Copied`/`Failed`), e pedir ao dono um uso normal de um dia.
  - [ ] Hipóteses a testar, nesta ordem: (1) outro programa ou o próprio Windows consome o Alt da esquerda (menu da janela, barra de título, `Alt+Esc`/`Alt+Tab`) e o gancho não vê o `Up`; (2) tecla "presa" em `logic.is_active()` depois de um `Up` perdido (sem tick de segurança); (3) modelo ainda carregando (`speech::preload`) ou descarregado (`disable` → `unload`) e o pedido cai em "baixe o modelo"; (4) `begin()` falha no microfone sem aviso visível; (5) corrida entre uma dictação em `thinking` e a próxima tecla; (6) a Whisper escolhida sem estar baixada ou o `busy` travado.
  - [ ] Corrigir a causa achada e cobrir com teste de `KeyLogic`/sequência de eventos (incluindo `Up` perdido, tecla repetida e Alt+outra tecla).
  - Checkpoint L8.1: 20 ditados seguidos, em 3 programas diferentes (navegador, chat e editor), sem falha silenciosa; quando falhar, a pílula diz por quê. **Real: NOT_RUN.**
- [ ] **L8.2 — Mãos livres (toque duplo) não funciona (M).** Ligar junto com L8.1, mesma peça (`flow/logic.rs`, `os_windows.rs`). Hoje a tela mostra "Alt da esquerda Alt da esquerda" e "Mãos livres com um toque" desligado.
  - [ ] Reproduzir com o toque duplo e com "um toque": medir o intervalo aceito entre os dois toques, o que acontece com a máscara do Alt (`mask_menu`) no segundo toque e se o `Up` do primeiro fecha a dictação antes do segundo `Down`.
  - [ ] Corrigir e testar a máquina de estados (`Start` → `Lock` → `Finish`) com sequências temporizadas, para o toque duplo e para o de um toque.
  - [ ] A pílula mostra claramente que está em "mãos livres" (`locked`) e como terminar.
  - Checkpoint L8.2: toque duplo inicia, fala longa (30 s) e novo toque cola; o mesmo no modo de um toque. **Real: NOT_RUN.**
- [ ] **L8.3 — Anotador: botão "Virar tarefa" nas ações (P).** Cada item de **Ações / To-do** da nota ganha um botão que cria uma tarefa simples em **Tarefas**: só o título, sem data, hora, prioridade nem alarme. Mais detalhe, o dono edita depois em Tarefas.
  - [x] Reaproveitar o armazenamento de tarefas (`tasks/store.rs`, `Todo/todo.json`) e o caminho de criação existente; guardar a origem (`noteId`) para o item mostrar "já virou tarefa" e **não duplicar** em um segundo clique.
  - [x] O clique do dono é a confirmação; nada vira tarefa sozinho. Sem data inferida.
  - [x] Testes: cria uma tarefa por clique, não duplica, sobrevive a regerar o resumo (o item continua marcado) e a tarefa criada chega ao sync (L6) como qualquer outra.
  - Checkpoint L8.3: uma ação da nota vira tarefa em um clique e aparece em Tarefas sem data. **Real: NOT_RUN.**
- [ ] **L8.4 — Estilos: escolher quem faz a limpeza (M).** Hoje o texto sempre vai para o Claude (`flow/cleanup.rs` → `claude::complete`). Passa a haver uma escolha de motor, no mesmo molde dos resumos do Anotador (`SummaryEngine`): **Claude** (chave ou CLI), **ChatGPT / Codex** (OAuth ou CLI), **Antigravity / Gemini** e **Mesmo provedor do Chat**.
  - [ ] Nova preferência `cleanupEngine` (padrão: o comportamento atual) e seletor na aba **Estilos**; o aviso de privacidade da aba passa a dizer para onde o texto vai conforme o motor (nunca a voz).
  - [ ] Reaproveitar `chat::complete_oneshot` e `cli_relay.rs`/`codex_plan.rs`; manter o tempo máximo curto (4 s na API, 10 s por CLI) e, em qualquer falha, colar o texto como foi ouvido, com a causa no log.
  - [ ] Testes: cada motor com mock, motor sem login cai no texto cru com aviso claro, e o estilo `Do jeito que foi dito` nunca chama motor.
  - Checkpoint L8.4: com cada motor conectado, o mesmo ditado volta limpo; com o motor desconectado, o texto cru é colado. **Real: NOT_RUN.**
- [ ] **L8.5 — Orca em Abas ativas e em Agentes (M).** Orca é ambiente de execução (CLI `orca.exe`, terminais, worktrees). Hoje aparece só na [2B.0.6/2B.7B](roadmap/fase-2b-paridade-agentes.md), que está pausada; esta subetapa puxa de lá só o necessário.
  - [ ] Descoberta curta (só leitura): repetir `orca status --json` e `orca terminal list --json`, anotar o formato e confirmar o hook do Orca coexistindo com o do Lumen. Reaproveitar a fixture sanitizada da 2B.0.6, sem prompt, token nem caminho pessoal.
  - [ ] **Agentes:** linha do Orca com estado (instalado, aberto) e, se o Orca tiver hook próprio, o mesmo fluxo de instalar/reinstalar/desinstalar com backup e diff dos outros agentes; nunca apagar entradas do Orca.
  - [ ] **Abas ativas:** listar os terminais do Orca como abas (nome, agente, estado) e permitir "Abrir terminal" (`orca terminal switch`); nada de enviar texto sem clique.
  - [ ] Criar `windows/src-tauri/src/orca.rs` procurando `orca.exe` no PATH ou em `%LOCALAPPDATA%\Programs\orca\resources\bin\orca.exe`, nunca `orca.cmd`; validar `term_` + UUID antes de usar; nunca ler `last-status.json`, que traz prompt e resposta.
  - Checkpoint L8.5: com o Orca aberto, ele aparece em Agentes e em Abas ativas; fechado ou ausente, some sem erro. **Real: NOT_RUN.**
- [ ] **L8.6 — WhatsApp pela ponte do Orbit (G; é possível).** Resposta à pergunta do dono: **sim, dá para reaproveitar a ponte** (`whatsapp-pj-mcp`, derivada do `whatsapp-mcp` com `whatsmeow`, em `127.0.0.1:8080`) em vez de instalar o GOWA. Ela já expõe `/api/status`, `/api/chats`, `/api/messages` e, para envio, só **rascunhos** (`/api/sends`) que um humano aprova na Approval UI. Ganho: **um só aparelho vinculado** no WhatsApp (a ponte do Orbit) em vez de dois, e nenhum download extra.
  - Limites que valem: a ponte **não é GOWA nem OpenWA**, então o Lumen precisa de um terceiro provedor (`provider.rs` + parser em `gateway.rs`); o Lumen só **lê** e, se for o caso, cria **rascunho** — nunca envia (regra do Orbit D7/15 e do fork); mensagens ficam só na memória, como hoje.
  - [ ] Descoberta (só leitura, sem enviar nada): confirmar o JSON real de `/api/status`, `/api/chats` e `/api/messages`, a forma de autenticação (porta e eventual token), se a ponte aceita dois clientes ao mesmo tempo (Orbit e Lumen) e como avisa mensagem nova (webhook ou só leitura periódica de 20 s, como o fallback atual).
  - [ ] Spec: provedor `orbit-bridge` ao lado de GOWA/OpenWA; em **Integrações → WhatsApp**, "Usar a ponte do Orbit" detecta a porta, mostra o estado e some o botão "Instalar o WhatsApp". Mensagem é **dado não confiável**, nunca instrução.
  - [ ] Implementar leitura e, depois, "Responder" que cria rascunho na ponte e abre a Approval UI (sem enviar). Testes com servidor falso em loopback.
  - Checkpoint L8.6: a ilha mostra as conversas pela ponte do Orbit com o Orbit aberto ao mesmo tempo; nenhuma mensagem é enviada pelo Lumen. **Real: NOT_RUN.**
- Checkpoint L8: os seis itens validados pelo proprietário no notebook, cada um com versão e relato registrados. Depois disso as fases M voltam à fila.

### Depois desta fase
O plano M0–M10 continua como **"Lumen assistente"**, com as mesmas regras M. O modo "mais ativo" (resumos, cobranças e sugestões) é a F14 da Segunda Mente e roda nos dois aparelhos. (QwenPaw) Referência para a F14: o heartbeat, que roda perguntas fixas por intervalo e só dentro de uma janela de horário ativo.
