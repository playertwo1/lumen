# Spec — Fase M2: Mochi, controle nativo do Windows

**Data:** 2026-11-17  
**Fase:** [M2 — Controle nativo do Windows](../../roadmap/m2-windows-nativo.md) (`M2.1` a `M2.7`)  
**Depende de:** [M1 — Fundação](../../arquivadas/superpowers/specs/2026-11-16-m1-fundacao.md), fechada em 2026-11-17 ([estado](../../STATE-FORK.md))  
**Regras gerais e das fases M:** [AGENTS.md](../../../AGENTS.md)  
**Plano:** [2026-11-17-m2-windows-nativo.md](../plans/2026-11-17-m2-windows-nativo.md)  
**Branch:** `feat/m2-windows-nativo`

## 1. Objetivo

Entregar seis famílias de comandos de rotina do Windows (volume e mudo, brilho, aplicativos, janelas, mídia e energia) como ferramentas do contrato da M1 (`ToolDefinition` em `windows/src-tauri/src/mochi/registry.rs`), chamadas pelo cérebro do chat ou pelo servidor MCP local. Cada ferramenta usa só APIs nativas do Windows. Nenhuma monta comando, caminho ou shell a partir do texto do pedido, e nenhuma usa Agent Desktop.

Ao fechar a M2, 10 comandos de rotina funcionam pelo chat de forma repetível, e toda falha aparece como falha, com motivo.

## 2. Escopo que NÃO faz parte da M2

- Agent Desktop, visão e automação de tela (fase M5).
- PowerShell, `cmd`, `shutdown.exe`, `rundll32` e `explorer` com argumento vindo do pedido.
- Voz (M4), Obsidian (M3), arquivos, navegador (M6), celular (M7) e rotinas (M8).
- Elevação (UAC), hibernação, troca da saída de áudio, volume por aplicativo, brilho relativo ("mais claro") e qualquer ação além da lista de cada subetapa.
- Tela de configuração nova ou chave para o usuário habilitar ferramentas. Nesta fase a habilitação é feita no código (`ToolRegistry::with_default_tools`), como o `ping` na M1.
- Alteração visual em telas, pílulas ou cartões já publicados (AGENTS). A M2 não adiciona tela. Se surgir frase de interface, ela vai em `windows/src/i18n/extra.json` nas 13 línguas.
- Apps da Microsoft Store na lista de aplicativos (decisão em aberto, seção 9).

## 3. Código existente para reaproveitar

Busca feita em `windows/src-tauri/src/` antes de propor código novo:

| Área | Código existente (arquivo: função) | Uso na M2 | Limite |
|---|---|---|---|
| Áudio COM | `flow/hush.rs`: `imp::sessions`, `mute_others`, `restore`. `spotify.rs`: `win::spotify_audio`, `win::spotify_volume`, `win::set_spotify_volume` | Copiar o padrão: `CoCreateInstance(MMDeviceEnumerator)`, `GetDefaultAudioEndpoint(eRender, eMultimedia)`, `Activate`. | Os dois mexem em sessões por aplicativo, não no volume master. `hush` guarda em arquivo a lista de sessões mudas: a M2 não usa essa lista. |
| Brilho | Nada no código. `vitals.rs`: `mod os`, `thermal_zones` mostram a conexão WMI (`IWbemLocator`, `ConnectServer`, `CoSetProxyBlanket`). `fullscreen.rs`: `MonitorFromWindow`, `GetMonitorInfoW` | Padrão de conexão WMI para o brilho interno (namespace `root\wmi`, diferente do que `vitals.rs` usa). DDC/CI é código novo. | Sem reaproveitamento de lógica de brilho. |
| Aplicativos | `voice/mod.rs`: `open_app` e `find_app` (varredura de `.lnk` em `%APPDATA%` e `%ProgramData%` no Menu Iniciar). `voice/wake.rs`: `normalize`. `platform/windows.rs`: `reveal_folder`, `shell_open_uri` | A varredura vira função `pub(crate)` compartilhada. `normalize` serve para comparar nomes. | A escolha de `find_app` por "contém" não serve: a M2.3 exige pedir escolha quando o nome é ambíguo. `shell_open_uri` é privada hoje. |
| Janelas | `platform/windows.rs`: `top_windows` (EnumWindows com filtro de janela visível sem dono), `bring_forward`, `process_table`. `window_context.rs` (EnumWindows, GetWindowRect). `fullscreen.rs` (área do monitor) | Enumeração, área de trabalho do monitor, processo da janela. | `top_windows` devolve só handle, pid e título. A M2.4 precisa do retângulo, lido pelo DWM. `session_window.rs` escolhe janela de sessão de terminal: não serve. |
| Mídia | `spotify.rs`, módulo `win`: `com`, `Manager::RequestAsync`, `control` com a macro `asked!`, id por hash (`spotify:{kind}:{hash}`) | Mesma API de sessão de mídia (GSMTC) que a pílula de música usa. | `find_session` só acha Spotify e YouTube Music, não o player ativo. A pílula não muda (seção 5.5). |
| Energia | Nada no código. Busca por `LockWorkStation`, `SetSuspendState`, `ExitWindowsEx`, `InitiateSystemShutdown`, `shutdown.exe` sem resultado. Só `rundll32` em `platform/windows.rs`: `open_url` | Criar do zero. `platform/windows.rs`: `current_user_sid` mostra `OpenProcessToken` para o privilégio de desligamento. | Nenhum. |
| Sanitização | `mochi/sanitize.rs`: `sanitize_external_text`, `format_untrusted_context` (M1.6) | Nomes de apps, títulos de janela e nomes de faixa passam por aqui antes de voltar ao modelo. | Nenhum. |
| Registro | `mochi/audit.rs`: `AuditLog::record`, `redact_summary` | O resumo vem de `audit_summary`. | Nenhum. |
| Permissões | `mochi/permissions.rs`: `DefaultDenyRequester`; `mochi/mod.rs`: helper de teste `FixedRequester` | Testes de negar, expirar e aceitar. | Nenhum. |

