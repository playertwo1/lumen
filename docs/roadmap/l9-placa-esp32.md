> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../ROADMAP-FORK.md). Regras gerais: [AGENTS.md](../../AGENTS.md). Referência do protocolo já implementado na placa: `esp32-s3-amoled-2-16` (`firmware/main/hub.c`, `firmware/main/extras.c`, `tools/lumen_bridge/README.md` e `docs/roadmap/assistente-de-voz/V6-PONTE-LUMEN.md`).

# L9 — Ponte nativa com a placa ESP32-S3 AMOLED

**Objetivo:** permitir que o Lumen converse nativamente com a placa de mesa `esp32-s3-amoled-2-16` (na mesma rede local ou via relay Cloudflare), substituindo os scripts Python auxiliares (`lumen_bridge` e, opcionalmente, `tokenserver`) para que a placa use a voz e a transcrição do PC, sincronize tarefas sem duplicar e controle mídia/agentes.

**Pré-requisitos:** L4 (Tarefas), L6 (Sync com a Segunda Mente) e M2 (Controle nativo de mídia/volume do Windows). Pode ser desenvolvida em paralelo às fases M sem alterar o protocolo já existente com o app Android.

**Princípios de segurança e isolamento:**
- Nenhum token do ChatGPT ou do celular trafega entre a placa e o Lumen nem aparece em log.
- A placa tem chave própria de 32 bytes (hex), guardada no cofre do Windows e revogável no Lumen.
- Se o Lumen fechar ou o PC desligar, a placa volta a operar sozinha sem travar.

## L9.1 — Servidor HTTP LAN, pareamento de 6 dígitos e revogação

- [x] Decidir se a ponte da placa entra como rotas no servidor local já existente (`windows/src-tauri/src/phone_link/server.rs`) ou em listener HTTP LAN dedicado (padrão `8787`, configurável), mantendo compatibilidade com o cliente da placa (`hub.c`). *(Listener HTTP/1.1 dedicado em `esp32_bridge/server.rs` na porta `8787`, filtro `is_local`.)*
- [x] Implementar os endpoints públicos:
  - `GET /v1/ping` → `200 {"ok":true,"service":"lumen-bridge"}`.
  - `POST /v1/pair` com `{"code":"123456","device_id":"esp32-s3"}` → valida o código de 6 dígitos em tempo constante, invalida o código após uso, gera chave hex de 32 bytes, persiste no cofre do Windows e retorna `200 {"ok":true,"key":"<hex>"}` (ou `401` se inválido).
- [x] Exigir `Authorization: Bearer <key>` validado em tempo constante em todos os endpoints autenticados.
- [x] Adicionar UI no Lumen para digitar/armar o código de 6 dígitos exibido na placa, ver estado da conexão e revogar a chave da placa na hora (`401` imediato).
- [ ] Checkpoint L9.1: código e loopback HTTP validados (`secrets::tests`, `settings::tests`, `esp32_bridge::tests`, `esp32-settings.test.mjs`). **Real na placa: NOT_RUN** (aguarda teste físico do dono).

## L9.2 — Ponte de voz (STT local, resposta e TTS WAV para a placa)

- [x] Implementar `POST /v1/voice` autenticado aceitando `audio/wav` binário (gravado pelo microfone da placa) ou `application/json` (`{"text":"..."}`).
- [x] Transcrever o áudio recebido usando o pipeline local do Lumen (`speech/mod.rs`), gerar a resposta pelo assistente (`voice/mod.rs`) e sintetizar a fala em WAV PCM16 mono 22050 Hz (respeitando o teto de buffer HTTP da placa: `96 KiB` na resposta JSON).
- [x] Retornar `200 {"ok":true,"heard":"...","answer":"...","sample_rate":22050,"wav_b64":"..."}`.
- [ ] Checkpoint L9.2: código e loopback validados (`voice_mock_json_returns_22050_wav_under_96kib`, `voice_rejects_bad_content_type_and_bad_wav`, `loopback_ping_and_voice_auth_contract`, `< 96 KiB`, `22050 Hz` 44 B WAV). **Real na placa: NOT_RUN** (aguarda teste físico do dono).

## L9.3 — Sincronização bidirecional de tarefas sem duplicação

- [x] Implementar `POST /v1/tasks/sync` autenticado recebendo `{"envelopes":[...]}` no formato `SyncEnvelope` v1 (`entity_id`, `updated_at`, `payload: {id, text, done, due_ts}`) e retornando `200 {"ok":true,"envelopes":[...]}`.
- [x] Fazer upsert idempotente por `entity_id` na lista de Tarefas do Lumen (L4), respeitando o `updated_at` mais recente.
- [x] Encaminhar tarefas recebidas da placa para a outbox da L6 para que cheguem ao Segunda Mente no celular uma única vez.
- [ ] Checkpoint L9.3: código e testes unitários validados (`tasks::esp32_sync::tests`: `<= 3800 B`, guarda de idempotência contra `time(NULL)`, LWW, outbox L6). **Real na placa: NOT_RUN** (aguarda teste físico do dono).

