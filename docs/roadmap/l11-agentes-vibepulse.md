> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../ROADMAP-FORK.md). Regras gerais: [AGENTS.md](../../AGENTS.md). Contrato: `esp32-s3-amoled-2-16/docs/roadmap/mqtt/PROTOCOLO.md`, seção "Agentes e cotas". Lado da placa: etapa P (`docs/roadmap/VIBEPULSE.md`) no mesmo repositório. Celular: F17-08 e F17-09 do `segundamente`.

# L11 — Agentes e cotas para a placa (ideias do VibePulse)

**Estado:** Em andamento (L11.1, L11.2 e L11.3 concluídas). Atualizado em 2026-11-28.

**Objetivo:** o Lumen faz no Windows o que o tokenserver do VibePulse faz no Mac:
- sabe quais agentes (Claude Code, Codex) estão trabalhando, com modelo e esforço;
- segura as perguntas e os pedidos de permissão deles até a placa ou o celular responderem, e injeta a resposta na mesma sessão;
- prevê quando a cota acaba e guarda o histórico de picos.

Entrega isso pela rede local (`/api/*`, porta 8787/8737, como na L9.6) e pelo MQTT (L10).

**Base:** [VibePulse](https://github.com/niclasvestlund-YT/vibepulse) v1.5.0 (MIT, © 2026 Niclas Vestlund). O lado PC dele é Python (`tools/tokenserver/`). Aqui ele é reescrito em Rust dentro de `esp32_bridge/`, com a origem e a licença MIT no cabeçalho de cada arquivo adaptado. Os vetores de teste dele (`test-vectors/`, `sim-fixtures/`) entram nos testes.

**Pré-requisitos:**
- L9.6: painel `/api/tokens` e `/api/agents`, `Esp32AgentPanelState` e hooks via `pipe.rs`.
- L10.1, só para a L11.5.

**Riscos registrados:**
- Responder a pergunta do Claude pelo `updatedInput` do hook `PreToolUse` funciona por experimento do VibePulse, mas não é documentado: reconferir a cada versão do Claude Code.
- No Codex, a pergunta depende de o modelo chamar a ferramenta MCP `ask`. Permissões vêm do hook `PermissionRequest`.
- O VibePulse nunca validou o NEEDS YOU numa placa física.

## L11.1 — Monitor de agentes

- [x] Ler os JSONL do Claude Code (`~/.claude/projects/**/*.jsonl`) e os rollouts do Codex (`~/.codex/sessions/**/rollout-*.jsonl`), juntando com o que o `pipe.rs` já recebe dos hooks (`esp32_bridge/agent_status.rs`, commit `90ddf601`). Estados como em `agent_status.py`:
  - **Claude:** `user` → trabalhando; `tool_use` → atividade (lendo, editando, buscando, rodando, testando, compilando, esperando); `end_turn` → esperando; `result` → feito ou erro.
  - **Codex:** `turn_context`/`task_started` → trabalhando; `function_call` → atividade; `task_complete` → feito ou erro.
  - **Modelo e esforço:** no Claude, `message.model` e `effort` no topo do registro; no Codex, `turn_context.payload`.
- [x] Prazos: `working` sem notícia há mais de 120 s vira `unknown`; `waiting`/`error` com mais de 2 h, também. No máximo 16 tarefas por provedor e 4 no snapshot; só o nome da pasta do projeto. *(commit `90ddf601`)*
- [x] `GET /api/agent-status` com o snapshot v2 do contrato (`seq` crescente, `updated_ms` como idade). *(commit `90ddf601`, `loopback_agent_status_v2_endpoint`)*
- [x] Checkpoint L11.1: testes com fixtures de JSONL (Claude e Codex) cobrindo estados, prazos, modelo e esforço. *(`cargo test --lib esp32_bridge::agent_status`: 9/9 + `loopback_agent_status_v2_endpoint`)*

## L11.2 — Segurar e responder perguntas e permissões

- [x] Rotas de hook só por loopback (recusar outro IP, `Host` e `Origin` estranhos): *(`esp32_bridge/server.rs`: `is_loopback_ip`, `is_valid_loopback_host`, `validate_hook_ingress` + `l11_2_loopback_hook_guards_and_end_to_end_interactions`)*
  - **Claude Code:** `PreToolUse` com matcher `AskUserQuestion` → `/api/hook/question`; `PermissionRequest` → `/api/hook/permission`. Hooks HTTP com prazo de 120 s.
  - **Codex:** hook `PermissionRequest` → `/api/codex/permission`, e ferramenta MCP `ask` → `/api/codex/question` (contrato portado em Rust sem alterar configs reais).
- [x] Guarda de pedidos como `interactions.py`: *(`esp32_bridge/interactions.rs`: `InteractionStore`)*
  - limites: até 8 pendentes, identificador aleatório de 16 bytes, textos cortados em 96/64/64 bytes;
  - prazo de 120 s; o cliente que caiu é detectado a cada 2 s;
  - só o pedido mais antigo vai no campo `pending` do `/api/agent-status` e no tópico `pc/agentes/pedido`.
- [x] Regras de `can_approve`, sem exceção: *(`can_approve_rules_for_claude_and_codex_and_truncation` + `safe_shell_command_whitelist_and_chaining_blocks`)*
  - **pergunta do Claude:** uma questão, sem múltipla escolha, e uma opção cujo rótulo termina em `(recommended)`. Sem marcador, não aprova (nunca inventar);
  - **pergunta do Codex:** 2 ou 3 opções e exatamente uma recomendada;
  - **permissão do Codex:** só a faixa segura (`ls`/`cat`/`head`/`tail`/`wc`/`grep`/`rg` sem opções; `git status`/`diff`/`log`/`show`/`branch`; `npm test` / `npm run test` / `npm run build`; `cargo test` / `cargo build`; `go test`; `pytest`; `python -m unittest`; `cmake --build`; `make`/`ninja` com alvos `all`/`build`/`test`/`check`; `idf.py build`), bloqueando `; & | > <` acento grave `$` e quebra de linha;
  - texto cortado bloqueia a aprovação.
- [x] Resposta na mesma sessão: *(`build_claude_question_hook_response`, `build_permission_hook_response`, `build_codex_question_response`, `HttpResponse::empty_ok`)*
  - **pergunta do Claude:** `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"questions":[...],"answers":{"<pergunta>":"<rótulo>"}}}}`;
  - **permissão:** `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"|"deny"}}}`;
  - **Codex `ask`:** `{"status":"answered","option_index":i,"answer":"..."}` ou `{"status":"computer"}`;
  - **sem decisão:** corpo vazio, e o terminal assume.
- [x] Decisões aceitas: *(`server.rs` + `mqtt.rs` + `interactions.rs`: `resolve_http`, `panic_http`, `resolve_mqtt`)*
  - **rede local:** `POST /api/interaction/<id>`, com HMAC v2 `v2|<provider>|<id>|<view_sha256>|<verdict>|<ts>` e a chave da sala;
  - **pânico:** `POST /api/panic`;
  - **MQTT:** `pc/agentes/decisao`.
  - Em todas: `ts` a ±90 s, `view_sha256` igual, uso único e a primeira resposta vence.
- [x] Substituir o `Esp32AgentPanelState` atual (aprovação de permissão da L9.6) por esta guarda, mantendo `/api/agents` enquanto a placa antiga existir. *(`esp32_bridge/mod.rs` + `server.rs` + `legacy_agents_snapshot`)*
- [x] Checkpoint L11.2: testes com os vetores do VibePulse (`test-vectors/`) para HMAC, `view_sha256`, faixa segura, regra de recomendação, prazo, cliente caído e "silêncio nunca aprova". *(`fixed_hmac_and_view_sha256_vectors_match_board_and_vibepulse`, `silence_never_approves_across_all_rejection_cases`, `queue_limits_8_pending_and_oldest_first`, `l11_2_loopback_hook_guards_and_end_to_end_interactions`)*

## L11.3 — Previsão de ritmo e semanal do modelo

- [x] Histórico de amostras como `usage_history.py` (`esp32_bridge/usage_history.rs`):
  - no máximo uma por provedor, janela e ciclo de reset a cada 15 min;
  - 8 dias de retenção, em `%LOCALAPPDATA%\Lumen\usage-history.json`, com escrita atômica.
- [x] Previsão por regressão linear nas amostras do ciclo nas últimas 24 h:
  - precisa de ≥ 3 amostras, 90 min e 1 ponto de variação; senão, `collecting`;
  - inclinação ≤ 0 → `unavailable`;
  - projeção ≥ 100 → `exhausts` (`...At` e `...OffsetMin`);
  - senão → `at_reset` (`...PctAtReset` e `...PaceFactor`).
- [x] Semanal do modelo mais pesado do Claude (`claudeModelWeekPct/ResetMin/Label`) a partir do OAuth usage que o Lumen já lê.
- [x] Campos novos em `/api/tokens` e no `put_usage` do relay (a placa antiga ignora campos que não conhece).
- [x] Checkpoint L11.3: testes portados de `test_usage_history.py`. *(`cargo test --lib esp32_bridge::usage_history` + `cargo test --lib`, `npx tsc --noEmit`, `npm test` 795/795, `npm run build`)*

## L11.4 — Max Tracker

- [ ] Histórico de 400 dias em `%LOCALAPPDATA%\Lumen\max-tracker.json` (schema v1 do VibePulse), alimentado pela cota (pico diário da janela de 5 h; semana marcada só com 100%) e pelo volume de tokens dos JSONL (com leitura retroativa limitada a 1 MiB por passo).
- [ ] Derivados: sequência de dias programando, semanas seguidas no máximo, semanas no máximo, pico médio de 20 semanas, dias no máximo e níveis de volume em tercis.
- [ ] `GET /api/max-tracker` v1, com 20 semanas × 7 dias por provedor, `[pct, nível]` e `-1` sem dado.
- [ ] Checkpoint L11.4: testes portados de `test_max_tracker.py`.

## L11.5 — Publicação pelo MQTT (era a L10.7)

- [ ] Publicar retidos e cifrados `pc/agentes/status` (a cada mudança, no máximo 1 por segundo), `pc/agentes/pedido` (vazio quando não houver), `pc/uso` (quando mudar) e `pc/max-tracker` (no máximo 1 a cada 5 min); assinar `pc/agentes/decisao`.
- [ ] Ao fechar o Lumen: limpar `pc/agentes/pedido` e marcar o status como vazio, para a placa não mostrar pedido fantasma.
- [ ] Checkpoint L11.5: teste com broker embutido (`rumqttd`): pedido publicado, decisão da placa e do celular, primeira vence e pedido limpo.

## L11.6 — Instalar os hooks com consentimento

- [ ] Tela em Integrações → Placa: "Responder agentes pela placa", desligado por padrão. Ao ligar, mostra exatamente o que será escrito e:
  - grava os hooks HTTP no `~/.claude/settings.json` (com backup e remoção limpa ao desligar);
  - instala o plugin/hook do Codex.
- [ ] Desligado: nenhum hook segura nada, e os agentes perguntam no terminal como hoje.
- [ ] Checkpoint L11.6: teste de instalação e remoção num diretório de usuário temporário, sem tocar no real.

## L11.7 — Validação real

- [ ] Com a placa (etapa P) e o celular (F17-08), seguir o roteiro da P6 da placa: pergunta do Claude aprovada, permissão negada, `cargo test` do Codex aprovado, `rm -rf` só no PC, prazo vencido sem aprovar, PC desligado no meio, primeira resposta vence.
- [ ] Release pelo [checklist](release-checklist.md), com o pedido do dono.

## Aceite (Checkpoint L11)

- Placa e celular mostram agentes, alertas, previsão e histórico vindos do Lumen, em casa e fora.
- Nenhuma aprovação sem toque explícito; nenhum texto de pergunta ou comando em claro no broker.
- `cargo test --lib`, `npm test`, `npx tsc --noEmit` e `npm run build` verdes; validação real registrada pelo dono.