Não reaproveitar: `platform/windows.rs: cli_candidates` e `find_on_path` procuram CLIs de agentes, não aplicativos.

## 4. Features do crate `windows` e dependências

Já ligadas em `windows/src-tauri/Cargo.toml` (bloco `cfg(windows)`), e que a M2 usa: `Foundation`, `Media_Control`, `Storage_Streams`, `Win32_Foundation`, `Win32_Media_Audio`, `Win32_System_Com`, `Win32_System_Power`, `Win32_System_Wmi`, `Win32_System_Threading`, `Win32_System_Diagnostics_ToolHelp`, `Win32_Graphics_Gdi`, `Win32_Graphics_Dwm`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Shell`, `Win32_Security`, `Win32_Security_Authorization`.

A acrescentar, cada uma no commit da subetapa que a usa (confirmar ao compilar):

- M2.1: `Win32_Media_Audio_Endpoints` (`IAudioEndpointVolume`).
- M2.2: `Win32_Devices_Display` (DDC/CI: `GetPhysicalMonitorsFromHMONITOR`, `GetVCPFeatureAndVCPFeatureReply`, `SetVCPFeature`, `DestroyPhysicalMonitors`).
- M2.4: `Win32_UI_HiDpi` só se o encaixe precisar de DPI por monitor. A alternativa é usar o `rcWork` do monitor em pixels físicos, que já está disponível.
- M2.6: `Win32_System_Shutdown` (`LockWorkStation`, `ExitWindowsEx`).

Nenhuma crate nova. `tokio` já tem `rt` (usado por `spawn_blocking`). Todo código Windows fica em `#[cfg(windows)]`. A versão para outros sistemas devolve `Availability::Unavailable` com o motivo "exclusivo do Windows", para o build Linux continuar compilando.

## 5. Decisões de contrato (valem para todas as subetapas)

5.1. **Risco é fixo por ferramenta.** `ToolDefinition.risk` não depende dos argumentos. Por isso a energia são quatro ferramentas: `bloquear_tela` (médio), `suspender`, `reiniciar` e `desligar` (altos). Um id nunca muda de risco.

5.2. **`verify` é pura e não lê o sistema.** Sua assinatura é `fn(&args, &output)`. O padrão é: `execute` age, relê o estado real e devolve `{ "pedido": …, "observado": … }`; `verify` só compara os dois. Sem essa separação, a verificação depois de agir não existe.

5.3. **`dry_run` e `audit_summary` são puros.** Não consultam o sistema. A frase do cartão e o registro vêm dos argumentos. Regras que dependem da combinação de argumentos (por exemplo, `nivel` obrigatório quando `acao` é `definir_volume`) ficam em `dry_run`, que roda antes da permissão e devolve `InvalidInput`. O validador de `validate_schema` não tem `anyOf`, `oneOf`, `not`, `pattern` nem condicionais.

5.4. **`check_availability` é síncrona e só lê o `ToolContext`.** A sondagem do sistema acontece antes, em uma nova função `mochi::host::context()` (arquivo novo `mochi/host.rs`), que preenche `ToolContext.capabilities`. Capacidades: `audio_endpoint`, `internal_display`, `ddc_display`, `display`, `start_menu`, `media_session`, `sleep_supported`, `shutdown_privilege`. Os pontos que hoje montam o contexto passam a usar essa função: `chat_send_with_mochi` (`mochi/chat_bridge.rs`, chamado por `lib.rs`) e `mcp_server.rs`, que hoje usa `ToolContext::default()` em `tools/list` e `tools/call`. A sondagem de DDC/CI tem cache curto (seção 9).

5.5. **Objetos COM não atravessam `await`.** COM não é `Send`. Toda chamada ao sistema roda em `tokio::task::spawn_blocking`, com `CoInitializeEx` na própria thread e com os objetos criados dentro dela. `execute` continua conferindo `ExecutionContext::is_cancelled()` antes de agir. O `spotify.rs` não muda: a pílula de música continua como está, e o padrão de COM é copiado em poucas linhas.

5.6. **Backend atrás de uma função pequena, e sem alcance do real nos testes.** Cada ferramenta separa três partes:
- (a) um plano puro, que vai de argumentos a operação;
- (b) uma função `run_with(backend, plano)` que fala com o sistema por um trait pequeno;
- (c) uma função `os()` que devolve a implementação real, só em `#[cfg(windows)]`.

