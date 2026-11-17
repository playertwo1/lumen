> Etapa **concluída e arquivada** da Fase 0 (pausada). Índice: [ROADMAP-FORK.md](../../ROADMAP-FORK.md).

## 0.4 — Hook do Claude Code e Windows Terminal

- [x] Conferir se o hook está instalado; antes de instalar/atualizar, criar backup datado, mostrar o diff e obter confirmação explícita.
- [x] Executar uma ação que gere pedido de permissão; confirmar que a ilha mostra Permitir/Negar.
- [x] Clicar Negar e confirmar que o agente recebe a recusa; repetir e clicar Permitir.
- [x] Repetir sem clicar: o Coucou não pode aprovar sozinho; o pedido deve continuar no terminal.
- [x] Usar “Abrir terminal” e confirmar que a janela da sessão correta vem à frente.
- Checkpoint: confirmar cada uma das três decisões (permitir, negar, sem clique) e o terminal-alvo. Não marcar aprovado se só a tela do cartão foi vista.
- Status (2026-11-09): APROVADO — hooks instalados após aprovação explícita (12 handlers, backup datado). Testes na pasta temporária confiada, com `claude --permission-mode default` no Windows Terminal (o `defaultMode: auto` do usuário e comandos só de leitura como `echo` não geram `PermissionRequest`). Cada rodada pediu a ferramenta Write para criar um arquivo de texto na pasta. Negar: pedido 34084-1, clique Negar, hook respondeu deny, arquivo não criado. Permitir: pedido 34084-2, clique Permitir, hook respondeu allow, `coucou-allow.txt` criado. Sem clique: pedido 34084-17 sem decisão na ilha; o log registrou `timed out — terminal takes over` após ~108 s e o arquivo não foi criado; a pergunta passou ao terminal. Abrir terminal: no cartão de sessão encerrada, trouxe à frente a aba da sessão correta (`coucou-noclick2`) entre três abas na mesma pasta. Observação: um clique destinado a um cartão do Antigravity aprovou o cartão seguinte do Claude Code ~1 s depois (fora da proteção de 600 ms); rodada descartada e repetida. Validado só com `--permission-mode default`, não no modo `auto`.
