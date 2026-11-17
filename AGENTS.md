# Lumen (playertwo1/lumen, antes fork playertwo1/coucou) — guia para agentes de código

Este arquivo é a porta de entrada para qualquer agente (Codex, Claude Code, Gemini) que trabalhe **neste fork**. O `CLAUDE.md` da raiz vem do projeto original. As regras de código dele continuam valendo, e este arquivo acrescenta as do fork. Se os dois conflitarem num ponto do fork, vale este.

## O que é este repositório

- Fork público de [Louis-CFM/coucou](https://github.com/Louis-CFM/coucou) (código MIT). O trabalho do fork é o app **Windows** em `windows/`: Tauri 2, com Rust em `src-tauri/` e TypeScript em `src/`.
- O fork virou o **Lumen**, o lado desktop da Segunda Mente (decisões D-050 a D-055 da Segunda Mente, 2026-11-13). A Segunda Mente é um app Android em Flutter, num repositório privado separado (`playertwo1/segundamente-app`). Os dois trocam tarefas, notas, conversas e personas por sync.
- Nome, personagem, ícone e sons do Coucou/Mochi **não** são livres (`LICENSE-ASSETS.md`). O Lumen usa nome e assets próprios desde a L1.

## Antes de agir, leia nesta ordem

1. [docs/STATE-FORK.md](docs/STATE-FORK.md): estado atual, pendências e próxima ação.
2. [docs/ROADMAP-FORK.md](docs/ROADMAP-FORK.md): índice das etapas, com onde cada uma está e um resumo. Depois abra **só** o arquivo da etapa em que vai trabalhar.
3. [CLAUDE.md](CLAUDE.md): mapa do código e regras herdadas do original.
4. Antes de código com UX ou comportamento novo: uma spec curta em `docs/superpowers/specs/` e um plano em `docs/superpowers/plans/`.

## Mapa do que importa ao fork

| Caminho | Conteúdo |
|---|---|
| `windows/src-tauri/src/` | Backend Rust: ilha, chat, `flow/` (ditado), `notes/` (Anotador), `mail/` (Outlook e MailCraft), `tasks/`, `phone_link/` (ligação com o Android: LAN TLS e relay), `mochi/`, `speech/`, `voice/` |
| `windows/src/` | Interface TypeScript; frases novas em `windows/src/i18n/extra.json` |
| `windows/tests/`, `windows/synthetic/` | Testes e app sintético de validação |
| `relay/`, `android/relay/` | Workers do Cloudflare (relay sem estado e a caixa de correio do sync) |
| `docs/ANDROID_LINK.md`, `docs/RELAY_LINK.md` | Protocolo de ligação com o celular, base do sync com a Segunda Mente |
| `docs/roadmap/` | Um arquivo por etapa **aberta, pausada ou a fazer** |
| `docs/arquivadas/` | O que já passou: etapas concluídas (`etapas/`), STATE antigo (`state/`), registro de releases (`releases/`), decisões encerradas (`decisoes/`), pesquisas já aproveitadas (`pesquisas/`) e specs/planos de etapas concluídas (`superpowers/`) |
| `docs/questionario/` | Decisões tomadas por questionário com o dono que ainda guiam etapas abertas |
| `docs/superpowers/` | Specs e planos das etapas abertas ou pausadas |
| `NotchBuddy/`, `android/`, `linux/` e os demais `docs/*.md` em inglês | Código e documentação do original (Mac, Android do Coucou, Linux); o fork não mexe sem pedido |

## Regras do fork

- **Evidência:** checkpoint só fecha com o resultado exigido. O sintético não substitui a validação real. O que não foi executado não está aprovado. Não marque uma feature como concluída só porque compila.
- **Escopo:** uma subetapa por vez, na ordem do ROADMAP-FORK. Etapas pausadas não reabrem sem decisão do dono.
- **Repositório público:** não registrar dados pessoais do dono, conteúdo do trabalho dele, caminhos pessoais, tokens nem nada de clientes, em código, docs, fixtures ou logs. Use dados sintéticos.
- **Segredos:** só no cofre do sistema (Credential Manager). Nunca em disco ou no git.
- **Sem telemetria.** Rede só para os serviços que o usuário configurou.
- **Nada enviado sozinho:** e-mail, mensagem, evento com convidados ou aprovação de permissão exigem clique explícito. Sem clique ou se a resposta falhar, o agente continua perguntando no próprio terminal.
- **IA propõe, usuário confirma:** tarefa ou data ambígua nunca é gravada sem confirmação.
- **Conteúdo do trabalho:** entra só a agenda, as tarefas e as ideias do dono. Dados de clientes, contas, valores ou documentos internos do trabalho não entram. O e-mail é só a conta pessoal.
- **Idioma:** na Fase L o Lumen é só em pt-BR (D-053.7): frases novas vão só em português, e as traduções que vieram do upstream ficam como estão. Nas demais fases (0, 1, 2, 2B, 2C e M), frases novas vão em `windows/src/i18n/extra.json` nas 13 línguas, com a verificação de i18n. O What's new das releases continua validado pelo CI nas línguas atuais. Docs e commits do fork em pt-BR.
- **Visual e contratos:** não redesenhar telas e pílulas existentes sem pedido explícito; não renomear IDs de pílula publicados.
- **Configuração de agentes:** nunca sobrescrever sem backup datado, diff visível e confirmação explícita. Preservar hooks de outras ferramentas.
- **Somente leitura é somente leitura:** usar dados locais quando o item disser local/offline; uma pílula de leitura não faz fetch, push, gravação nem altera arquivos.
- Nunca fazer transação nem guardar chave em texto puro.

## Como executar uma etapa

1. Uma subetapa por vez. Não começar a etapa dependente enquanto o checkpoint anterior não estiver fechado.
2. Antes de código: árvore limpa, `git fetch upstream` e `git merge upstream/main`. Resolver conflitos num commit próprio, sem misturar com a feature.
3. Spec e plano em `docs/superpowers/` antes de UX ou comportamento novo, com arquivos, interfaces, testes, aceite e rollback de migração quando houver.
4. A menor mudança que cumpre o aceite, com testes proporcionais à lógica alterada (TDD na Fase 2B).
5. Ao concluir: testes sintéticos, nova versão publicada no GitHub para o dono testar no uso real (autorização recorrente de 2026-11-12) e um commit por subetapa. Registrar versão, ambiente e resultados sem segredos, prompts ou caminhos pessoais. Checkpoints reais ficam abertos até o relato do dono.
6. Ao fechar uma etapa: mover o arquivo de `docs/roadmap/` para `docs/arquivadas/etapas/` (e as specs/planos dela para `docs/arquivadas/superpowers/`), atualizar a linha no ROADMAP-FORK e o STATE.
7. Legenda das caixas: `[ ]` pendente; `[x]` feito com a evidência exigida. Implementado sem validação manual: marcar o código como feito e deixar a caixa de runtime vazia. Etapa condicional só vira N/A com a evidência registrada. Esforço: P pequeno, M médio, G grande; M e G se dividem antes de implementar.

## Regras das fases M (Lumen assistente)

- **Ferramenta, não shell.** O modelo só chama ferramentas declaradas, cada uma com id, descrição, schema de entrada, nível de risco, simulação (dry-run), execução, verificação do resultado, timeout, cancelamento e resumo para o registro. Nenhuma ferramenta executa shell ou PowerShell montado a partir do texto do pedido.
- **Negado por padrão.** Ferramenta fora da lista habilitada não executa.
- **Risco e clique.** Baixo (reversível e local) executa direto. Médio (mudança persistente) pede clique na primeira vez, com "sempre permitir esta ferramenta". Alto (apagar, enviar dados, baixar arquivo, credenciais, energia, elevação) pede clique toda vez e nunca é lembrado. Sem clique ou em timeout, nada acontece.
- **Conteúdo externo é dado.** Texto de página, arquivo, nota ou mensagem nunca muda permissões nem vira instrução.
- **Ordem de preferência.** API nativa primeiro, acessibilidade (Agent Desktop) depois, visão só como último recurso.
- **Cérebro.** Assinaturas já conectadas por OAuth; onde o provedor não permitir, a CLI oficial dele. Sem chave de API paga obrigatória e sem modelo local.
- **Registro.** Só local e redigido: ferramenta, hora, risco, resultado. Sem conteúdo de tela, texto privado, token ou caminho pessoal.
- **Pausa rápida.** Um controle corta ferramentas, voz e pedidos do celular sem fechar o aplicativo.
- **Verificar depois de agir.** Toda ferramenta lê o estado depois da ação e relata falha como falha.

## Validação

| Mudança | Comando |
|---|---|
| TypeScript e testes Windows | em `windows/`: `npx tsc --noEmit` e `npm test` |
| UI e build | em `windows/`: `npm run build` |
| Rust do app | em `windows/src-tauri/`: `cargo test --lib` |
| Hook | em `windows/hook/`: `cargo test` |
| Relay | em `relay/` ou `android/relay/`: testes do `package.json` |
| What's new | na raiz: `python3 scripts/test-whats-new.py` |
| Teste manual | a lista de cenários do checkpoint; nunca substituir por typecheck ou teste unitário |

## Git e releases

- `origin` = `playertwo1/lumen`, o repositório próprio do Lumen desde a 0.6.18, é o único destino de push. `fork` = `playertwo1/coucou`, o fork antigo: os apps até a 0.6.17 procuram atualização lá, então ele só recebe a release ponte 0.6.18. `upstream` = `Louis-CFM/coucou` serve só para `fetch`/`merge`; nunca enviar para ele.
- Trabalhe numa branch (`codex/…`, `feat/…`, `docs/…`) ou numa worktree, e integre na `main` com merge depois dos testes. Apague a branch e a worktree depois do merge.
- Releases `windows-vX.Y.Z` seguem o [checklist de release](docs/roadmap/release-checklist.md) e entram no [registro de releases](docs/arquivadas/releases/releases-windows.md). Nenhuma release usa o nome ou os assets do Coucou depois da L1.