`execute` escolhe o backend assim: em `cfg(test)` vale o backend falso e a implementação real não é alcançável. Testes nunca chamam `LockWorkStation`, `SetSuspendState`, `ExitWindowsEx`, `SetVCPFeature` nem `ShellExecuteExW`. Em build normal vale o real.

5.7. **Texto do sistema é dado.** Nomes de aplicativos, títulos de janela e nomes de faixa passam por `sanitize_external_text` antes de sair em `output` e são encapsulados em `<external_context untrusted="true" data_only="true">` ao voltar ao modelo. Nada disso entra no `audit_summary`. Nomes de dispositivo de áudio e de monitor não saem em `output`.

5.8. **Pausa e cancelamento.** Nada novo. `invoke_tool` já bloqueia com a pausa rápida antes de qualquer ferramenta. As ferramentas de energia não têm cancelamento depois do clique aceito, porque a chamada é imediata.

5.9. **Registro.** `ToolRegistry::with_default_tools()` (`mochi/registry.rs`) passa a chamar `tools::register_m2(&mut registry)` (arquivo novo `mochi/tools/mod.rs`). Cada subetapa acrescenta as suas ferramentas nessa função com `register_and_enable`. Desabilitar uma ferramenta é tirar a linha dela.

## 6. Subetapas

Todas as ferramentas usam `timeout` de 5 s, exceto `apps` (`abrir`, 10 s) e `brilho` (8 s), porque esperam leituras de confirmação.

### 6.1. M2.1: volume e mudo (risco baixo)

- **Id:** `audio`
- **Descrição:** "Lê o volume do sistema, define o volume de 0 a 100, silencia ou reativa o som."
- **Schema de entrada:**

```json
{
  "type": "object",
  "properties": {
    "acao": { "type": "string", "enum": ["ler", "definir_volume", "silenciar", "reativar_som"] },
    "nivel": { "type": "integer", "minimum": 0, "maximum": 100 }
  },
  "required": ["acao"],
  "additionalProperties": false
}
```

  `nivel` é obrigatório com `definir_volume`. Com outra ação, `dry_run` recusa a chamada, para não haver dúvida sobre o que foi pedido.
- **Risco:** `Low`.
- **API:** `IMMDeviceEnumerator` → `GetDefaultAudioEndpoint(eRender, eMultimedia)` → `IMMDevice::Activate::<IAudioEndpointVolume>` → `SetMasterVolumeLevelScalar`, `GetMasterVolumeLevelScalar`, `SetMute`, `GetMute`.
- **Por que texto não vira comando:** `acao` é enum e `nivel` é inteiro validado, convertido para escalar entre 0 e 1 dentro do código. Nenhum argumento chega a processo, caminho ou shell.
- **`check_availability`:** disponível com `audio_endpoint`. Sem dispositivo de saída, `Unavailable("sem dispositivo de saída de áudio")`. Se o pedido chegar mesmo assim, `execute` responde `PrerequisiteMissing` com o mesmo motivo.
- **`verify`:** em `definir_volume`, `|observado − pedido| ≤ 1`. Em `silenciar` e `reativar_som`, o mudo observado tem de ser igual ao pedido. Em `ler`, a leitura é o resultado. Divergência vira `VerificationFailed`.
- **Saída:** `{ "acao", "nivel" (0 a 100), "mudo" (booleano) }`.
- **Registro:** vai "volume em 20%", "som silenciado", "som reativado" ou "leitura de volume". Não vai o nome do dispositivo nem o volume de outros aplicativos.
- **Testes sem hardware:**
  - schema: 0 e 100 aceitos; −1, 101, 20,5, texto e campo extra recusados;
  - `dry_run`: `definir_volume` sem `nivel` recusado; `nivel` com `silenciar` recusado;
  - `verify` pura, com tolerância;
  - `run_with` com backend falso: sem endpoint vira `PrerequisiteMissing`; endpoint que não aplica o valor vira `VerificationFailed`; caso correto vira sucesso.
- **Teste manual no chat:** "volume 20%" relata "volume em 20%"; "tira o som" relata "som silenciado"; "volume 101%" é recusado e o volume não muda. Com a saída de áudio desligada ou ausente, "volume 20%" responde "sem dispositivo de saída de áudio".
- **Arquivos novos:** `mochi/host.rs`, `mochi/tools/mod.rs`, `mochi/tools/audio.rs`.
- **Arquivos alterados:** `mochi/mod.rs` (`pub mod host;`, `pub mod tools;`), `mochi/registry.rs` (`with_default_tools` chama `tools::register_m2`), `mochi/chat_bridge.rs` e `mochi/mcp_server.rs` (contexto vindo de `host::context()`), `windows/src-tauri/Cargo.toml` (`Win32_Media_Audio_Endpoints`).

### 6.2. M2.2: brilho (risco baixo)

- **Id:** `brilho`
- **Descrição:** "Define o brilho da tela interna do notebook, ou do monitor externo que responde por DDC/CI, de 0 a 100."
- **Schema de entrada:** `{ "nivel": integer 0 a 100 }`, obrigatório, sem campo extra.
- **Risco:** `Low`.
- **API:**
  - Tela interna: WMI em `root\wmi`, com `WmiMonitorBrightnessMethods.WmiSetBrightness(timeout, nivel)` e leitura de `WmiMonitorBrightness.CurrentBrightness`. A conexão segue o padrão de `vitals.rs`.
  - Monitor externo: `EnumDisplayMonitors` e `GetPhysicalMonitorsFromHMONITOR` (dxva2), com `SetVCPFeature` e `GetVCPFeatureAndVCPFeatureReply` no código VCP 0x10. `DestroyPhysicalMonitors` ao final.
  - Ordem: tela interna primeiro. Se não houver, os monitores externos que respondem.
