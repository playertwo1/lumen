# M0.1 — Licenças, manutenção e estratégia de distribuição

**Data:** 2026-11-15  
**Fase:** [M0 — Descoberta e licenças](../../etapas/m0-descoberta.md)  
**Escopo:** Verificação factual de licenças, atividade upstream, suporte Windows e decisão de empacotamento para automação desktop e web do Lumen (sem alterações de código de produto).

## 1. Tabela consolidada de licenças e classificação

| Componente / Repositório | Licença oficial | Última release (GitHub API em 2026-11-15) | Último push / Atividade | Suporte Windows | Versão local verificada | Classificação | Forma de instalação |
|---|---|---|---|---|---|---|---|
| `vercel-labs/agent-desktop` | `Apache-2.0` | `v1.3.1` (`2026-11-13T21:02:21Z`) | `2026-11-15T12:53:40Z` (`archived: false`) | Sim — CLI (`x86_64`/`aarch64-pc-windows-msvc`) e FFI ZIP nativos | `1.2.0` (`agent-desktop-win32-x64.exe`) | **Adaptador (processo separado)** | **À parte pelo usuário** (ou sob demanda em `%LOCALAPPDATA%\Lumen\bin`) |
| `vercel-labs/agent-browser` | `Apache-2.0` | `v0.43.0` (`2026-11-12T23:09:31Z`) | `2026-11-15T12:43:23Z` (`archived: false`) | Sim — CLI (`agent-browser-win32-x64.exe`) + modo `mcp` + `--profile` | `0.42.0` (`doctor`: `8 pass, 0 warn, 0 fail`, Chrome `157.0.8103.51`) | **Adaptador (processo separado)** | **À parte pelo usuário** (`npm install -g agent-browser`) |
| `agentscope-ai/QwenPaw` (helper `console/src-tauri/src/computer_use_server/` + `bin/qwenpaw-computer-use-helper.rs`) | `Apache-2.0` | `v2.3.0` / commit `f47dd19` (`2026-11-13`) | Ativo | Sim (Rust WGC + UIA + enigo + named pipe, ~2.800 linhas) | Avaliado em repo (`docs/arquivadas/pesquisas/qwenpaw.md`) | **Só inspiração / reserva técnica** (captura WGC e pipe em M2.4/M5); **núcleo Python rejeitado** | Não empacotado |
| `understudy-ai/understudy` | `MIT` | Conferido em `2026-11-11` | — | Não para GUI (helper Swift restrito ao macOS) | — | **Só inspiração** (regras de negócio em M1, M5 e M8) | Não empacotado |
| `microsoft/PowerToys` | `MIT` | Conferido no fork (`parqueado.md`) | — | Sim (C#/Win32) | — | **Só inspiração** (conector parqueado em `parqueado.md`) | Não empacotado |
| Repositórios de referência do pacote original (`Greninja`, `CatchAI`, `MoHan`, `Calabrass`) | A conferir antes de qualquer cópia | — | — | — | Nenhum trecho copiado | **Só leitura para ideias** | Não empacotado |

## 2. Motivos da classificação e decisão de instalação

1. **`agent-desktop` como adaptador externo (instalado à parte):**
   - Mantém licença compatível (`Apache-2.0`), está ativo (`pushed_at: 2026-11-15`) e já entrega binários Windows nativos (`win32-x64` / `x86_64-pc-windows-msvc`), além de suportar CLI JSON (`list-windows`, `snapshot`, ações por `window-id`/`ref`).
   - Embutir no instalador do Lumen aumentaria o peso do setup (~19 MB atuais na `0.7.4`), exigiria assinar binários adicionais para evitar bloqueios de SmartScreen/Defender e acoplaria releases do desktop/Lumen a cada patch semanal do upstream. Instalado à parte (`npm install -g agent-desktop` ou download explícito confirmado pelo usuário para `%LOCALAPPDATA%\Lumen\bin`), o Lumen verifica presença e versão via adaptador Rust (`M2.4`) com degradação graciosa se ausente.

2. **`agent-browser` como adaptador externo (instalado à parte):**
   - Licença `Apache-2.0`, ativo (`v0.43.0`), suporte Windows validado localmente (`agent-browser doctor` com `8 pass, 0 warn, 0 fail`).
   - Possui gerenciamento próprio de daemon e runtime de navegador/Chrome além do CLI `win32-x64`. Embutir isso dentro do instalador MSI/NSIS do Lumen tornaria o pacote pesado e frágil em atualizações silenciosas. Fica como ferramenta opcional instalada à parte (`npm install -g agent-browser`) e invocada sempre com `--profile %LOCALAPPDATA%\Lumen\browser-profile` (`M5`).

3. **`QwenPaw` helper Rust como reserva técnica / inspiração:**
   - Embora tenha licença `Apache-2.0` e implementação Rust/UIA/WGC em Tauri (`console/src-tauri/src/computer_use_server/`), na M0.4 o `agent-desktop` já demonstrou captura confiável de árvore UIA no Windows 11 (`notepad.exe`, 19 refs, 10/10 PASS).
   - O desenho do helper QwenPaw (captura WGC direta em Rust e aprovação por named pipe) fica documentado como alternativa caso M5 precise de fallback visual quando UIA falhar em apps Electron/Canvas. O núcleo Python e módulos de telemetria permanecem rejeitados.

4. **Referências somente-leitura (`Understudy`, `PowerToys`, `Greninja`, `CatchAI`, `MoHan`, `Calabrass`):**
   - Nenhum código foi copiado. Qualquer reaproveitamento futuro de trecho de `Greninja`, `CatchAI`, `MoHan` ou `Calabrass` exige antes checar e registrar o arquivo `LICENSE` oficial do respectivo repositório.
