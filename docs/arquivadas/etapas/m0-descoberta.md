> Parte do roadmap do fork. Índice, regras gerais e ordem das fases: [ROADMAP-FORK.md](../../ROADMAP-FORK.md).

## Fase M0 — Mochi: descoberta e licenças (P; sem código de produto)

Objetivo: trocar suposições por fatos antes de construir. Nada do app muda nesta fase. Provas de conceito ficam fora de `windows/src` e `windows/src-tauri/src` e não são publicadas.

Depende de: Checkpoint 2B.8 e Checkpoint 2C.3 — **reavaliados em 2026-11-15**: `2B` e `2C` foram pausadas (`D-053`), e a base funcional equivalente (`pipe.rs`, ilha, Toast nativo, `cli_relay.rs`, OAuth ChatGPT/Google, `phone_link/` e `secrets.rs`) foi entregue na Fase L até `windows-v0.7.4`.

Resultado de cada subfase: registrar em `docs/superpowers/specs/AAAA-MM-DD-mochi-m0-<tema>.md`, sem segredos, prompts ou caminhos pessoais.

### M0.1 — Licenças e manutenção

- [x] Para `vercel-labs/agent-desktop` e `vercel-labs/agent-browser`: ler a licença no repositório oficial, data da última release, suporte a Windows e forma de instalação.
- [x] Classificar cada um: reuso direto, adaptador (processo separado), só inspiração ou rejeitar.
- [x] Decidir se cada um é instalado à parte pelo usuário ou embutido no instalador; registrar o motivo.
- [x] Repositórios de referência do pacote (Greninja, CatchAI, MoHan, Calabrass, PowerToys): só leitura para ideias. Antes de copiar qualquer trecho, registrar a licença dele.
- [x] `understudy-ai/understudy` (MIT, conferido em 2026-11-11): só inspiração. É Node/TypeScript com helper Swift e a GUI só funciona no macOS; nenhum código serve ao app Tauri/Rust. As ideias aproveitadas estão marcadas "(Understudy)" em M1, M5 e M8; as rejeitadas, com motivo, em [parqueado.md](../../roadmap/parqueado.md).
- [x] (QwenPaw) `agentscope-ai/QwenPaw` (Apache 2.0, conferido em 2026-11-13; ver [pesquisa](../pesquisas/qwenpaw.md)): classificar o helper de computer use em Rust/Tauri (`console/src-tauri/src/computer_use_server/`, UI Automation, captura WGC, entrada e aprovação por *named pipe*) como alternativa ao Agent Desktop. O núcleo Python fica rejeitado.
- **Checkpoint M0.1 (concluído em 2026-11-15):** tabela completa em [2026-11-15-mochi-m0-licencas.md](../superpowers/specs/2026-11-15-mochi-m0-licencas.md) — `agent-desktop` (`v1.3.1`, `Apache-2.0`) e `agent-browser` (`v0.43.0`, `Apache-2.0`) como adaptadores instalados à parte pelo usuário; helper Rust do `QwenPaw` (`Apache-2.0`, `v2.3.0`) como reserva técnica/inspiração.

### M0.2 — Cérebro por provedor

- [x] Para Claude, ChatGPT/Codex e Gemini: verificar nos termos e na documentação oficial se o login OAuth da assinatura pode ser usado direto pelo Coucou ou só pela CLI oficial.
- [x] Para cada CLI: confirmar se aceita um servidor MCP local, como ele é registrado e em qual arquivo de configuração.
- [x] Ler `windows/src-tauri/src/cli_relay.rs` e `chat.rs`; anotar o que já existe para enviar pedido e receber resposta.
- [x] Não gravar em nenhuma configuração de agente nesta subfase.
- **Checkpoint M0.2 (concluído em 2026-11-15):** matriz registrada em [2026-11-15-mochi-m0-cerebro.md](../superpowers/specs/2026-11-15-mochi-m0-cerebro.md) — **Claude:** OAuth direto proibido -> `claude -p` (`--mcp-config` + `--strict-mcp-config`); **Gemini:** OAuth direto proibido para LLM (`google.rs` é só Drive/Calendar) -> CLI (`agy`/`gemini`) ou API key; **ChatGPT:** `chatgpt.rs` PKCE herdado + `codex exec` (`-c` / `--ignore-user-config`).

### M0.3 — Prova: volume

- [x] Em um projeto de teste à parte, ler o volume master pela API de áudio do Windows.
- [x] Alterar para um valor de teste, ler de novo e restaurar o valor original.
- [x] Repetir 10 vezes; registrar falhas.
- **Checkpoint M0.3 (concluído em 2026-11-15):** **10/10 PASS** via `windows` `0.61` (`IAudioEndpointVolume`), média `12.131 ms` (`10.566–15.279 ms`), valor original restaurado sem `powershell` nem binário externo ([2026-11-15-mochi-m0-provas.md](../superpowers/specs/2026-11-15-mochi-m0-provas.md)).

### M0.4 — Prova: desktop

- [x] Com o Agent Desktop, listar as janelas abertas e localizar um controle no Bloco de Notas (app de teste).
- [x] Repetir 10 vezes; medir tempo e falhas. Não clicar nem digitar em app real.
- **Checkpoint M0.4 (concluído em 2026-11-15):** **10/10 PASS** (`agent-desktop` `1.2.0` + `notepad.exe`), média `446.3 ms` (`418.1–505.4 ms`), `19` elementos com `"Editor de Texto"` localizado e comportamento minimizado anotado (`minimized: true`, `"states": ["offscreen"]`, `"Restaurar"`) ([2026-11-15-mochi-m0-provas.md](../superpowers/specs/2026-11-15-mochi-m0-provas.md)).

### M0.5 — Prova: web

- [x] Com o Agent Browser, abrir `https://example.com` em perfil isolado e ler o título.
- [x] Repetir 10 vezes; confirmar que o perfil principal do navegador do usuário não foi tocado.
- **Checkpoint M0.5 (concluído em 2026-11-15):** **10/10 PASS** (`agent-browser` `0.42.0`, `doctor` `8 pass, 0 warn, 0 fail`), média `378.2 ms` (`141.3 ms` quente / `2358.6 ms` cold launch), perfil principal do Chrome intacto e destino final `%LOCALAPPDATA%\Lumen\browser-profile` definido ([2026-11-15-mochi-m0-provas.md](../superpowers/specs/2026-11-15-mochi-m0-provas.md)).

### M0.6 — Mapa do que já existe

- [x] Localizar no código Windows: cartão Permitir/Negar da ilha, envio de chat, voz (`voice/`), ligação com o celular (`phone_link/`), cofre de segredos (`secrets.rs`) e sessão de mídia.
- [x] Para cada um, anotar o arquivo de entrada e se pode ser reutilizado sem mudar o visual.
- **Checkpoint M0.6 (concluído em 2026-11-15):** tabela completa pronta para a M1 em [2026-11-15-mochi-m0-mapa.md](../superpowers/specs/2026-11-15-mochi-m0-mapa.md).

### M0.7 — Fechamento

- [x] Juntar as tabelas de M0.1 a M0.6 em um relatório curto.
- [x] Listar bloqueios e perguntas que impedem a M1; cada uma com responsável.
- [x] Atualizar o Estado da M0 no índice.
- **Checkpoint M0 (concluído em 2026-11-15):** relatório e tabela de bloqueios registrados em [2026-11-15-mochi-m0-fechamento.md](../superpowers/specs/2026-11-15-mochi-m0-fechamento.md) — 3 provas reproduzíveis sem código de produto, zero credencial no repo e nenhuma configuração de agente alterada.
