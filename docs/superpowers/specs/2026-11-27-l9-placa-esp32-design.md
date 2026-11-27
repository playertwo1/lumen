# Spec — L9: Ponte nativa do Lumen com a placa ESP32-S3 AMOLED

**Data:** 2026-11-27 · **Etapa:** [L9](../roadmap/l9-placa-esp32.md) · **Referência do firmware:** `esp32-s3-amoled-2-16` (`firmware/main/hub.c`, `firmware/main/extras.c`, `tools/lumen_bridge/README.md`, `tools/tokenserver/README.md`)

## 1. Objetivo

Embutir no binário nativo do Lumen (`windows/src-tauri/src/esp32_bridge/`) a ponte HTTP LAN, o sync bidirecional de tarefas, o cliente de relay Cloudflare criptografado (AES-256-GCM), o controle de mídia/volume nativo e o painel opcional de cotas/aprovação de agentes consumidos pela placa `esp32-s3-amoled-2-16`, eliminando a necessidade de rodar `lumen_bridge/bridge.py` e `tokenserver/tokenserver.py` em separado.

## 2. Arquitetura e Módulos

- `windows/src-tauri/src/esp32_bridge/mod.rs`: estado compartilhado (`Esp32Bridge`), ciclo de vida (início/parada com o app e ao salvar ajustes), loop de relay em segundo plano (20 s para `/v1/bridge/<room>` e 300 s para `PUT /v1/usage`), comandos Tauri (`esp32_*`).
- `windows/src-tauri/src/esp32_bridge/server.rs`: servidor HTTP/1.1 assíncrono em `tokio::net::TcpListener` (`0.0.0.0:<porta>` filtrado por `is_local(peer.ip())`, padrão `8787`, e listener companheiro opcional na porta `8737` compartilhando o mesmo roteador/estado quando o painel de agentes está ativo para compatibilidade com a URL padrão do `tokenserver`).
- `windows/src-tauri/src/esp32_bridge/crypto.rs`: derivação `SHA-256(bridge_key_ascii)` (`ring::digest::SHA256`), cifra/decifra `AES-256-GCM` (`ring::aead::AES_256_GCM`, IV 12 B, tag 16 B, JSON `{"iv","ct","tag"}` em Base64 padrão), validação de sala (`^[A-Za-z0-9_-]{8,64}$`) e geração de chave de 32 bytes (64 hex).
- `windows/src-tauri/src/esp32_bridge/relay.rs`: cliente HTTP (`reqwest` + `rustls-tls`, timeout 8 s) para `GET/POST /v1/bridge/<room>` (`<= 64 KiB`, TTL `3600 s`) e `PUT /v1/usage` (`<= 512 B`).
- `windows/src-tauri/src/esp32_bridge/audio_wav.rs`: decodificação de `audio/wav` (RIFF/WAVE PCM16 1–2 canais, `8000..=48000 Hz`, reamostragem para `16000 Hz` via `speech::audio::Resampler`) e síntese TTS em memória (`windows::Media::SpeechSynthesis::SpeechSynthesizer` com fallback WAV PCM16 mono `22050 Hz` canônico de 44 bytes, teto `MAX_WAV_BYTES = 64_800` B com fade-out de 20 ms para manter a resposta JSON `< 96 KiB`).
- `windows/src-tauri/src/esp32_bridge/tokens_agents.rs`: snapshot de cotas (`TokensSnapshot`, cache Claude via `StatusLine` em `pipe.rs` e Codex via JSONL em `~/.codex/sessions` com TTL 60 s), payload sanitizado `build_clean_usage_payload` (`<= 512 B`) e máquina de estados `Esp32AgentPanelState` (aprovação de uso único com TTL `115 s`, `id` aleatório `esp-app-<32-hex>`, detecção de comandos perigosos e sanitização de `project` para apenas o nome da pasta folha).
- `windows/src/settings/esp32.ts`: seção `esp32Section` em `Notch → Integrações` (pareamento de 6 dígitos, estado da ponte, revogação imediata, configuração do relay e switch do painel de cotas/agentes).

## 3. Contratos Congelados (Fidelidade ao `hub.c` e `extras.c`)

