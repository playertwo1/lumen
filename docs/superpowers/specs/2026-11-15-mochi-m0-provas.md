# M0.3, M0.4 e M0.5 — Provas de conceito técnicas (Volume, Desktop e Web)

**Data:** 2026-11-15  
**Fase:** [M0 — Descoberta e licenças](../../roadmap/m0-descoberta.md)  
**Isolamento:** Todos os projetos e perfis de teste rodaram em diretório temporário fora do repositório (`scratch`), sem alterações em `windows/src` ou `windows/src-tauri/src`, e foram removidos ao final das execuções. Nenhum clique ou digitação foi efetuado em aplicativos reais do usuário.

---

## M0.3 — Prova: Volume master pela API do Windows

- **Implementação testada:** Projeto Rust isolado fora do repositório utilizando diretamente `windows` `0.61` (`IMMDeviceEnumerator` -> `GetDefaultAudioEndpoint(eRender, eConsole)` -> `Activate::<IAudioEndpointVolume>` -> `GetMasterVolumeLevelScalar` / `SetMasterVolumeLevelScalar`), sem invocar `powershell.exe`, nircmd ou processos externos.
- **Descoberta técnica de build:** No crate `windows` `0.61`, `IMMDevice::Activate` exige habilitar tanto `Win32_Media_Audio` e `Win32_Media_Audio_Endpoints` quanto `Win32_System_Com_StructuredStorage` e `Win32_System_Variant` para compilar `PROPVARIANT`.
- **Protocolo:** Em 10 iterações consecutivas: ler `orig`, gravar `target = 0.3500`, reler `read_test`, restaurar `orig` e reler `read_restored`.
- **Resultados medidos (`cargo run --offline`, saída integral resumida):**
  - **Sucesso:** **10/10 PASS** (`orig=0.0000`, `read_test=0.3500`, `read_restored=0.0000` em todas as iterações).
  - **Latência total por ciclo (ler -> alterar -> conferir -> restaurar -> conferir):**
    - Mínima: `10.566 ms` (`10566 us`)
    - Média: `12.131 ms` (`12131 us`)
    - Máxima: `15.279 ms` (`15279 us`)
- **Critério do Checkpoint M0.3:** Atendido (`10/10`, valor original restaurado, zero `powershell` e zero binário externo).

---

## M0.4 — Prova: Automação Desktop com Agent Desktop (`notepad.exe`)

- **Ferramenta testada:** `agent-desktop` `1.2.0` (`vercel-labs/agent-desktop`, `Apache-2.0`).
- **App alvo controlado:** Instância limpa de `notepad.exe` aberta em segundo plano apenas para o teste e encerrada ao término pelo seu PID (`25888`, sem tocar outras janelas).
- **Descobertas de contrato e ambiente (Windows 11):**
  1. `notepad.exe` no Windows 11 passa por um stub AppExecutionAlias que gera processo filho `Notepad`; o identificador de app reportado pelo `agent-desktop` é `app_name: "notepad.exe"`.
  2. `agent-desktop list-windows --app notepad.exe` retorna envelope JSON `{ version, ok, command, data: [...] }` (`data` é um array plano de janelas, cada uma com `id`, `title`, `app_name`, `pid`, `bounds`, `minimized`, `visible`).
  3. O campo editável do Bloco de Notas foi localizado via `snapshot --window-id w-1576628 -i --compact` como elemento de nome `"Editor de Texto"`, `role="textfield"`, `role_description="documento"`, `native_id="15"`, com ações disponíveis `["SetValue", "Scroll", "SetFocus"]`.
- **Resultados medidos em 10 ciclos consecutivos (`list-windows` + `snapshot -i --compact`, sem clicar/digitar):**
  - **Sucesso:** **10/10 PASS** (`complete: true`, `ref_count: 19` em todos os ciclos).
  - **Latência por ciclo completo:**
    - Mínima: `418.1 ms`
    - Média: `446.3 ms`
    - Máxima: `505.4 ms`
- **Comportamento com janela minimizada (medido):**
  - Após `agent-desktop minimize --window-id w-1576628` (`ok: true`, `verified: true`):
  - `list-windows --app notepad.exe` continua listando a janela com `minimized: true`, `visible: true` e `bounds: { x: 0, y: 1713, width: 237, height: 39 }`.
  - `snapshot --window-id w-1576628 -i --compact` ainda retorna `ok: true`, `complete: true` e os mesmos `19` elementos (`Editor de Texto` preservado), porém marca os controles e a janela com `"states": ["offscreen"]` e altera o botão `"Minimizar"` para `"Restaurar"` (`Minimize-Restore`).
  - **Implicação para M5:** Antes de clicar visualmente por coordenadas ou focar campos em apps terceiros, o Mochi deve checar `minimized` / `"offscreen"` e restaurar ou trazer a janela ao foco somente após aprovação do usuário.
- **Critério do Checkpoint M0.4:** Atendido (taxa `10/10`, latência medida ~446 ms e comportamento minimizado documentado).

---

## M0.5 — Prova: Automação Web isolada com Agent Browser (`https://example.com`)

- **Ferramenta testada:** `agent-browser` `0.42.0` (`vercel-labs/agent-browser`, `Apache-2.0`; pré-checagem `agent-browser doctor`: `8 pass, 0 warn, 0 fail`).
- **Perfil isolado:** Diretório temporário dedicado (`scratch\m0-browser-profile`) passado explicitamente via `--profile <dir>`. Na arquitetura final (`M5`), o equivalente será `%LOCALAPPDATA%\Lumen\browser-profile`.
- **Resultados medidos em 10 ciclos (`open https://example.com` + `get title --json`):**
  - **Sucesso:** **10/10 PASS** — todas as 10 iterações retornaram `"Example Domain"`.
  - **Latência por ciclo:**
    - Iteração 01 (cold launch do daemon + Chromium): `2358.6 ms`
    - Iterações 02 a 10 (`reused: true`): `141.3 ms` a `189.2 ms`
    - Mínima: `141.3 ms` · Média geral (10 ciclos): `378.2 ms` · Máxima: `2358.6 ms`
- **Verificação de não interferência no perfil principal do usuário:**
  - `15` entradas criadas dentro do diretório isolado de teste.
  - Timestamp UTC do arquivo `%LOCALAPPDATA%\Google\Chrome\User Data\Default\Preferences` conferido antes e depois da bateria: `2026-11-10T02:43:40.6829335Z` (`UNCHANGED = True`), confirmando isolamento completo.
  - Ao final, o daemon de teste foi encerrado (`agent-browser --profile ... close` -> `✓ Browser closed`) e a pasta temporária removida.
- **Critério do Checkpoint M0.5:** Atendido (`10/10`, perfil principal intacto, destino isolado `%LOCALAPPDATA%\Lumen\browser-profile` definido para M5).
