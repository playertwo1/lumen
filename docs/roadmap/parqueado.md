> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../ROADMAP-FORK.md). Regras gerais e das fases M: [AGENTS.md](../../AGENTS.md).

## Parqueado — não iniciar sem escolher escopo

### Segunda Mente no Windows

- [ ] Escolher um escopo: captura+relatório, somente leitura ou paridade total.
- [ ] Ler D-042 em `segundamente/docs/DECISIONS.md` e listar plugins Flutter sem suporte Windows.
- [ ] Criar spec que define o que funciona sem esses plugins e quais dados ficam locais.
- Checkpoint: sem escopo e decisão aprovados, não criar target Windows nem substituir plugins.

### Ponte Notetaker → Segunda Mente via Google Drive

- [ ] Decidir salvar automaticamente ao parar a gravação ou perguntar a cada chamada.
- [ ] Definir aviso, opt-in, formato, diretório e comportamento sem Drive instalado/conectado.
- [ ] Testar que nada é enviado sem opt-in e que reimportar não duplica a transcrição.
- Checkpoint: integração externa só começa após escolha do fluxo e aprovação da spec.

### Conectores externos do pacote Mochi

- [ ] n8n, PowerToys, LocalSend e KDE Connect: só voltam com um caso de uso real descrito pelo proprietário.
- [ ] Antes de qualquer integração, rever a licença e o custo de manter o conector.
- Checkpoint: sem caso de uso aprovado, não instalar nem integrar.

### Ensinar rotina por demonstração (ideia do Understudy)

O usuário faz a tarefa uma vez, o app grava e o modelo extrai uma rotina com parâmetros.

- [ ] Só volta depois do Checkpoint M8 e com um caso de uso real em que "salvar como rotina" (M8.2) não baste.
- [ ] Versão aceitável: gravar só eventos de acessibilidade (app, janela, controle, ação), sem vídeo, sem captura de tela e sem texto digitado; o proprietário revisa os passos antes de qualquer envio ao cérebro.
- Motivo do parqueamento: no Understudy isso depende de vídeo da tela, monitor global de teclado e mouse e envio de quadros ao provedor do modelo. Isso captura senhas e conteúdo privado e contraria as regras de registro e de conteúdo de tela das fases M. O código é Swift, só macOS.
- Checkpoint: sem spec aprovada com esse limite de captura, não gravar nenhum evento global.

### Ideias do Understudy rejeitadas (2026-11-11)

Fonte: `understudy-ai/understudy` (MIT). Só reabrir com decisão explícita do proprietário.

- Ferramenta `bash`/shell livre: contraria "Ferramenta, não shell".
- Localizar controle por captura de tela com segundo modelo (grounding visual): exige um modelo de visão chamado por chave de API a cada clique, envia capturas de tela ao provedor e adivinha posição, o que a M5.5 proíbe. O cérebro por assinatura/CLI não serve para isso.
- Oito canais de mensagem (Telegram, Slack, WhatsApp…) e gateway HTTP/WebSocket: é acesso de fora da rede local, já excluído em 2026-11-09, e a M1.5 só permite stdio ou loopback. O celular já entra pela ligação local.
- Anexar ao Chrome do usuário por extensão (abas já logadas): contraria o perfil isolado da M6.
- Agendamento por cron: já excluído em 2026-11-09.
- Cristalização automática (analisar o histórico de conversas em segundo plano e publicar rotina sozinho): guarda o texto das conversas, gasta a assinatura sem pedido, quebra "0 % de CPU com a ilha escondida" e salva rotina sem clique (M8.2). A parte útil virou "salvar como rotina" na M8.2.
- Níveis de autonomia que sobem sozinhos (`manual` → `full_auto`): no Understudy é só projeto, sem código de promoção; aqui o risco alto pede clique toda vez e isso não é negociável.
- Observação passiva contínua e sugestões proativas: vigilância permanente da tela, sem implementação no próprio Understudy.
- Subagentes, playbooks e workers: a rotina da M8 é uma lista plana de ferramentas; orquestração em estágios não tem caso de uso.
- Reuso de código: runtime Node sobre `pi-agent-core` e helper Swift para macOS; nada encaixa no Tauri/Rust.

### Ideias do MyBrain (2026-11-25)