- **Por que texto não vira comando:** `nivel` é inteiro validado e segue como número. Não há texto de pedido em nenhuma chamada.
- **`check_availability`:** disponível com `internal_display` (há instância de `WmiMonitorBrightness`) ou com `ddc_display` (um monitor respondeu à leitura do VCP 0x10 na sondagem). Sem nenhum dos dois, `Unavailable("nenhuma tela com brilho controlável")`. Em um desktop sem tela interna, a ferramenta fica oculta. Se o pedido chegar mesmo assim, `execute` responde `PrerequisiteMissing` com o mesmo motivo, sem inventar valor.
- **Monitor externo que não responde no clique:** `PrerequisiteMissing("este monitor não responde por DDC/CI; brilho indisponível neste monitor")`.
- **`verify`:** relê o brilho (WMI ou VCP 0x10) e espera até 1,5 s, em passos de 250 ms, porque alguns drivers atualizam com atraso. Diferença maior que 2 vira `VerificationFailed`. Leitura impossível também vira `VerificationFailed("o brilho não pôde ser confirmado")`. Nunca há sucesso sem leitura.
- **Saída:** `{ "nivel" (pedido), "observado" (lido), "fonte": "tela_interna" ou "monitor_externo" }`. A fonte é uma categoria, não o nome do monitor.
- **Registro:** vai "brilho em 60% (tela interna)" ou "brilho em 60% (monitor externo)". Não vai modelo, fabricante nem número de série.
- **Testes sem hardware:**
  - schema: 0 e 100 aceitos; 101 e −1 recusados;
  - o plano escolhe a tela interna antes do externo;
  - backend falso sem tela: `PrerequisiteMissing`, e a saída não tem `nivel`;
  - monitor sem resposta: mensagem de indisponível;
  - leitura divergente e leitura ausente: `VerificationFailed`.
- **Teste manual no chat:** "brilho 60%" no notebook relata 60. Em desktop sem tela interna, o modelo não oferece a ferramenta ou a resposta diz que não há tela controlável. Com monitor externo, anotar só se ele respondeu por DDC/CI (sim ou não) e o tipo de conexão, sem modelo.
- **Arquivos novos:** `mochi/tools/brightness.rs`.
- **Arquivos alterados:** `mochi/host.rs` (`internal_display`, `ddc_display`), `mochi/tools/mod.rs`, `windows/src-tauri/Cargo.toml` (`Win32_Devices_Display`).

### 6.3. M2.3: listar e abrir aplicativos (risco baixo)

- **Id:** `apps`
- **Descrição:** "Lista os aplicativos do Menu Iniciar ou abre um deles pelo nome."
- **Schema de entrada:**
  - `acao`: enum `["listar", "abrir"]`, obrigatório;
  - `nome`: string de 1 a 64 caracteres, obrigatório só com `abrir`.

  `dry_run` recusa `nome` com `listar` e recusa `abrir` sem `nome`.
- **Risco:** `Low`.
- **API:**
  - Listagem: varredura de `.lnk` em `%APPDATA%\Microsoft\Windows\Start Menu\Programs` e `%ProgramData%\Microsoft\Windows\Start Menu\Programs`, extraída de `voice/mod.rs` para `start_menu_shortcuts()` (`pub(crate)`).
  - Abertura: `ShellExecuteExW` com verbo `open` sobre o caminho do `.lnk` que a varredura encontrou (`shell_open_uri`, que passa a `pub(crate)`). Nenhuma linha de comando é montada.
- **Por que texto não vira comando:** `nome` é normalizado por `voice::wake::normalize` (minúsculas, sem acento, só letras e dígitos) e serve apenas para comparar com o índice. `;` e `&` viram espaço e não casam com nada. Caminho nunca vem do pedido.
- **Ambiguidade:** a pontuação segue a escala de `find_app` (exato, depois prefixo, depois contém). Se duas entradas empatam no melhor resultado, nada abre. A resposta lista até 5 nomes para o usuário escolher no chat. Um nome mais preciso abre.
- **Aliases:** "VS Code" casa com "Visual Studio Code" por uma lista curta de aliases em `tools/apps.rs` (seção 9).
- **`check_availability`:** disponível com `start_menu` (há pelo menos uma pasta do Menu Iniciar). Sem ela, `Unavailable`.
- **`verify`:** em `abrir`, o processo do alvo tem de aparecer na tabela de processos em até 5 s. O executável é lido do atalho por `IShellLinkW::GetPath`. Se a `execute` não vê o processo, `VerificationFailed("o aplicativo não apareceu")`. Processo que já estava aberto conta como aberto.
- **Saída:**
  - `listar`: `{ "apps": [até 200 nomes, sanitizados a 64 caracteres] }`;
  - `abrir`: `{ "aberto": nome do aplicativo escolhido, sanitizado }`.
