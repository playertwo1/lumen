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

### Ideias do Speakr (2026-11-25)

Fonte: [`murtaza-nasir/speakr`](https://github.com/murtaza-nasir/speakr), app web auto-hospedado em Python (Flask) que grava ou recebe áudio, transcreve, separa falantes, resume, extrai eventos e responde perguntas sobre a biblioteca de gravações com citações. Estudado na `master` de 2026-11-24. A parte da Segunda Mente está em `docs/fases/estudo-speakr-leon.md` no repositório dela.

**Licença: só ideias, nunca código.** O Speakr é AGPL-3.0 (e exige CLA de quem contribui com ele). Trazer código dele obrigaria o Lumen a virar AGPL. Implementar do zero, sem copiar nem traduzir linha a linha.

Ordem sugerida, se o dono aprovar: L-SP1 (junto da M3) → L-SP2 → L-SP3 → L-SP4.

#### L-SP1 — Citação verificada do trecho (M, encaixa na M3)

- **No Speakr:** `src/services/citations.py`. O texto vai ao modelo com marcadores de segmento (`[S12 12:34 Dana]`) e o modelo cita no formato `[S12 "palavras exatas"]`. O app normaliza (minúsculas, espaços) e confere se a citação aparece de fato naquele segmento: se aparece, vira um link numerado `[1]` que abre o trecho; se não, mantém o número mas marca **não verificada**. Uma gramática só serve à resposta inteira e ao texto que chega aos pedaços (streaming), segurando no máximo 400 caracteres de marcador incompleto.
- **No Lumen hoje:** a M3 (Obsidian) promete "buscar, citar e capturar notas", mas não define como a citação é conferida. O resumo do MailCraft e o do Anotador também não apontam de onde veio cada afirmação.
- **Como seria:** na M3, cada nota lida pelo assistente entra com um id curto (`[N1 caminho/relativo.md]`) e parágrafos numerados; o modelo cita `[N1 "trecho"]`. O Lumen confere o trecho contra o arquivo lido **naquele turno** e mostra `[1]` clicável (abre a nota no Obsidian ou no Lumen) ou o aviso "trecho não encontrado". O mesmo formato serve depois ao resumo do MailCraft (citar a mensagem) e do Anotador.
- **Regras:** "conteúdo externo é dado" (o trecho citado nunca vira instrução); nada do conteúdo vai para o registro local, só o id da nota e se a citação foi verificada.
- **Aceite:** testes do parser em Rust: citação exata, com espaços e maiúsculas diferentes, trecho inexistente, segmento inexistente, marcador cortado no fim do streaming, duas grafias da mesma citação ficando com o mesmo número. Teste real: perguntar algo que está em duas notas e clicar nas duas citações.

#### L-SP2 — Palavras do dicionário como "hotwords" no ditado (P a M, depende de conferência)

- **No Speakr:** vocabulário próprio e hotwords (global, por pasta ou etiqueta, ou na hora de gravar) puxam o reconhecimento para nomes, siglas e jargão que ele erraria.
- **No Lumen hoje:** o ditado já tem dicionário e substituições (`windows/src-tauri/src/flow/store.rs`, aplicados por `text::prepare`), mas só **depois** do reconhecimento. O motor é sherpa-onnx com Parakeet (`Cargo.toml`).
- **Como seria:** conferir primeiro se o modelo e o modo de decodificação usados aceitam hotwords no sherpa-onnx (para modelos transdutores ele oferece lista de hotwords com `modified_beam_search`; a busca gulosa não usa). Se aceitar, passar as palavras do dicionário como hotwords com um peso fixo e manter as substituições como estão. Se não aceitar, registrar N/A e não trocar de motor por isso.
- **Aceite:** 10 frases faladas com nomes e siglas do dicionário (dados sintéticos), contagem de acertos antes e depois, e tempo de reconhecimento que não piore de forma perceptível.

#### L-SP3 — Gravar reunião (microfone + som do computador) para a Segunda Mente (G, insumo da ponte Notetaker)

- **No Speakr:** grava o microfone, o som do sistema ou os dois misturados, com guia por sistema (no Windows: Stereo Mix, VB-Cable, Voicemeeter). A gravação vai ao servidor em pedaços durante a captura, então sessões de horas sobrevivem a recarregar a página.
- **No Lumen hoje:** não grava reunião; a ideia está parqueada como "Ponte Notetaker → Segunda Mente via Google Drive" (acima).
- **Como seria, se a ponte for aprovada:** capturar o som do sistema por WASAPI loopback (API nativa do Windows, sem driver virtual) e o microfone, misturar e gravar em pedaços de alguns minutos no disco local, para que fechar o Lumen ou travar não perca a reunião inteira. O destino é a Segunda Mente (que já importa transcrição com horários); o Lumen não precisa transcrever.
- **Cuidado:** gravar outras pessoas exige que elas saibam. A spec precisa de aviso visível enquanto grava e de um botão claro de parar; nunca começar sozinho. Conteúdo de clientes não entra (regra do fork).
- **Aceite:** gravar 30 minutos sintéticos, matar o processo no minuto 20 e recuperar ao menos os 15 primeiros minutos; nada sai da máquina sem o fluxo escolhido na ponte.

#### L-SP4 — Da nota para o evento no Calendar (P)

- **No Speakr:** "extração de eventos" acha itens de ação e eventos de agenda na transcrição e gera `.ics` (`src/services/calendar.py`).
- **No Lumen hoje:** a L8.3 transforma uma ação do Anotador em tarefa; a L7 cria evento no Calendar com confirmação. Falta ligar os dois para eventos.
- **Como seria:** no Anotador, uma frase com data e hora claras ("reunião com o contador dia 3 às 14h") ganha o botão "Virar evento", que abre o cartão de evento da L7 já preenchido. Data ou hora ambígua segue a regra de sempre: fica para o dono preencher. Sem `.ics`: o Calendar já é o destino.
- **Aceite:** teste da extração com data explícita, ambígua e ausente; teste real criando um evento a partir de uma nota.

#### Já coberto ou não se aplica (não fazer)

- **Aviso de envio sem criptografia** (`src/config/outbound_warnings.py`: avisa quando um serviço remoto usa `http://` fora da máquina): os endereços dos provedores do Lumen são fixos e em `https` (`openai_compat.rs`). Vale para a Segunda Mente, que aceita base URL própria.
- **Resumo por modelo, chat sobre a gravação, busca na biblioteca:** o Anotador e o chat já cobrem o que o Lumen precisa; a biblioteca de gravações é papel da Segunda Mente.

#### Rejeitado

- **Servidor web multiusuário, SSO, grupos, compartilhamento e links públicos, API REST, webhooks, cotas por usuário e registro de auditoria de admin:** o Lumen é um app de uma pessoa; nada disso tem caso de uso aqui.
- **Separação de falantes e perfis de voz (WhisperX):** precisa de servidor com GPU e guarda impressão de voz de terceiros. Se um dia vier, é na Segunda Mente (lá já é ideia futura).
- **Pasta "buraco negro" que importa áudio sozinha:** o ditado e o Anotador não trabalham com arquivos de áudio soltos; para a Segunda Mente o caminho é o compartilhar.
- **Reuso de código:** AGPL-3.0 (ver acima).

### Ideias do Leon (2026-11-25)

Fonte: [`leon-ai/leon`](https://github.com/leon-ai/leon), assistente pessoal em Node.js/TypeScript (com partes em Python), na reescrita 2.0 (branch `develop`, estudada em 2026-11-26 pelo horário do autor). O resumo da arquitetura está em `core/context/ARCHITECTURE.md` dele; o laço do agente em `server/src/core/llm-manager/llm-duties/react-llm-duty/`, a memória em `server/src/core/memory-manager/` e o assistente proativo em `server/src/core/pulse-manager/pulse-manager.ts`.

**Licença: MIT.** Dá para copiar código, desde que o aviso de copyright do Leon ("Copyright (c) 2019-present, Louis Grenard") vá junto do trecho copiado e na lista de licenças de terceiros. Na prática o núcleo dele é Node e o do Lumen é Rust, então quase tudo é ideia; só um trecho TypeScript da interface poderia vir direto. O nome, o logotipo e o personagem do Leon não entram.

**O Leon é o projeto mais parecido com as fases M.** Os princípios batem: ferramentas declaradas em vez de shell livre, cada passo registrado, verificar o resultado. Por isso a maior parte das ideias abaixo melhora o laço do assistente que já existe.

Ordem sugerida, se o dono aprovar: L-LE1 (antes da M5/M6, que têm mais passos) → L-LE2 e L-LE3 (com a M5/M6) → L-LE4 → L-LE5 (dentro da M9.2).

#### L-LE1 — Guardas no laço do assistente (M)

- **No Leon:** antes de executar, o laço valida e **repara** argumentos pelo schema, **bloqueia chamadas repetidas** idênticas, devolve **todo** sucesso ou falha como observação estruturada ao mesmo laço, tem prazo total por turno (modelo, tentativas, ferramentas e revisão dividem o mesmo prazo e reservam tempo para a resposta) e, ao estourar, relata o que ficou incompleto em vez de continuar.
- **No Lumen hoje:** `mochi/chat_bridge.rs` limita o laço a `MAX_TOOL_STEPS = 8` e o registro (`mochi/registry.rs`) valida o schema e tem timeout por ferramenta. Não há bloqueio de chamada repetida nem prazo do turno inteiro.
- **Como seria:**
  - **Chamada repetida:** a mesma ferramenta com os mesmos argumentos (JSON normalizado) no mesmo turno não executa de novo; o modelo recebe "já executada, resultado: …". Evita, por exemplo, dois "fechar janela" seguidos.
  - **Reparo de argumentos, só o trivial:** número em texto ("20", "20%") vira número quando o schema pede inteiro; espaços nas pontas saem. Nada é inventado: argumento ausente continua erro.
  - **Prazo do turno:** por exemplo 60 s no total (definir na spec), com o resumo final garantido; ao estourar, a resposta diz o que foi feito e o que faltou.
  - **Cancelamento:** a pausa rápida da M1 já corta; conferir que ela também interrompe o laço no meio.
- **Aceite:** testes em `chat_bridge.rs` com modelo falso que repete a chamada, que manda "20%" como string, e que nunca para (estoura o prazo e o relato sai correto).

#### L-LE2 — Carregar as ferramentas aos poucos (M, quando houver muitas)

- **No Leon:** o modelo vê primeiro só os grupos de ferramentas (toolkits) com uma linha cada; o schema completo de uma função só entra no contexto quando ela vai ser usada, e schemas de grupos parados são compactados.
- **No Lumen hoje:** são poucas ferramentas (M2) e todas vão no pedido. Com a M3 (Obsidian), M5 (acessibilidade) e M6 (web), a lista cresce e cada pedido fica mais caro e mais lento.
- **Como seria:** agrupar por área (sistema, notas, interface, web). O primeiro pedido leva só os grupos; uma ferramenta interna `abrir_grupo` devolve os schemas do grupo pedido. O registro continua negado por padrão: abrir um grupo não habilita nada.
- **Quando:** só se a medição mostrar ganho. Medir tokens e tempo do primeiro pedido com todas as ferramentas da M6 antes de decidir.

#### L-LE3 — Resultado grande vira artefato com prévia (M, com a M3/M6)

- **No Leon:** saída grande de ferramenta fica num registro de artefato; o modelo recebe uma prévia limitada e um identificador para pedir mais páginas. O contexto tem orçamento fixo e trocas antigas são compactadas quando preciso.
- **No Lumen hoje:** o resultado da ferramenta vai inteiro em `<mochi_tool_result>`. Para volume e brilho isso é pequeno; uma página web (M6) ou uma nota longa (M3) não é.
- **Como seria:** acima de um limite (definir na spec), o resultado fica só na memória do processo durante a conversa e o modelo recebe as primeiras linhas, o tamanho total e uma ferramenta `ler_mais(id, página)`. Nada vai para disco nem para o registro local.
- **Aceite:** teste com resultado de 200 KB: o pedido ao modelo fica abaixo do limite e a página 3 traz o trecho certo.

#### L-LE4 — Pausa para perguntar sem perder o plano (P a M)

- **No Leon:** quando falta uma informação, o laço pausa com o plano e o estado guardados; a resposta do dono retoma a mesma tarefa.
- **No Lumen hoje:** se o modelo pergunta algo no meio, a resposta do dono começa um turno novo, e o que já foi feito no turno anterior só aparece como histórico.
- **Como seria:** uma ferramenta interna `perguntar_ao_dono(pergunta, opções?)` encerra o turno marcando-o como "aguardando"; a próxima mensagem retoma com as observações já obtidas. Útil para a M4 (voz: "qual dos dois monitores?") e a M8 (rotinas).
- **Aceite:** teste com modelo falso que pergunta no passo 2 e termina no passo 3 depois da resposta, sem repetir o passo 1.

#### L-LE5 — Memória do Leon como candidata na M9.2 (avaliação)

- **No Leon:** memória em camadas (contexto temporário, fatos e preferências duráveis, resumo diário, memória de conversa), separadas por perfil; um `OWNER.md` legível e curado como perfil do dono; extração em segundo plano que reconcilia correções com os registros relevantes e **rejeita substituições velhas** (uma informação antiga não apaga uma mais nova); busca pelo QMD, com índice atualizado só onde mudou.
- **Para a M9.2:** entra na tabela ao lado de Mem0 e Graphiti como "modelo de memória sem serviço novo". O dono já usa o QMD no cofre, e um `OWNER.md` legível e editável combina com a regra de nada escondido. Avaliar com as mesmas 10 perguntas da M9.1.
- **Cuidado:** a extração automática roda depois de cada conversa e gasta a assinatura; na avaliação, medir o custo e considerar extrair só quando o dono pedir ("lembra disso").

#### Rejeitado

- **Pulse (o assistente investiga sozinho em segundo plano e decide quando falar):** gasta a assinatura sem pedido e quebra "0 % de CPU com a ilha escondida", pelos mesmos motivos da cristalização automática do Understudy. A iniciativa do assistente mora na Segunda Mente (F14); as regras de frequência do Pulse foram registradas lá.
- **Experiências aprendidas sozinhas** (procedimentos salvos como `SKILL.md` depois de cada turno, até 196): é a cristalização automática já rejeitada; a M8.2 continua com "salvar como rotina" aprovado pelo dono.
- **Ferramenta de shell e agentes de código como ferramenta:** contraria "Ferramenta, não shell".
- **Telemetria:** o Leon vem com `telemetry_enabled: true` no `config.sample.yml`. O Lumen não tem telemetria.
- **Modelo local escolhido por memória disponível (llmfit, llama.cpp):** fora das regras das fases M.
- **Servidores MCP externos instalados pelo dono, com OAuth:** está no parqueado de conectores; o Lumen só expõe o próprio servidor MCP local.
- **Interface gerada pelo modelo (Aurora: HTML, CSS e JavaScript gerados num quadro isolado):** risco alto para pouco ganho num chat de ilha. Só reabrir com caso de uso (por exemplo um gráfico de cotas) e, mesmo assim, preferir componentes fixos.
- **Browser Use e Cua para web e computer use:** a M5 e a M6 já escolheram Agent Desktop e Agent Browser. Só voltam como alternativa se esses falharem na avaliação, conferindo licença antes.
- **Satellite (um servidor central e aparelhos que executam ferramentas):** o Lumen é o próprio aparelho; a M7 usa a ligação com o celular que já existe.
- **Humor do assistente influenciado pelo clima ("Vibes") e diário interno do assistente:** exige serviço de clima não configurado e não ajuda nas tarefas do dono.

### Fora de escopo do Mochi por decisão (2026-11-09)

- Plugin Obsidian Local REST API: o cofre é lido direto pelos arquivos.
- Chave de API paga obrigatória e modelo local como cérebro.
- Acesso do celular pela internet (fora da rede local).
- Agendamento de rotinas por horário.