1. **Públicos (LAN):**
   - `GET /v1/ping` → `200 {"ok":true,"service":"lumen-bridge"}`.
   - `POST /v1/pair` com `{"code":"123456","device_id":"esp32-s3"}` → valida código de 6 dígitos em tempo constante (`subtle::ConstantTimeEq`, TTL 300 s, uso único), gera chave hex de 64 chars (32 B), salva em `esp32-bridge-keys` no Credential Manager e retorna `200 {"ok":true,"key":"<hex>"}` (ou `401`).
2. **Autenticados (`Authorization: Bearer <key>` em tempo constante sem curto-circuito):**
   - `POST /v1/voice`: aceita `audio/wav` (`<= 4 MiB`) ou `application/json` (`{"text":"...","mock":bool}`). Respeita `PAUSED` (`503`). Retorna `200 {"ok":true,"heard":"...","answer":"...","sample_rate":22050,"wav_b64":"..."}` com tamanho total do JSON `< 96 KiB` (`98_304 B`) e header WAV canônico de 44 bytes.
   - `GET /v1/agenda` continua servido pelo Cloudflare Worker (`relay/`), não pelo servidor LAN do Lumen.
- `POST /v1/tasks/sync`: recebe `{"envelopes":[...]}` no formato `SyncEnvelope` v1 (`contract_version`, `entity_id`, `updated_at`, `payload: {id, text, done, due_ts}`), faz upsert idempotente na lista L4 (sem incrementar `version` nem re-enfileirar na L6 se o conteúdo não mudou; caso novo/alterado, enfileira uma vez na outbox L6) e retorna `200 {"ok":true,"envelopes":[...],"count":N}` limitado a `<= 3_800` bytes (teto `HTTP_SMALL_MAX = 4096` do `hub.c`).
   - `POST /v1/media`: aceita `{"action":"play_pause"|"next"|"prev"|"vol_up"|"vol_down","dry_run":bool}`. Respeita `PAUSED` (`503`). Usa APIs nativas da M2 (GSMTC e `IAudioEndpointVolume`, passo ±10% para volume, sem PowerShell) e retorna `200 {"ok":true,"action":"...","playing":<bool>}`.
3. **Relay Cloudflare (`/v1/bridge/<room>` e `/v1/usage`):**
   - Chave AES: `SHA-256` sobre os bytes ASCII da `bridge_key` (64 chars hex), idêntico ao `hub.c:504`.
   - `GET /v1/bridge/<room>` drena a fila; envelopes `"pc"` ainda não sincronizados drenados junto são re-coalescidos em uma única mensagem criptografada para não perder envio para a placa.
4. **Painel de Cota e Agentes (L9.6, `/api/*`):**
   - `hub.c` não envia `Authorization` em `/api/tokens`, `/api/agents` e `/api/agents/decide`. Portanto, esses endpoints ficam ativos **somente** na LAN quando `esp32_agent_panel_enabled == true` **e** há pelo menos uma chave pareada em `esp32-bridge-keys` (se não houver chave pareada ou após revogação, retornam `401`). Se `Authorization` for enviado, é validado em tempo constante.
   - `POST /api/agents/decide` (`{id, decision: "allow"|"deny"}`) é uso único com TTL `115 s` (`409` se expirado/desconhecido/repetido) e resolve `crate::resolve_approval` no `pipe.rs`. Timeout nunca aprova.

## 4. Segurança, Privacidade e Rollback

- Segredos (`esp32-bridge-keys`, `esp32-relay-key`, `esp32-relay-room`) vivem exclusivamente em `secrets.rs` (`ESP32_INTERNAL_KEYS`), inacessíveis à webview.
- Conexões não-locais (`!is_local(peer.ip())`) são descartadas antes da leitura HTTP.
- Logs nunca imprimem chaves, tokens, códigos de pareamento, conteúdo de tarefas ou caminhos completos.
- **Rollback:** `Settings` usa `#[serde(rename_all = "camelCase", default)]`; reverter os commits da L9 restaura o estado anterior sem migração destrutiva de dados (`todo.json`, `sync.json`, `settings.json`).