- **Registro:** vai "abrir aplicativo: <nome, até 60 caracteres>" e "listagem de aplicativos (n itens)". Não vai o caminho do atalho (contém o nome da conta em `%APPDATA%`) nem o texto digitado quando não casou.
- **Testes sem hardware:**
  - normalização: `"calc; del x & notepad"` não casa com nada;
  - índice falso (lista sintética de nome e caminho): o exato vence; empate devolve escolhas e não abre nada (contador do lançador falso igual a 0); nome ausente vira `NotFound`;
  - nome com `<` e quebra de linha num atalho sai sanitizado;
  - lançador falso não é chamado com `;` nem com `&`.
- **Teste manual no chat:** "abre o VS Code" abre o Visual Studio Code e relata "aberto". "abre o editor" pede um nome mais preciso. "abre calc; del &" responde "não encontrado" e nada abre. "lista os aplicativos" mostra a lista.
- **Arquivos novos:** `mochi/tools/apps.rs`.
- **Arquivos alterados:** `voice/mod.rs` (extrai `start_menu_shortcuts`, comportamento de voz igual e testes de voz passando), `platform/windows.rs` (`shell_open_uri` e `process_table` passam a `pub(crate)`), `mochi/host.rs` (`start_menu`), `mochi/tools/mod.rs`.

### 6.4. M2.4: janelas (risco baixo)

- **Id:** `janelas`
- **Descrição:** "Lista janelas abertas; move, redimensiona, minimiza ou põe duas lado a lado; desfaz a última organização."
- **Schema de entrada:**
  - `acao`: enum `["listar", "mover", "redimensionar", "minimizar", "lado_a_lado", "desfazer"]`, obrigatório;
  - `alvo`: inteiro de 0 a 100000, id devolvido por `listar`;
  - `alvos`: array de 2 inteiros de 0 a 100000 (`minItems` e `maxItems` em 2);
  - `x`, `y`: inteiros de −10000 a 10000;
  - `largura`, `altura`: inteiros de 100 a 10000.

  `dry_run` exige os campos de cada ação e recusa campo de outra ação. Por exemplo, `mover` exige `alvo`, `x` e `y`.
- **Risco:** `Low`. A ação é reversível (`desfazer`) e local.
- **API:**
  - Enumeração: `top_windows` (EnumWindows) com `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`, para o retângulo visível sem sombra.
  - Monitor: `MonitorFromWindow` e `GetMonitorInfoW` (`rcWork`).
  - Posição: `SetWindowPos`. Minimizar: `ShowWindow(SW_MINIMIZE)`.
  - Estado anterior: `GetWindowPlacement` e `SetWindowPlacement`, que também guardam maximizada e minimizada.
- **Lado a lado:** divide o `rcWork` do monitor da primeira janela em duas metades. A primeira de `alvos` fica à esquerda e a segunda à direita. Largura ímpar: a metade da direita recebe o pixel extra. Janela maximizada é restaurada antes de encaixar.
- **Desfazer:** a última organização fica em memória (estado estático no `tools/arrange.rs`), com o `WINDOWPLACEMENT` de cada janela tocada. Some quando o Lumen fecha. Antes de restaurar, confere `IsWindow` e se o pid da janela continua o mesmo. Se não, responde "janela não existe mais" e não toca nas outras.
- **Escala e monitores:** os cálculos usam pixels físicos do `rcWork` do monitor onde a janela está. Janela que cruza dois monitores usa o monitor com maior área de interseção.
- **Janela de administrador:** `SetWindowPos` falha com acesso negado (UIPI). Esse erro vira "esta janela é de administrador; o Lumen não pode movê-la".
- **Escolha de janelas:** o Lumen não escolhe as janelas sozinho. "organize as janelas para programar" faz o modelo listar, e, se houver mais de duas candidatas, perguntar no chat quais duas. A ferramenta não adivinha por nome de processo. "Para programar" não é um layout próprio.
- **`check_availability`:** disponível com `display` (há monitor e janelas de usuário). Sem ele, `Unavailable`.
- **`verify`:** relê o retângulo de cada janela movida, com tolerância de 2 px. Janela minimizada tem de dar `IsIconic` verdadeiro. `desfazer` tem de restaurar o placement salvo.
- **Saída:**
  - `listar`: `[{ "alvo", "titulo" (sanitizado, 60 caracteres), "monitor" (número de 1 a N), "minimizada" }]`, no máximo 50 itens, encapsulado como texto externo;
  - demais ações: `{ "acao", "alvos", "resultado": "ok" }`.
- **Registro:** vai "janelas organizadas lado a lado", "organização desfeita" ou "janela minimizada". Não vai o título nem o nome do processo.
- **Testes sem hardware:**
  - matemática pura do encaixe: metades, largura ímpar, origem negativa e monitor com origem deslocada;
  - `desfazer` com backend falso: janela que sumiu vira erro sem tocar nas outras;
  - schema: `alvos` com 1 ou 3 itens recusado; `alvo` fora da faixa recusado;
  - erro de acesso negado vira a mensagem de administrador;
  - título com `\n` e um `<mochi_tool_call>` falso sai neutralizado.