Fonte: [`mhss1/MyBrain`](https://github.com/mhss1/MyBrain), app Android em Kotlin Multiplatform (tarefas, notas, diário, favoritos, calendário e um assistente com ferramentas). Estudado no commit da `main` de 2026-10-03. A parte que serve à Segunda Mente está registrada no repositório dela, em `docs/fases/estudo-mybrain.md`.

**Licença: só ideias, nunca código.** O MyBrain é GPL-3.0. O Lumen vem do Coucou (MIT) e é público: copiar ou traduzir linha a linha um trecho do MyBrain obrigaria a distribuir o Lumen inteiro como GPL. Ideia, fluxo de tela e regra de negócio podem ser reaproveitados; implementar do zero em Rust e TypeScript, sem abrir o código dele ao lado durante a escrita. Ícones do MyBrain (Flaticon, com atribuição obrigatória) também não entram.

Ordem sugerida, se o dono aprovar: L-MB1 (pequena e independente) → L-MB2 → L-MB3 → L-MB4 → L-MB5. Nenhuma reabre sem decisão do dono e sem spec em `docs/superpowers/specs/`.

#### L-MB1 — Limpar as lápides das tarefas depois de 90 dias (P)

- **No MyBrain:** `local-sync/.../sync/domain/CleanupDeletedEntitiesUseCase.kt` apaga os registros de exclusão com mais de 90 dias (`TOMBSTONE_RETENTION = 90.days` em `SyncConstants.kt`), num trabalho que roda uma vez por dia.
- **No Lumen hoje:** a exclusão é lápide (`deleted_at` em `windows/src-tauri/src/tasks/model.rs`, `TaskStore::delete` em `store.rs`), e nada a remove. O arquivo de tarefas só cresce. O sync já poda os `mutation_id` aplicados (`prune_applied_mutations` em `tasks/sync.rs`), mas não as tarefas apagadas.
- **Contrato:** o `CONTRATO-SYNC.md` da Segunda Mente (seção de exclusão) manda guardar a lápide **no mínimo** 90 dias. O celular já poda depois disso (`_isExpiredTombstone` em `lib/data/services/todo_sync_service.dart`). O Lumen está dentro do contrato, só nunca limpa.
- **Como seria:** ao carregar o `TaskStore` (e no máximo uma vez por dia, sem timer novo, para manter 0 % de CPU com a ilha escondida), remover as tarefas com `deleted_at` há 90 dias ou mais **que não estejam na outbox nem em conflito aberto**. Na mesma passada, tirar do estado do sync o que só servia a essas tarefas (base da merge de 3 vias, `extra_fields`, conflitos órfãos).
- **Risco conhecido:** um aparelho que fique mais de 90 dias sem sincronizar e ainda tenha a tarefa viva pode ressuscitá-la. É o mesmo risco que o celular já aceita; registrar na spec.
- **Aceite (testes em `tasks/store.rs` e `tasks/sync.rs`):** lápide com 91 dias some; com 89 fica; lápide pendente na outbox fica mesmo com 91 dias; tarefa viva nunca é tocada; recarregar duas vezes não muda nada (idempotente); um envelope remoto antigo de uma tarefa podada não a recria como lápide nem como tarefa viva se a versão dele for a da lápide. `cargo test --lib`.
- **Rollback:** a remoção só acontece no arquivo local. O backup semanal no Drive (L7) guarda a versão anterior.

#### L-MB2 — Ferramentas de tarefas no assistente (M)

- **No MyBrain:** `ai/data/.../tools/TaskToolSet.kt` dá ao modelo `search_tasks`, `create_task`, `create_multiple_tasks` e `update_task_completed`; há conjuntos iguais para notas, diário, favoritos e calendário. Ele **grava direto**, sem confirmação. Isso não vem para cá.
- **No Lumen hoje:** o registro de ferramentas (`windows/src-tauri/src/mochi/registry.rs`) só tem controle do Windows (`mochi/tools/`: áudio, brilho, apps, janelas, mídia, energia) e o `ping`. A proposta de tarefa existe, mas fora do assistente: `tasks/proposal.rs` gera o cartão a partir do ditado ou do atalho.
- **Como seria, seguindo as regras das fases M:**
  - `tarefas_buscar` (risco baixo, só leitura): busca por título, devolve id, título, data, prioridade e se está feita. Sem notas privadas no registro local.
  - `tarefas_propor` (risco baixo, não grava): reaproveita `parse_llm_proposal` e a mesma regra de data/hora ambígua; o resultado é o cartão de confirmação que já existe, e a tarefa só nasce no clique.
  - `tarefas_concluir` (risco médio: mudança persistente, clique na primeira vez com "sempre permitir"): marca feita e enfileira no sync como o botão da lista faz.
  - Cada uma com `verify` que relê o `TaskStore` depois da ação e `audit_summary` sem o título da tarefa (só id e resultado).
- **Aceite:** testes do registro (schema, risco, negado por padrão) e do `verify`; teste real pelo chat: "quais tarefas tenho para amanhã?", "me lembra de pagar a conta sexta" (tem que sair cartão com data ambígua, sem gravar) e "marca a tarefa X como feita" (pede clique na primeira vez).
- **Depende de:** decisão do dono de dar ao assistente acesso às tarefas; encaixa depois da M2 e antes da M8 (rotinas usariam essas ferramentas).

#### L-MB3 — Propor várias tarefas de uma vez (M)

- **No MyBrain:** `create_multiple_tasks` cria uma lista inteira numa chamada ("organiza minha semana" vira 6 tarefas).
- **No Lumen hoje:** `build_proposal_prompt` em `tasks/proposal.rs` extrai **uma** proposta por fala. Ditar "comprar pão, ligar pro banco e renovar o seguro" vira uma tarefa só com o texto inteiro.
- **Como seria:** o prompt passa a devolver uma lista (máximo 10 itens), cada item com as mesmas regras de `date_state`/`time_state`. O cartão mostra os itens com uma caixa de seleção cada, edição por item e um botão "Criar selecionadas". Nada é gravado sem o clique; uma data ambígua num item não bloqueia os outros, só aquele fica marcado para revisar. Com um item só, o cartão é o de hoje (não mexer no visual existente).
- **Aceite:** testes de `parse_llm_proposal` com lista, lista vazia, 11 itens (corta em 10 e avisa), item sem título (descarta) e JSON de um item só (compatível com o formato atual). Teste real: ditar três tarefas numa frase e desmarcar uma antes de criar.
- **Vale também para a Segunda Mente** (`create_todo_proposal` também é um item por chamada); combinar o formato do cartão nos dois lados para não divergir.

#### L-MB4 — Cartão do resultado no chat depois da ferramenta (M)

- **No MyBrain:** `ai/data/.../repository/AiToolExecutor.kt` (`extractResultObject`), depois de uma ferramenta criar ou achar um item, lê o item de verdade no banco e mostra um cartão dele na conversa (`ai/presentation/.../components/AiResultItemCards.kt`), além da resposta em texto.
- **No Lumen hoje:** `mochi/chat_bridge.rs` devolve o resultado ao modelo em `<mochi_tool_result>` e `strip_tool_tags` o remove da resposta; o dono só vê a frase do modelo. Se o modelo resumir errado ("pronto, volume em 20 %" quando a ferramenta falhou), não há nada na tela que mostre o real.
- **Como seria:** cada `ToolDefinition` pode, opcionalmente, devolver um resumo de exibição feito a partir do `verify` (o estado relido depois da ação, nunca o que o modelo disse). O chat mostra uma linha compacta abaixo da resposta: ferramenta, resultado (ok/falhou) e o valor lido, por exemplo "Volume: 20 %" ou, com a L-MB2, o cartão da tarefa com "Abrir". Falha aparece como falha, mesmo se o texto do modelo disser outra coisa.
- **Visual:** é um elemento novo no chat; segue o estilo existente e não muda as bolhas atuais.
- **Aceite:** teste de que o resumo vem do `verify` e não da resposta do modelo; teste real com uma ferramenta que falha (app inexistente) e outra que funciona.

#### L-MB5 — Anexar uma tarefa ou nota ao chat (P a M)

- **No MyBrain:** `AttachTaskSheet.kt` e `AttachNoteSheet.kt` deixam escolher uma tarefa ou nota e mandá-la como contexto da mensagem ("me ajuda a quebrar esta tarefa em passos").
- **No Lumen hoje:** o chat aceita arquivos arrastados, mas não itens do próprio Lumen (tarefas do Hub, notas do Anotador).
- **Como seria:** um botão de anexo no chat abre a lista de tarefas e notas recentes, com busca. O item vai como texto marcado de dado (título, data, subtarefas ou o corpo da nota), nunca como instrução, seguindo a regra "conteúdo externo é dado". Antes de enviar, o anexo aparece na caixa de mensagem e pode ser removido, porque vai para o provedor do cérebro.
- **Aceite:** teste de que o conteúdo anexado vai delimitado como dado; teste real com uma tarefa com subtarefas e uma nota longa (corta e avisa acima de um limite definido na spec).

#### Já coberto no Lumen ou na Segunda Mente (não fazer)

- **Data rígida para a IA com erro explícito** (`parseDateTimeFromLLM`): o Lumen já é mais rigoroso, com `date_state`/`time_state` e "ambíguo nunca é gravado" (`tasks/proposal.rs`).
- **Sync local por NSD, WebSocket, AES-GCM, zstd e QR** (`local-sync/`): o Lumen já tem LAN TLS, relay com cifra ponta a ponta e pareamento por QR (L6, `phone_link/`). O merge do MyBrain é "o mais recente ganha" por `updatedDate`; o Lumen faz merge de 3 vias com conflito por campo. Trocar seria retrocesso.
- **Subtarefas, prioridade, repetição e lembrete:** já existem (`tasks/model.rs`, avisos Toast da L4).
- **Diário com humor e favoritos pelo menu compartilhar:** a Segunda Mente já tem cartões de humor e de link na linha do tempo e recebe conteúdo pelo compartilhar.
- **Backup e exportação:** o Lumen já faz backup semanal no Drive com restauração verificada (L7).

#### Rejeitado

- **IA gravando tarefas e notas direto, sem cartão:** contraria "IA propõe, usuário confirma".
- **Gemini Nano no aparelho (ML Kit GenAI) como cérebro:** é modelo local, fora das regras das fases M; e só existe no Android.
- **Android AppFunctions:** API só do Android; não se aplica ao Windows. Avaliada no lado da Segunda Mente.
- **Koog, Koin, Room, Compose e a arquitetura MVI:** pilha Kotlin, sem equivalente útil no Tauri/Rust.
- **Reuso de código ou de ícones:** licença GPL-3.0 e atribuição Flaticon (ver acima).

### Fora de escopo do Mochi por decisão (2026-11-09)

- Plugin Obsidian Local REST API: o cofre é lido direto pelos arquivos.
- Chave de API paga obrigatória e modelo local como cérebro.
- Acesso do celular pela internet (fora da rede local).
- Agendamento de rotinas por horário.
