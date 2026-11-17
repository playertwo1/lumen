> Etapas **descartadas** pelo proprietário em 2026-11-11 e arquivadas. Índice: [ROADMAP-FORK.md](../../ROADMAP-FORK.md).

## 1.3 — Portfólio de cripto, somente leitura (G)

**Descartado (2026-11-11), decisão do proprietário:** não vai usar. Nada foi implementado.

- [ ] Obter decisão do proprietário sobre exchanges/carteiras suportadas e dados a exibir (saldo, ativos, moeda de conversão).
- [ ] Definir permissões somente leitura, armazenamento de credenciais no serviço seguro existente, atualização e dados locais no mini-spec.
- [ ] Não implementar até as fontes e os campos serem escolhidos; chaves de saque, transações e trading ficam fora.
- [ ] Com decisão aprovada, criar fixtures sem credenciais e testes para saldo vazio, resposta incompleta, limite de API e erro de rede.
- [ ] Implementar uma fonte por subfase; conferir total/moeda sem arredondamento enganoso.
- Checkpoint: credencial nunca aparece em log, nenhum endpoint de escrita é chamado e os valores batem com a fonte.

## 1.4 — Cal.com em calendário em vez de lista (P)

**Descartado (2026-11-11), decisão do proprietário:** não usa Cal.com. A visão do upstream (#1006) continua no app, sem alteração; o teste manual não será feito. O registro abaixo fica como referência.

- [x] Ler `NotchBuddy/Sources/App/CalcomPoller.swift` e o card/lista atual no Windows.
- [x] Definir janela temporal, navegação, fuso horário, eventos simultâneos e estados vazio/erro — já definidos pelo upstream (#1006), sem mini-spec própria do fork.
- [x] Aprovar o desenho antes de alterar uma tela existente — N/A: a visão já está na `main`; nada foi alterado.
- [ ] Testar paginação, mudança de dia/fuso, ausência de eventos e erro de autenticação — automatizados, exceto o erro de autenticação (ver status).
- [x] Implementar a visualização preservando dados e comandos atuais de abertura do evento.
- Checkpoint: datas/horas correspondem ao fuso escolhido e não há evento duplicado ou perdido nos limites da página.

**Status 1.4 (2026-11-11): parcial, já implementado pelo upstream, teste manual pendente —** a descoberta mostrou que o calendário já existe no Windows: PR #1006 (@lazarevtill, commits `0c36e64` e `3b8752d`), visão mês → dia → reserva no card existente, com o botão "Abrir Cal.com" mantido. O que cada parte cobre:
- `calcom.rs`: janela de 60 dias com paginação por cursor (até 10 páginas de 100). Um id repetido ou um cursor repetido viram erro, sem duplicar a reserva. Resposta limitada a 2 MB e timeout.
- `core/calendar.ts`: dado inválido vira erro, diferente de calendário vazio. Os dias usam o fuso local.
- `views/calendar.ts`: não navega para fora do intervalo buscado.
- Teste novo: uma reserva às 23:30 locais fica no dia local, mesmo caindo no dia seguinte em UTC. Rodado em processo com quatro fusos (UTC+14, UTC-7, UTC+0 e o da máquina, UTC-3; o deslocamento foi conferido em cada execução): 5/5 em cada. `npm test`: 588 aprovados.
- Não coberto por teste: o erro de autenticação (401 mostra "API error 401" pelo caminho genérico; `fetch` usa URL fixa e não tem teste HTTP) e uma reserva que atravessa a meia-noite (aparece só no dia de início).
- Teste manual com chave Cal.com real: pendente, depende do proprietário.