- **Teste manual no chat:** "organize as janelas para programar", depois de escolher duas, deixa as duas lado a lado. "desfaz a organização das janelas" restaura. "minimiza a janela X" esconde a janela. Com um só monitor, o teste de dois monitores fica registrado como HARDWARE INDISPONÍVEL. Janela de administrador mostra o erro claro.
- **Arquivos novos:** `mochi/tools/arrange.rs`.
- **Arquivos alterados:** `platform/windows.rs` (`top_windows` passa a `pub(crate)`, com helper de retângulo), `mochi/host.rs` (`display`), `mochi/tools/mod.rs`. Não exige feature nova além das já ligadas, salvo `Win32_UI_HiDpi` se a decisão for usá-la.

### 6.5. M2.5: mídia (risco baixo)

- **Id:** `midia`
- **Descrição:** "Toca, pausa, avança ou volta a faixa do player de mídia ativo do Windows."
- **Schema de entrada:** `acao`, enum `["tocar", "pausar", "proxima", "anterior"]`, obrigatório, sem campo extra.
- **Risco:** `Low`.
- **API:** `GlobalSystemMediaTransportControlsSessionManager::RequestAsync`, depois `GetCurrentSession()`, depois `TryPlayAsync`, `TryPauseAsync`, `TrySkipNextAsync` e `TrySkipPreviousAsync`. Para verificar, `GetPlaybackInfo` e `TryGetMediaPropertiesAsync`. COM inicializado como em `spotify.rs::win::com`.
  - A pílula usa `TryTogglePlayPauseAsync`. A M2 usa `TryPlayAsync` e `TryPauseAsync`, para que o comando não inverta um estado que já era o pedido.
- **Player ativo:** é a sessão que o Windows marca como atual (`GetCurrentSession`). Nunca se percorrem todas as sessões, então nenhum outro player é tocado. `find_session` de `spotify.rs` não é usado, porque só acha Spotify e YouTube Music.
- **Por que texto não vira comando:** `acao` é enum. Nenhum texto do pedido chega ao sistema.
- **`check_availability`:** disponível com `media_session` (existe sessão atual). Sem ela, `Unavailable("nenhum player de mídia ativo")`.
- **Sem player em `execute`:** `PrerequisiteMissing("nenhum player de mídia ativo; nada foi aberto")`. A ferramenta não chama `spotify_open` nem `open_url`, então não abre nada.
- **`verify`:** `tocar` espera status `Playing`; `pausar` espera `Paused`; `proxima` e `anterior` esperam que o id da faixa mude. O id é um hash de título, artista e álbum, no mesmo estilo do id por hash de `spotify.rs`. O hash é só para comparar e não sai. Espera até 1,5 s. Em `proxima` e `anterior` sem mudança, `VerificationFailed("a faixa não mudou")`. Uma faixa só, por exemplo, responde isso honestamente.
- **Saída:** `{ "acao", "status": "tocando" ou "pausado", "faixa_mudou": booleano }`.
- **Registro:** vai "tocar, pausar, próxima ou anterior no player ativo". Não vai título, artista nem nome do aplicativo.
- **Testes sem hardware:**
  - mapa de `acao` para operação e status esperado;
  - `verify` pura, com faixa igual e com faixa diferente;
  - backend falso sem sessão: `PrerequisiteMissing`, com zero chamadas;
  - duas sessões falsas, com a atual sendo a B: só B recebe chamadas;
  - `TryPlayAsync` recusado vira `ExecutionFailed`.
- **Teste manual no chat:** com Spotify tocando, "pausa a música" pausa e relata; "próxima música" muda a faixa. Com um vídeo tocando no navegador como player atual, só ele responde. Sem nada tocando, "toca a música" responde "nenhum player ativo" e nada abre.
- **Arquivos novos:** `mochi/tools/media.rs`.
- **Arquivos alterados:** `mochi/host.rs` (`media_session`), `mochi/tools/mod.rs`.
- **Não alterados:** `spotify.rs`. A pílula de música não muda.

### 6.6. M2.6: energia (risco médio e alto)

- **Ids:** `bloquear_tela` (`Medium`), `suspender`, `reiniciar` e `desligar` (`High`).
- **Schema de entrada:** `{ "type": "object", "properties": {}, "additionalProperties": false }` para os quatro.
- **Descrição (pt-BR):** "Bloqueia a tela." / "Suspende o computador." / "Reinicia o computador." / "Desliga o computador."
- **Texto do `dry_run` (vai ao cartão):**
  - `bloquear_tela`: "Bloquear a tela agora."
  - `suspender`: "Suspender o computador agora. O Lumen não sabe se há trabalho não salvo, e apps não são avisados."
  - `reiniciar` e `desligar`: "Reiniciar (ou desligar) o computador agora. Apps podem pedir para salvar; se algum recusar, a ação é cancelada. O Lumen não sabe se há trabalho não salvo."
- **API:**
  - `bloquear_tela`: `LockWorkStation` (`Win32_System_Shutdown`).
  - `suspender`: `SetSuspendState(false, false, false)` (`Win32_System_Power`).
  - `reiniciar` e `desligar`: habilitar `SeShutdownPrivilege` no token do processo (`AdjustTokenPrivileges`, no padrão de `current_user_sid`), depois `ExitWindowsEx` com `EWX_RESTART` ou `EWX_SHUTDOWN`, sem `EWX_FORCE`. Assim os apps podem pedir para salvar.
  - Nada de `shutdown.exe`, `rundll32` nem PowerShell.
