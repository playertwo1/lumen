> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../ROADMAP-FORK.md). Regras gerais: [AGENTS.md](../../AGENTS.md). Contrato: `esp32-s3-amoled-2-16/docs/roadmap/mqtt/PROTOCOLO.md` (fonte da verdade). Lado da placa: `firmware/main/mqtt_link.c` e `crypto_box.c` no mesmo repositório. Etapa irmã no celular: F17 do `segundamente`.

# L10 — Placa por MQTT

**Estado:** Não iniciada. A placa já fala MQTT (Q1 do `esp32-s3-amoled-2-16`, gravada e com autoteste em 2026-11-27). Atualizado em 2026-11-27.

**Objetivo:** o Lumen fala com a placa ESP32-S3 AMOLED e com o Segunda Mente por um broker MQTT na nuvem, ao mesmo tempo, dentro e fora de casa, com o conteúdo cifrado. Substitui o pareamento de 6 dígitos (L9.1), o `/v1/tasks/sync` (L9.3), a sala `/v1/bridge` do relay (L9.4) e o `/v1/media` por HTTP (L9.5).

**Fica como está:**
- a voz pela rede local em `/v1/voice` (L9.2), só com outro bearer;
- o painel `/api/tokens` e `/api/agents` e o `put_usage` no relay (L9.6), até a L11.

**Pré-requisitos:** L9 implementada. Broker criado pelo dono (Q0 do `esp32-s3-amoled-2-16`) só para a validação real; o código e os testes não dependem dele.

**Princípios:**
- **Broker:** nunca vê conteúdo. Tudo fora de `presenca/` vai em AES-256-GCM com AAD = `from`.
- **Segredos:** URI, usuário, senha, sala e chave ficam em `secrets.rs` e nunca chegam ao webview, a logs ou a commits.
- **Fallback:** se o Lumen fechar, a placa e o celular continuam se falando pelo broker.

## L10.1 — Cliente MQTT e presença

- [ ] Adicionar `rumqttc` (tokio + rustls + `webpki-roots`) e o módulo `esp32_bridge/mqtt.rs`.
  - MQTT 3.1.1 com TLS, client_id `lumen-<sala>`, keepalive 60 s.
  - LWT `sm/<sala>/presenca/lumen` = `0`, retido; publicar `1` retido ao conectar.
  - Reconexão automática com espera crescente.
- [ ] Ligar e desligar junto com `apply_runtime_config` (mesmo ciclo de vida da ponte atual) e expor o estado em `esp32_status`: conectado, host, sala e placa online (assinando `presenca/placa`).
- [ ] Checkpoint L10.1: teste com broker embutido (`rumqttd` em `dev-dependencies`) cobrindo conectar, LWT e reconexão.

## L10.2 — Pareamento pelo código da placa

- [ ] Campo "Código da placa" em Configurações → Integrações → Placa ESP32-S3, aceitando o JSON do QR da placa: `{"t":"sm-mqtt","v":1,"uri","u","p","sala","k"}`.
  - Validar `t`, `v`, a sala (12 `[a-z0-9]`) e a chave (64 hex).
  - Guardar em `secrets.rs`.
- [ ] Mostrar só host, sala e estado. Botão "Esquecer placa" apaga os secrets.
- [ ] Revogação na placa (chave nova): o Lumen passa a falhar ao abrir os envelopes e mostra "Chave da placa mudou — cole o código novo".
- [ ] Checkpoint L10.2: testes de validação do código (inválido, versão errada, sala e chave malformadas) e `esp32-settings.test.mjs` atualizado.

## L10.3 — Tarefas pelo MQTT

- [ ] Envelope compatível com `crypto_box.c`.
  - Chave AES = SHA-256 do texto da chave da sala; AAD = `from`; IV aleatório.
  - Ignorar o próprio `from`; rejeitar envelope sem `from`.
  - Reaproveitar `crypto.rs`.
- [ ] Publicar um tópico retido por tarefa, `sm/<sala>/tarefas/<entity_id>`, com `SyncEnvelope` v1 (`updated_at` no topo, `origin_role` `lumen`), a partir de `tasks/esp32_sync.rs` e do `TaskStore`.
  - `updated_at` muda só quando o conteúdo muda.
  - Payload vazio retido quando a tarefa é apagada.
