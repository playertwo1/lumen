# L8.6 — WhatsApp pela ponte do Orbit

## Problema
O Lumen mostra WhatsApp na ilha só por GOWA ou OpenWA, que precisam de um aparelho vinculado próprio. O Orbit já tem uma ponte WhatsApp local (`whatsapp-pj-mcp`, Go + whatsmeow, fora deste repositório) com um aparelho vinculado. Reaproveitar a ponte evita um segundo aparelho e um download.

## Contrato encontrado no código da ponte (só leitura, nada chamado)
- **Endereço:** `http://127.0.0.1:8080` (`WHATSAPP_BRIDGE_PORT`, padrão 8080). Só loopback; a ponte recusa outro host.
- **Autenticação:** nenhuma. Não há token nem header. A proteção é o `origin_guard`: recusa `Host` que não seja loopback e recusa `Origin` de navegador que não esteja em `WHATSAPP_ALLOWED_ORIGINS`. Cliente sem `Origin` (o Lumen, a partir do Rust) passa.
- **Clientes:** servidor HTTP comum, sem limite de clientes no código. Orbit e Lumen podem ler ao mesmo tempo.
- `GET /api/status` → `{connected, authenticated, auth_state, device_jid?, disconnected_seconds?, last_client_error?, metrics…}`.
- `GET /api/chats?limit=&offset=` → `{chats:[{jid, chat_type, name, last_message_time (s), last_message_preview, unread_count}], count}`.
- `GET /api/messages?chat_jid=&limit=&since=` → `{messages:[{id, chat_jid, sender_jid, sender_display, timestamp (s), type, content_text, is_from_me, attachment?{mime, filename, duration_seconds, …}}], count, chat?}`. `since` é segundo Unix e inclusivo; `limit` vai até 500.
- Tipos: `text`, `image`, `video`, `voice`, `audio`, `document`, `sticker`, `location`, `contact`, `reaction`, `system`.
- `POST /api/sends` cria **rascunho** que um humano aprova na Approval UI. O Lumen não usa.

## Diferenças para GOWA e OpenWA
- Sem token: GOWA usa basic auth e OpenWA, `X-API-Key`. A ponte não pede nada, então não há segredo para guardar.
- Sem webhook: o Lumen só lê, de 20 s em 20 s, como o fallback atual.
- Sem QR no Lumen: a vinculação acontece no Orbit.
- Timestamps em segundos (GOWA e OpenWA variam). O conteúdo já vem com `scrubbed_text`.

## Decisão
- Novo `Kind::Orbit` (`prefs.gateway = "orbit"`, padrão `http://127.0.0.1:8080`) ao lado de GOWA e OpenWA. O provedor reaproveita o `Http` existente, sem cabeçalho de auth.
- `status` lê `/api/status`. Conectado e autenticado vira `Connected`; autenticado e fora do ar vira `Connecting`; sem autenticação vira `NeedsQr`, sem QR.
- `recent_messages` lê `/api/chats?limit=20`, depois `/api/messages?chat_jid=…&limit=10&since=…` nos chats que mudaram (até 8), como o GOWA.
- `messages` (cartão aberto) lê `/api/messages?chat_jid=…&limit=10`.
- **Sem envio:** `send` devolve `Refused` sem tocar na rede. `mark_read` é no-op, porque a ponte não tem essa rota e o Lumen nunca escreve no WhatsApp. `register_webhook` não é chamado.
- **Sem listener:** com `orbit` o Lumen não abre a porta do webhook. O estado do webhook fica `Off`.
- Mensagens continuam só em memória. Texto é dado não confiável: nunca vira instrução.
- **Adiado:** "Abrir no Orbit para responder". Nenhuma rota de leitura informa a URL da Approval UI (porta 8081 por padrão, login próprio com segredo em arquivo). Sem URL segura, o botão fica para depois.

## Arquivos
- `windows/src-tauri/src/whatsapp/gateway.rs`: `Kind::Orbit`, `orbit::{status, chats, messages}` (parsers).
- `windows/src-tauri/src/whatsapp/provider.rs`: variante `Provider::Orbit`.
- `windows/src-tauri/src/whatsapp/mod.rs`: sem listener para `orbit`.
- `windows/src-tauri/src/whatsapp/prefs.rs`: aceita `orbit` em `cleaned()`.
- `windows/src/core/whatsapp.ts`, `windows/src/settings/whatsapp.ts`, `windows/src/core/bridge.ts`: opção "Usar a ponte do Orbit", sem botão de instalar nem campo de token.

## Testes
- Rust: parsers com JSON sintético (`orbit::*`); servidor falso em loopback (`TcpListener` em `127.0.0.1:0`, uma rota por caminho) para `status`, `recent_messages`, `messages`; `send` recusa sem conexão; `prefs` aceita `orbit`.
- TS: `linkLabel`/`testResultLabel` para Orbit sem QR; `DEFAULT_URLS.orbit`.

## Aceite
- Com a ponte rodando, "Usar a ponte do Orbit" + "Testar conexão" mostra o estado real.
- Sem o botão "Instalar WhatsApp" quando `orbit` está escolhido.
- Nenhuma chamada a `/api/sends` nem a rota de envio; nenhuma mensagem enviada.
- Checkpoint L8.6 (ilha com o Orbit aberto) fica **Real: NOT_RUN** até validar com a ponte real.
