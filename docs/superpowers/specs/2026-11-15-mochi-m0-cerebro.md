# M0.2 — Cérebro por provedor e registro de MCP local

**Data:** 2026-11-15  
**Fase:** [M0 — Descoberta e licenças](../../roadmap/arquivo/m0-descoberta.md)  
**Restrição observada:** Nenhuma configuração de agente (`<HOME>\.claude.json`, `<HOME>\.codex\config.toml`, `<HOME>\.gemini\...`) foi criada ou modificada nesta subfase.

## 1. Matriz de decisão por provedor (OAuth de assinatura vs. CLI vs. API paga)

| Provedor | OAuth direto da assinatura no app terceiro | MCP local na CLI oficial | Binário local verificado neste host | Caminho decidido para o Lumen (`M1.5+`) | Fontes consultadas |
|---|---|---|---|---|---|
| **Claude / Anthropic** | **Proibido** — Tokens OAuth de assinatura (`claude.ai`, Pro/Max/Team/Enterprise) são restritos exclusivamente aos clientes oficiais da Anthropic (`claude.ai`, Claude Desktop e CLI oficial Claude Code). | **Sim** (`claude mcp add/list` ou `--mcp-config <configs...> --strict-mcp-config`) | `claude` presente no PATH (`claude --help` verificado) | **CLI oficial obrigatória** (`claude -p` via `windows/src-tauri/src/claude_code.rs`) para assinatura; API key Console apenas se fornecida pelo usuário. | Termos de assinatura / docs Claude (`claude.com`), `claude --help`, `windows/src-tauri/src/claude_code.rs` |
| **ChatGPT / OpenAI** | **Parcial / Restrito ao fluxo oficial** — "Sign in with ChatGPT" (OAuth PKCE em `auth.openai.com`) existe apenas para apps/integrações autorizadas; reutilização não autorizada de tokens de assinatura viola política de uso automatizado. No código atual herdado do Coucou, `chatgpt.rs` implementa PKCE (`openid profile email offline_access resource.invoke chatgpt.tokens.use.direct`). | **Sim** (`codex mcp add/list`, `[mcp_servers.<id>]` no config TOML ou `-c` em `codex exec`) | `codex` presente no PATH (`codex --help` verificado) | **Duplo caminho compatível:** `chatgpt.rs` direto para chat/tool-calling compatível **e** `codex exec` via `cli_relay.rs` como fallback oficial de assinatura; chave OpenAI opcional para API paga. | Documentação OpenAI Codex / SIWC (`openai.com`, `learn.chatgpt.com`), `codex --help`, `windows/src-tauri/src/chatgpt.rs`, `windows/src-tauri/src/cli_relay.rs` |
| **Gemini / Google** | **Proibido para inferência LLM** — Google proíbe expressamente captar/reutilizar tokens OAuth do Gemini CLI ou Code Assist em apps terceiros (além de ter encerrado o fluxo CLI para assinaturas Google One AI Pro/Ultra de consumidor). O OAuth PKCE em `windows/src-tauri/src/google.rs` é exclusivo para Drive e Calendar. | **Sim** (`gemini mcp add` / `<HOME>\.gemini\settings.json` + `--allowed-mcp-server-names`; `agy mcp` / `agy --settings <arquivo>`) | `agy` presente no PATH (`agy --help` verificado); `gemini` não instalado (`NOT_FOUND`) | **CLI oficial (`agy` / `gemini`) ou API key paga (`google-api-key`)** — nunca reutilizar tokens de assinatura em HTTP direto no Lumen. | Documentação oficial Gemini CLI (`geminicli.com`), `agy --help`, `windows/src-tauri/src/google.rs`, `windows/src-tauri/src/cli_relay.rs` |

## 2. Como cada CLI registra e aceita um servidor MCP local

Leitura somente-leitura via `--help` e inspeção de `cli_relay.rs` e `claude_code.rs`:

1. **Claude Code (`claude`):**
   - Registro persistente opcional do usuário fica em `<HOME>\.claude.json` (`claude mcp add`).
   - **Descoberta chave para M1.5:** `claude` aceita as flags `--mcp-config <configs...>`, `--strict-mcp-config`, `--allowedTools`, `--setting-sources ""`, `--no-session-persistence` e `--disable-slash-commands`.
   - O arquivo `windows/src-tauri/src/claude_code.rs` **já usa** `--setting-sources "" --strict-mcp-config --no-session-persistence --disable-slash-commands` exatamente para não poluir nem carregar configurações pessoais do usuário. Em `M1.5`, basta adicionar `--mcp-config <arquivo-efemero-lumen.json>` (gerado em `%TEMP%` ou runtime dir apenas durante a chamada com o servidor MCP do Lumen) e `--allowedTools`, sem tocar `<HOME>\.claude.json`.

2. **Codex CLI (`codex`):**
   - Registro persistente do usuário fica em `<HOME>\.codex\config.toml` sob `[mcp_servers.<nome>]` (gerenciado por `codex mcp add/remove/list`).
   - `codex` aceita `--ignore-user-config` e `-c, --config <key=value>` na invocação, inclusive em `codex exec`.
   - Em `windows/src-tauri/src/cli_relay.rs`, o modo `Cli::Codex` hoje passa `exec --json --ephemeral --ignore-user-config --sandbox read-only --disable shell_tool --disable apps ...`. Para expor o MCP local do Lumen na `M1.5`, o adaptador precisará injetar a seção `mcp_servers.lumen` via `-c` (ou config efêmero autorizado) mantendo `--ignore-user-config` para isolar os demais servidores do usuário.

3. **Antigravity (`agy`) e Gemini CLI (`gemini`):**
   - `gemini` registra em `<HOME>\.gemini\settings.json` (`gemini mcp add`) e `cli_relay.rs` hoje o isola com `--allowed-mcp-server-names coucou-none --extensions none`. Para o Lumen Mochi, quando `gemini` estiver instalado, troca-se o filtro de permissão para `--allowed-mcp-server-names lumen` (ou configuração efêmera equivalente).
   - `agy` (presente no host) oferece subcomando `agy mcp` e flag `--settings <json-or-path>`, permitindo passar um arquivo de settings isolado sem editar as configurações pessoais do usuário.

## 3. O que `chat.rs` e `cli_relay.rs` já oferecem (e o que falta para M1)

Leitura direta de `windows/src-tauri/src/chat.rs` e `windows/src-tauri/src/cli_relay.rs`:

- **`chat.rs` já oferece:**
  - Roteamento unificado por `ChatState` entre provedores configurados (`chatgpt`, Anthropic, compatíveis OpenAI, Ollama local e `cli_relay`).
  - Comandos e entradas reutilizáveis: `send` (turnos com streaming para o Webview), `send_for_phone` (turnos para o canal do celular), `complete_oneshot` (usado por `notes`/L3 e resumo de e-mail L5) e `complete_summary`.
  - Listagem de modelos e histórico em memória.
- **`cli_relay.rs` já oferece:**
  - Abstração `Cli` com `Claude`, `Antigravity`, `Gemini`, `Codex`, `Cursor`, `Copilot`.
  - Resolução de executável em Windows (`.cmd`, `.exe`, caminhos npm/AppData), execução assíncrona com streaming/JSON (`send` e `complete`), limpeza de variáveis sensíveis e tratamento de erro.
- **Lacunas mapeadas para `M1`:**
  1. `chat.rs` não possui ainda um **registro único de ferramentas (tool schemas)** nem loop uniforme de *tool calling* entre OAuth direto (`chatgpt.rs`) e CLIs via MCP.
  2. `voice/brain.rs` hoje está desacoplado de `chat.rs`: exige `anthropic-api-key` fixa (`crate::claude::voice_turn`) com 6 ferramentas embutidas (`pills`, `music`, `service_info`, `open_app`, `prepare_email`, `web_search`). Precisará migrar na M4 para o mesmo cérebro de `M1`.
  3. `claude_code.rs` e `cli_relay.rs` hoje travam todos os MCPs (`--strict-mcp-config` vazio / `--allowed-mcp-server-names coucou-none` / `--disable apps`). Em `M1.5` precisarão liberar **somente** o servidor MCP local do Lumen (`stdio` ou named pipe autenticado por sessão).