- [ ] Assinar `tarefas/+`. Aplicar a regra do maior `updated_at` (empate mantém o local) e apagar ao receber payload vazio.
- [ ] Ao reconectar, depois da janela de cerca de 3 s:
  - Apagar localmente a tarefa já publicada ou recebida que não teve retida.
  - **Trava:** não apagar nada se nenhuma retida chegou, ou se sumiria mais da metade e mais de 3 tarefas; nesse caso, republicar.
  - Só rodar a limpeza com o SUBACK OK e a conexão estável a janela toda.
- [ ] Decidir se o Lumen continua repassando as tarefas da placa à outbox da L6 (o celular agora as recebe direto pelo MQTT, com o mesmo `entity_id`) ou para de repassar, evitando dois caminhos para a mesma tarefa. Registrar a decisão.
- [ ] Checkpoint L10.3: testes com broker embutido cobrindo:
  - ida e volta cifrada, chave errada e `from` adulterado;
  - conflito velho, empate e novo;
  - apagar nos dois sentidos;
  - limpeza com e sem a trava.

## L10.4 — Mídia e voz

- [ ] Assinar `sm/<sala>/pc/midia` (`{"action":"play_pause"|"next"|"prev"|"vol_up"|"vol_down"}`) e chamar `esp32_media_control`.
- [ ] Publicar `sm/<sala>/pc/midia/estado` retido (`{"playing","title","artist"}`) quando o estado mudar.
- [ ] Voz pela rede local: `/v1/voice` passa a aceitar o bearer `hex(HMAC-SHA256(texto da chave da sala, "voice"))`, igual ao `sm_hmac_hex` da placa. Manter as chaves pareadas antigas enquanto existirem.
- [ ] Checkpoint L10.4: teste de mídia com broker embutido e vetor fixo do HMAC de voz.

## L10.5 — Remover o caminho antigo

- [ ] `server.rs`: tirar `/v1/pair`, `/v1/tasks/sync` e `/v1/media`.
- [ ] `relay.rs` e `run_relay_tick`: tirar `pull_bridge`, `post_bridge` e a parte de bridge. **Manter `put_usage`.**
- [ ] Tirar o código de 6 dígitos (arm, TTL, `pair_with_code`), os campos de sala do relay em `settings/esp32.ts` e os secrets `esp32-relay-room` e `esp32-bridge-keys`, este depois de migrar o bearer de voz.
- [ ] Migração na primeira abertura da versão nova: apagar os secrets antigos só depois de o código MQTT estar salvo.
- [ ] Checkpoint L10.5: `cargo test --lib`, `npx tsc --noEmit`, `npm test` e `npm run build` verdes; as rotas removidas respondem 404.

## L10.6 — Ponta a ponta com a placa e o celular

- [ ] Com o broker real: colar o código da placa, ver "placa online" e criar, concluir e apagar 10 tarefas alternando Lumen, placa e celular. Tudo converge em menos de 5 s.
- [ ] Celular desligado durante 3 alterações; ao religar, ele recebe tudo sem duplicar.
- [ ] "Próxima faixa" falada na placa troca a música no PC, com o PC fora da rede da placa (hotspot).
- [ ] Conferência de logs e do broker (MQTTX): nenhum texto claro de tarefa e nenhum segredo.
- [ ] Release seguindo o [checklist](release-checklist.md), com o pedido do dono.

## L10.7 — Uso e agentes por MQTT

Movida para a [L11 — Agentes e cotas para a placa](l11-agentes-vibepulse.md) (L11.5), que amplia o escopo com monitor de agentes, NEEDS YOU com resposta, previsão e histórico.

## Aceite (Checkpoint L10)

- Tarefas, mídia e presença funcionando com placa, Lumen e celular ao mesmo tempo, em redes diferentes, sem duplicar.
- Nenhuma rota de pareamento por código nem sala de bridge no Lumen.
- `cargo test --lib`, `npm test`, `npx tsc --noEmit` e `npm run build` verdes; validação real registrada pelo dono.