- **Por que texto não vira comando:** não há argumento. A ferramenta é escolhida pelo id.
- **`check_availability`:**
  - `bloquear_tela`: sempre disponível;
  - `suspender`: disponível com `sleep_supported` (`GetPwrCapabilities`, `SystemS3`). Sem ela, `Unavailable("suspensão desligada neste computador")`;
  - `reiniciar` e `desligar`: disponíveis com `shutdown_privilege`. Sem ela, `Unavailable`.
- **`verify`:** o que é possível ler é a resposta da API. `LockWorkStation`, `SetSuspendState` ou `ExitWindowsEx` com retorno verdadeiro viram `aceito: true`. Falha da API vira `ExecutionFailed` com o código Win32 numérico, sem texto de tela.
  - Limite registrado: depois da suspensão ou do desligamento, o Lumen não lê resultado. O relato diz "pedido aceito" ou "desligamento iniciado", nunca "feito".
- **Saída:** `{ "acao", "aceito": true }`.
- **Clique:** `bloquear_tela` é médio, então pode ser "sempre permitido" e a permissão é revogável em Permissões. Os três altos pedem clique toda vez e nunca são lembrados (regra da M1, aplicada pelo `PermissionEngine`).
- **Negar ou timeout:** o pipeline da M1 garante que nada chama o sistema. Os testes provam isso com `DefaultDenyRequester` e `FixedRequester`.
- **Registro:** vai ação, decisão (`AllowedOnce`, `Denied`, `TimedOut`, `AllowedAlways`) e resultado ("pedido aceito" ou "negado"). Não vai mais nada.
- **Testes sem hardware:**
  - contador do backend falso: zero em negar, timeout e pausa; um quando aceito;
  - schema sem argumento;
  - `dry_run` dos três altos contém o aviso de trabalho não salvo;
  - sem privilégio ou sem suspensão, `Unavailable`;
  - API falsa com retorno falso vira `ExecutionFailed`.
- **Teste manual (dono, em notebook de teste, só com trabalho salvo):**
  - "bloqueia a tela": negar não faz nada; aceitar bloqueia a tela;
  - "suspende o computador": negar não faz nada; aceitar suspende, e retomar cobre o M2.7;
  - "reinicia" e "desliga": negar não faz nada; deixar expirar não faz nada. A execução real só ocorre se o dono pedir, com tudo salvo.
- **Arquivos novos:** `mochi/tools/power.rs`.
- **Arquivos alterados:** `mochi/host.rs` (`sleep_supported`, `shutdown_privilege`), `mochi/tools/mod.rs`, `windows/src-tauri/Cargo.toml` (`Win32_System_Shutdown`).

### 6.7. M2.7: fechamento

- **Dez comandos de rotina** (proposta, o dono confirma): "volume 20%", "tira o som", "brilho 60%", "abre o VS Code", "abre o Bloco de Notas", "lista os aplicativos", "organize as janelas para programar", "desfaz a organização das janelas", "pausa a música", "próxima música". "Bloqueia a tela" fica de fora desta lista, porque bloqueia o computador a cada execução. Cada um roda três vezes pelo chat, com resultado registrado numa tabela de evidência: data, versão, resultado, sem dados pessoais.
- **Testes de borda:**
  - retomar da suspensão: depois de suspender e retomar, "volume 30%" e "pausa a música" continuam funcionando;
  - aplicativo ausente: "abre um app inexistente" responde "não encontrado", com motivo;
  - dispositivo de áudio trocado: trocar a saída padrão no Windows sem reiniciar o Lumen, e "volume 30%" age na saída nova. Por isso o endpoint não é guardado em cache.
- **Documentação:** `windows/README.md` (ferramentas da M2, limites e o que ficou de fora), `docs/STATE-FORK.md` e a linha da M2 em `docs/ROADMAP-FORK.md`.
- **Checkpoint M2:** 10 comandos com sucesso repetível (30 execuções, ou falhas registradas com motivo). Toda falha aparece como falha.
- **Arquivos alterados:** `windows/README.md`, `docs/STATE-FORK.md`, `docs/ROADMAP-FORK.md`, `docs/roadmap/m2-windows-nativo.md` (marcações e evidência).

## 7. Testes e validação

| Teste | Sem hardware (sintético) | Com hardware (manual) |
|---|---|---|
| Schema e validação | sim | não |
| Plano puro (argumentos para operação) | sim | não |
| `verify` e `audit_summary` puras | sim | não |
| Backend falso (sem dispositivo, divergência, sem sessão, negar) | sim | não |
| Sanitização de texto do sistema | sim | não |
| Chamada real ao sistema (volume, brilho, janelas, mídia, energia) | não (proibido em `cfg(test)`) | sim, pelo dono |
| Frases do checkpoint no chat | não | sim, pelo dono |

Validação automatizada, por subetapa (ver o plano): `cargo test --lib` em `windows/src-tauri/`, com o contorno de link descrito no plano. `npx tsc --noEmit` e `npm test` em `windows/` só se algum TypeScript mudar (a M2 não prevê mudança de TypeScript). O build Linux é conferido pela CI, porque o `cfg` fica do lado do Windows.

## 8. Riscos

