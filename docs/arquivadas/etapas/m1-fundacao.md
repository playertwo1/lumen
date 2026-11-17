> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../../ROADMAP-FORK.md). Regras gerais e das fases M: [AGENTS.md](../../../AGENTS.md).

## Fase M1 — Mochi: fundação (G)

Objetivo: o caminho completo "pedido → cérebro → ferramenta → resultado" funcionando com uma única ferramenta de teste inofensiva. Nenhuma ferramenta real nesta fase.

Depende de: Checkpoint M0.

Antes de M1.1: escrever spec e plano em `docs/superpowers/`, fixando nomes de módulos e arquivos a partir do mapa da M0.6. Uma subfase por commit.

### M1.1 — Contrato e registro de ferramentas

- [x] Definir o contrato de ferramenta descrito em "Regras das fases M" do AGENTS.md.
- [x] Criar o registro: lista de ferramentas habilitadas; o que não está na lista não existe para o modelo.
- [x] (Understudy) Ferramenta cujo pré-requisito falta (adaptador ausente, permissão negada, fase desligada) não é listada ao modelo, em vez de falhar só na execução. Testar com uma ferramenta de teste marcada como indisponível.
- [x] Criar a ferramenta de teste `ping`, que só devolve um texto fixo (risco baixo).
- [x] Testar: ferramenta desconhecida recusada; argumento fora do schema recusado; timeout encerra a chamada.
- Checkpoint M1.1: testes passam; nenhuma ferramenta executa texto como comando. *(Sintético em 2026-11-16: `windows/src-tauri/src/mochi/registry.rs`, `mod.rs`; `cargo test --lib mochi` passou.)*

### M1.2 — Motor de permissões

- [x] Implementar os três níveis de risco e a regra de clique do AGENTS.md.
- [x] Decidir e registrar onde ficam as permissões "sempre permitir" (arquivo de settings atual ou outro). *(Arquivo dedicado `%APPDATA%\Coucou\mochi-permissions.json` com `deny_unknown_fields` e marcador `.init` em `%LOCALAPPDATA%\Coucou\mochi-permissions.init`, preservando `settings.json`.)*
- [x] Testar: baixo executa; médio pede na primeira vez e respeita "sempre permitir"; alto pede sempre; revogar volta a pedir; sem resposta não executa.
- [x] (QwenPaw) Configuração de permissões ausente, corrompida ou com valor desconhecido nega tudo (falha fechada). O QwenPaw cai em `AUTO` nesse caso; não repetir. Testar com arquivo inválido.
- Checkpoint M1.2: não existe caminho de código que execute médio ou alto sem decisão registrada. *(Sintético em 2026-11-16: `PermitToken` com campo privado em `mochi/permissions.rs` + `ExecutionContext::for_tool`; UI de status/revogação/restauração em Configurações → Geral.)*

### M1.3 — Cartão de confirmação

- [x] Reutilizar o cartão Permitir/Negar existente da ilha, sem mudar o visual.
- [x] Mostrar: ferramenta, o que será feito em uma frase, nível de risco.
- [x] Frases novas em `windows/src/i18n/extra.json`, nas 13 línguas.
- [x] Testar: Permitir, Negar, fechar, timeout e dois pedidos seguidos.
- Checkpoint M1.3: cada clique decide só o pedido mostrado; timeout equivale a negar. *(Sintético em 2026-11-16: `mochi/approval_bridge.rs`, `pipe.rs`, `views.ts`, `windows/tests/mochi-approval.test.mjs`, `i18n.test.mjs`; **Real em runtime: NOT_RUN**.)*

### M1.4 — Registro local

- [x] Gravar por chamada: ferramenta, hora, risco, decisão e resultado.
- [x] Não gravar argumentos com texto livre, conteúdo de tela, tokens nem caminhos pessoais.
- [x] Oferecer "limpar registro" em Settings.
- [x] Testar que um segredo passado como argumento não aparece no arquivo.
- Checkpoint M1.4: registro legível, só local, e limpo pelo botão. *(Sintético em 2026-11-16: `mochi/audit.rs`, `settings/main.ts`, `windows/tests/mochi-audit.test.mjs`.)*

### M1.5 — Servidor MCP local

- [x] Expor o registro de ferramentas por MCP, somente local (stdio ou loopback); nunca em interface de rede externa.
- [x] Toda chamada recebida passa pelo motor de permissões da M1.2.
- [x] Registrar o servidor na configuração de cada CLI aprovada na M0.2, com backup datado, diff visível e clique.
- [x] Testar: chamada de fora do loopback recusada; remover o registro restaura o arquivo original.
- Checkpoint M1.5: a CLI lista a ferramenta `ping` e a chama; a configuração de outras ferramentas fica intacta. *(Sintético em 2026-11-16: `mochi/mcp_server.rs`, `mcp_config.rs`, `windows/hook/src/mcp.rs`, `settings/mcp.ts`, `windows/tests/mochi-mcp.test.mjs`. **Real com CLI externa em 2026-11-17:** o Claude Code listou a ferramenta `lumen/ping` e a chamou, resposta `{"status":"ok","pong":"lumen-mochi-m1"}`.)*

### M1.6 — Ligação com o chat e pausa rápida

- [x] Um pedido no chat da ilha chega ao cérebro com as ferramentas disponíveis e a resposta volta ao chat.
- [x] (Understudy) Texto externo embutido no pedido ao cérebro (título de janela, nome de arquivo, nome de app) tem caracteres de controle, invisíveis e quebras de linha removidos antes. Testar com um título que contém quebra de linha e uma instrução falsa.
- [x] Mostrar na ilha o estado: pensando, aguardando clique, executando, feito, falhou.
- [x] Implementar a pausa rápida do AGENTS.md e um botão de cancelar a execução em curso.
- [x] Testar: cérebro indisponível vira erro visível; pausa bloqueia `ping`; cancelar interrompe.
- Checkpoint M1.6: "faça um ping" no chat executa `ping` e mostra o resultado. *(Sintético com mock de cérebro em `mochi/chat_bridge.rs` e `windows/tests/mochi-chat.test.mjs`; **Real no chat em 2026-11-17:** na `0.7.13` com ChatGPT (`gpt-6-luna`) o modelo respondeu "ferramenta indisponível" sem chamar o `ping` (auditoria vazia); a instrução de ferramentas ficou explícita em `chat_bridge.rs` e, em modo dev, "faca um ping" respondeu "Pong! O núcleo local do Lumen está ativo." com estado "Feito (ping)". Falta confirmar numa release; o dono achou lento.)*

- [x] Checkpoint M1 (runtime manual no notebook) — **fechado por decisão do dono em 2026-11-17:** conversa, `ping` pelo chat (modo dev, com a correção do `chat_bridge.rs`) e `ping` por CLI externa via MCP validados no uso real; os cenários "pedido fora da lista", "pausa rápida" e "provedor fora do ar" ficaram sem teste real por escolha do dono (cobertos só pelos testes sintéticos).: o app abre, conversa em texto, mostra o estado e não executa nada fora da lista habilitada. **Sintético fechado em 2026-11-16:** `cargo test --lib` (`821 passed; 0 failed; 9 ignored` com manifesto `resource.lib`), `windows/hook` `cargo test` (`42 passed; 0 failed`), `npx tsc --noEmit` (OK), `npm test` (`750 passed; 0 failed`) e `npm run build` (OK). **Real: `NOT_RUN`**.
