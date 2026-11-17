# M0.7 — Fechamento da Fase M0 (Descoberta e Licenças)

**Data:** 2026-11-15  
**Fase:** [M0 — Descoberta e licenças](../../etapas/m0-descoberta.md)  
**Estado final:** **Concluída** (zero código de produto alterado, zero credencial no repositório, zero arquivo de configuração de agente modificado).

## 1. Síntese executiva dos resultados (M0.1–M0.6)

| Subfase | Entregável | Resultado verificado | Spec detalhada |
|---|---|---|---|
| **M0.1** | Licenças e estratégia de distribuição | `agent-desktop` (`v1.3.1`, `Apache-2.0`) e `agent-browser` (`v0.43.0`, `Apache-2.0`) classificados como **adaptadores externos instalados à parte pelo usuário**. Helper Rust do `QwenPaw` (`Apache-2.0`) classificado como **reserva técnica/inspiração**. Referências `Understudy`/`PowerToys`/pacote original somente-leitura. | [2026-11-15-mochi-m0-licencas.md](2026-11-15-mochi-m0-licencas.md) |
| **M0.2** | Cérebro por provedor e MCP local | **Claude:** OAuth assinatura proibido fora do app oficial -> CLI `claude -p` (`--mcp-config` + `--strict-mcp-config`). **Gemini:** OAuth assinatura proibido fora dos clientes oficiais (`google.rs` é só Drive/Calendar) -> CLI (`agy`/`gemini`) ou API key. **ChatGPT:** `chatgpt.rs` PKCE herdado + `codex exec` com `-c` / `--ignore-user-config`. `chat.rs` e `cli_relay.rs` mapeados. | [2026-11-15-mochi-m0-cerebro.md](2026-11-15-mochi-m0-cerebro.md) |
| **M0.3** | Prova: volume master Windows | `windows` `0.61` (`IAudioEndpointVolume`): **10/10 PASS**, restauração exata do volume original, média `12.131 ms` (`10.566–15.279 ms`), zero `powershell`/processo externo. | [2026-11-15-mochi-m0-provas.md](2026-11-15-mochi-m0-provas.md) |
| **M0.4** | Prova: Agent Desktop (`notepad.exe`) | **10/10 PASS** em `list-windows` + `snapshot -i --compact` (`19` elementos, `"Editor de Texto"` localizado, média `446.3 ms`). Comportamento minimizado medido (`minimized: true`, `"states": ["offscreen"]`, botão vira `"Restaurar"`). | [2026-11-15-mochi-m0-provas.md](2026-11-15-mochi-m0-provas.md) |
| **M0.5** | Prova: Agent Browser (`https://example.com`) | **10/10 PASS** com perfil isolado (`141.3 ms` a `2358.6 ms`, média `378.2 ms`), título `"Example Domain"` em todas e `Default\Preferences` do Chrome principal intacto (`UNCHANGED = True`). | [2026-11-15-mochi-m0-provas.md](2026-11-15-mochi-m0-provas.md) |
| **M0.6** | Mapa do código existente | Card Permitir/Negar (`pipe.rs`/`island.ts`/`permission_notifications.rs`), Chat (`chat.rs`/`views/chat.ts`), `phone_link/`, `secrets.rs` e mídia (`spotify.rs`) reutilizáveis sem mudar visual; `voice/brain.rs` precisa migrar da chave Anthropic fixa na M4. | [2026-11-15-mochi-m0-mapa.md](2026-11-15-mochi-m0-mapa.md) |

## 2. Reavaliação da dependência antiga (`2B.8` e `2C.3`)

Conforme instruído em [ROADMAP-FORK.md](../../../ROADMAP-FORK.md):
- As fases `2B` e `2C` foram formalmente **pausadas (D-053)**.
- A base técnica de execução exigida para a M0/M1 (hooks e cartão Permitir/Negar em `pipe.rs`/`island.ts`, Toast nativo WinRT, `cli_relay.rs`, chat ChatGPT OAuth PKCE, `phone_link/` TLS+relay, cofre `secrets.rs` e mídia GSMTC) já está **presente e operante na `main` por meio da Fase L** (até `windows-v0.7.4`).
- Portanto, a dependência histórica de `2B.8`/`2C.3` está **reavaliada e superada pela Fase L**.

## 3. Bloqueios e pendências antes de abrir código da Fase M1

| Item | Natureza | Por que impede / condiciona a M1 | Responsável | Status |
|---|---|---|---|---|
| **Checkpoints reais da Fase L (`L3`, `L4`, `L5`, `L6`, `L7` no notebook + S25 + `windows-v0.7.4`)** | Regra do roadmap (`ROADMAP-FORK.md`: "As fases M só começam depois do Checkpoint L7") | A M0 foi concluída em modo puramente investigativo sem código de produto; abrir código da **M1** exige o fechamento real de L7 (e testes reais pendentes da ponte L3–L6) pelo dono, ou liberação explícita do dono para seguir sinteticamente. | **Dono (usuário)** | Pendente |
| **Spec detalhada + Plano da Fase M1** | Governança (`m1-fundacao.md` + `ROADMAP-FORK.md`) | Fixar nomes exatos de módulo (`windows/src-tauri/src/mochi/`), esquema do catálogo único de ferramentas e protocolo do MCP local efêmero para `claude`/`codex`/`agy`/`gemini` antes do primeiro commit de código M1. | **Agente de código (na abertura da M1)** | A iniciar após L7 / autorização |
| **Escopo do `chatgpt.rs` em chamadas com tool-calling** | Técnico / Termos | Confirmar na abertura da M1 se o endpoint atual de `chatgpt.rs` aceita definições de `tools` no fluxo de respostas ou se ferramentas do Mochi rodarão inicialmente apenas via MCP nas CLIs (`claude`, `codex`, `agy`) e chave API quando configurada. | **Agente de código + Dono (em M1.5)** | Mapeado para M1.5 |
| **CLI `gemini` ausente neste host** | Ambiente local | `gemini` retornou `NOT_FOUND` no host atual; validações reais da M1.5 com Gemini CLI exigirão instalação prévia ou validação via `agy` + chave API Google quando aplicável. | **Dono (se desejar testar `gemini` CLI real)** | Informativo (não bloqueia M1 geral) |
| **Licença das referências do pacote original (`Greninja`, `CatchAI`, `MoHan`, `Calabrass`)** | Jurídico / Licenciamento | Continuam estritamente **somente-leitura** até que qualquer trecho específico precise ser copiado (momento em que a licença deverá ser verificada antes). | **Agente de código (sob demanda)** | Mitigado (sem cópia prevista) |

## 4. Conferência do Checkpoint M0

- [x] Três provas reproduzíveis executadas em código/comandos isolados fora do produto (`10/10` volume Rust COM, `10/10` Agent Desktop `notepad.exe`, `10/10` Agent Browser `--profile` isolado).
- [x] Nenhuma credencial, token ou caminho pessoal gravado no repositório.
- [x] Nenhuma configuração de agente alterada (`~/.claude.json`, `~/.codex`, `~/.gemini` intactos).
- [x] Decisão OAuth/CLI registrada por provedor em M0.2.