| Risco | Efeito | Mitigação |
|---|---|---|
| COM não é `Send` | erro de compilação ou thread errada | `spawn_blocking`; objetos COM criados e usados dentro da thread (seção 5.5) |
| Energia sem saber de trabalho não salvo | perda de dados | aviso no cartão; `ExitWindowsEx` sem `EWX_FORCE`; teste real só com tudo salvo |
| Verificação de energia limitada | sucesso falso | relato diz "pedido aceito", não "feito" (seção 6.6) |
| Brilho ignorado por driver | valor mostrado não bate com a tela | `verify` com leitura de volta e espera de 1,5 s |
| DDC/CI lento | listagem lenta | cache curto da sondagem (seção 9) |
| Janela de administrador | falha silenciosa | erro claro de acesso negado (seção 6.4) |
| Nome de aplicativo ambíguo ou malicioso | abrir o programa errado, ou instrução escondida no nome | pedido de escolha; sanitização e encapsulamento (seção 5.7) |
| Ferramentas M2 também no MCP | Claude Code ou Codex veem as ferramentas | mesmas regras de clique e de registro, sem atalho |
| Player ativo não é o esperado | comando vai ao player errado | mensagem diz "player ativo do Windows"; teste com dois players |
| Vazamento de título ou nome pelo registro | dado pessoal no `mochi-audit.json` | `AuditLog` só recebe o `audit_summary` da ferramenta; teste de vazamento por ferramenta |
| `cargo test --lib` falha neste Windows | sem validação automática | contorno documentado no plano; não mexer no `Cargo.toml` por causa disso |
| Compilação no Linux | build quebra | `cfg(windows)` em todo código de sistema e versão de stub |
| Crate `windows` com mais features | compilação mais lenta | nenhuma crate nova; cada feature entra com a subetapa que a usa |
| Dois monitores não testados | comportamento não validado | registro HARDWARE INDISPONÍVEL no checkpoint M2.4 |
| Privilégio de desligamento negado pela política | ação indisponível | `Unavailable` com motivo, sem tentativa |

## 9. Pontos em aberto (decisão do dono antes do código)

1. **Apps da Microsoft Store.** Recomendação: não entram nesta fase. Exigiria enumerar `shell:AppsFolder`. Ficam só os atalhos do Menu Iniciar.
2. **Aliases de nome** ("VS Code" para "Visual Studio Code"). Recomendação: lista curta fixa de até 10 pares em `tools/apps.rs`.
3. **Pedido de escolha.** Recomendação: só no chat, sem cartão novo, para não criar tela nesta fase.
4. **Cache da sondagem DDC/CI.** Recomendação: até 30 s, para não atrasar a listagem.
5. **Significado de "para programar".** Recomendação: `lado_a_lado` com duas janelas escolhidas pelo usuário. A alternativa, atalho por nome de processo, adivinha e não é recomendada.
6. **Os dez comandos do M2.7** (lista da seção 6.7).
7. **Arquivar a M2 só depois do relato do dono** sobre o checkpoint real, como manda o AGENTS. Os checkpoints reais ficam abertos até lá.
8. **Desfazer em memória.** Recomendação: sim, sem arquivo. A organização desfeita some ao fechar o Lumen.
9. **Verificação de energia.** Proposta: "pedido aceito" é o máximo que o Lumen confirma. O dono precisa saber disso antes de aprovar.

## 10. Rollback

- **Por subetapa:** um commit por subetapa. `git revert <commit>` remove a subetapa sem afetar as demais. Não há migração e nenhum arquivo persistente novo. O desfazer das janelas fica em memória, e o registro (`mochi-audit.json`) segue o formato da M1.
- **Desabilitar sem reverter:** tirar a chamada de `tools::register_m2` em `with_default_tools`. As ferramentas deixam de existir para o cérebro e para o MCP.
- **Efeitos no sistema não são desfeitos pelo rollback de código.** Volume, mudo e brilho ficam como estão, e o usuário ajusta. O desfazer das janelas vale enquanto o Lumen roda. Energia não tem desfazer.
- **Limpeza:** nenhum arquivo novo em disco. O registro pode ser limpo pelo botão da M1.
- **Não tocados:** `spotify.rs`, `flow/hush.rs`, `settings.json` e `mochi-permissions.json`.

## 11. Critérios de aceite

Os checkpoints de [m2-windows-nativo.md](../../roadmap/m2-windows-nativo.md) são o critério. Resumo:

- **M2.1:** "volume 20%" e "tira o som" funcionam e relatam o valor final; valor fora da faixa é recusado; sem dispositivo, erro claro.
- **M2.2:** brilho muda onde é suportado; o hardware testado fica anotado; sem tela controlável, nenhum valor é inventado.
- **M2.3:** "abre o VS Code" abre o aplicativo certo; nome ambíguo pede escolha; ausente vira erro; `;` e `&` não executam nada.
- **M2.4:** "organize as janelas para programar" arruma e "desfaz" restaura; janela de administrador vira erro claro; dois monitores registrados como HARDWARE INDISPONÍVEL se não houver o hardware.
- **M2.5:** os comandos agem no player ativo e em nenhum outro; sem player, responde isso e não abre nada.
- **M2.6:** nenhuma ação de energia ocorre sem clique; negar e expirar não fazem nada; o registro mostra a decisão.
- **M2.7:** 10 comandos com sucesso repetível; retomada da suspensão, aplicativo ausente e troca de áudio testados.