## L9.4 — Ponte remota via Relay Cloudflare criptografado (redes diferentes)

- [x] Integrar o cliente do Lumen à sala `/v1/bridge/<room>` do Worker (`Authorization: Bearer <RELAY_KEY>`), usando `GET` para drenar pendências e `POST` para enviar (teto `64 KiB` por mensagem, TTL `3600 s`).
- [x] Cifrar e decifrar os envelopes ponta a ponta com **AES-256-GCM**, chave `SHA-256(bridge_key)`, no formato JSON esperado pela placa: `{"iv":"<b64 12B>","ct":"<b64>","tag":"<b64 16B>"}`.
- [x] Sincronizar tarefas pendentes pela sala quando PC e placa não estiverem na mesma LAN (e registrar limite prático caso voz trafegue pelo relay).
- [ ] Checkpoint L9.4: código e testes mock HTTP validados (`esp32_bridge::crypto::tests`, `esp32_bridge::relay::tests::test_relay_client_flow`, worker `run_relay_tick`). **Real na placa: NOT_RUN** (aguarda teste físico do dono).

## L9.5 — Controle de mídia do Windows pela placa

- [x] Implementar `POST /v1/media` autenticado aceitando `{"action":"play_pause"|"next"|"prev"|"vol_up"|"vol_down"}`.
- [x] Ligar às APIs nativas de mídia/volume da M2 (sem PowerShell montado por texto) e retornar `200 {"ok":true,"action":"...","playing":<bool>}`.
- [ ] Checkpoint L9.5: código e testes loopback/unitários validados (`mochi::tools::audio::tests::step_volume_with_clamps_and_steps`, `mochi::tools::media::tests::esp32_media_with_toggles_play_pause`, `loopback_media_dry_run_and_paused_guard`). **Real na placa: NOT_RUN** (aguarda teste físico do dono).

## L9.6 — Painel de cota e aprovação de agentes integrado (opcional, substitui `tokenserver`)

- [x] Avaliar expor diretamente no Lumen os endpoints consumidos pelo painel da placa (`hub.c`):
  - `GET /api/tokens` (cota Claude/Codex 5h e semana, `...Stale` e `updatedAt`) + publicação opcional no relay `PUT /v1/usage` (apenas números e booleanos, sem tokens).
  - `GET /api/agents` (`yourTurn`, `turnSeq`, `agent`, `project`, `pendingApproval`) e `POST /api/agents/decide` (`{id, decision: "allow"|"deny"}`, uso único com expiração de `115 s`, sem aprovação automática em timeout).
- [x] Reaproveitar os hooks de agentes que o Lumen já opera no Windows, eliminando a necessidade de manter processos Python separados rodando em segundo plano.
- [ ] Checkpoint L9.6: código e testes loopback validados (`esp32_bridge::tokens_agents::tests`, `loopback_api_panel_gates_tokens_agents_and_replay_409`, hooks em `pipe.rs` e `lib.rs`). **Real na placa: NOT_RUN** (aguarda teste físico do dono).

## Aceite (Checkpoint L9)

- Evidência sintética na `main` (2026-11-27): `npx tsc --noEmit`, `npm test` (`791/791`), `npm run build`, `python scripts/test-whats-new.py`, `cargo test --lib --locked` (`992/992`, 10 ignored, incluindo `22/22` em `esp32_bridge` e `3/3` em `tasks::esp32_sync`) e `cargo check --bins --locked`.
- [ ] Mesma rede: "anota comprar leite" na placa responde em voz no alto-falante da placa e a tarefa entra uma única vez no Lumen (e segue para o celular via L6). *(Real na placa: NOT_RUN)*
- [ ] Redes diferentes: tarefas sincronizam via `/v1/bridge/<room>` cifrado com AES-256-GCM. *(Real na placa: NOT_RUN)*
- [ ] Controle de mídia (`play_pause`, `next`, `prev`, `vol_up`, `vol_down`) funciona pela placa. *(Real na placa: NOT_RUN)*
- [ ] Revogar a placa no Lumen bloqueia chamadas imediatamente (`401`); fechar o Lumen deixa a placa continuar funcionando sozinha. *(Real na placa: NOT_RUN)*
- [ ] Conferência de logs: nenhum token, segredo ou chave da placa aparece em disco ou console. *(Código conferido na auditoria; real na placa: NOT_RUN)*
