# Plano de Execução — Fase M2: Controle nativo do Windows

**Spec de referência:** [2026-11-17-m2-windows-nativo.md](../specs/2026-11-17-m2-windows-nativo.md)  
**Roadmap:** [m2-windows-nativo.md](../../roadmap/m2-windows-nativo.md)  
**Regras:** [AGENTS.md](../../../AGENTS.md) (seções "Regras das fases M" e "Como executar uma etapa")  
**Branch:** `feat/m2-windows-nativo`

## Antes de qualquer código

1. **Árvore limpa.** `git status` sem mudanças. Se houver mudança de outra etapa, ela vai para o próprio commit ou sai da árvore antes de começar.
2. **Sincronizar com o upstream.** `git fetch upstream` e `git merge upstream/main`. Conflito se resolve num commit próprio, sem misturar com a feature.
3. **Criar a branch** `feat/m2-windows-nativo` a partir da `main`. Trabalhar nela ou numa worktree.
4. **Fechar as decisões da seção 9 da spec.** Pelo menos as de números 1 a 5 precisam de resposta (ou da recomendação aceita) antes da M2.1.
5. **Uma subetapa por vez.** Não começar a seguinte sem o commit da anterior e a validação sintética verde.
6. **Dados.** Nada de dados pessoais, caminho de usuário real, token ou nome de cliente em código, docs, fixtures, logs ou mensagens de commit. Usar exemplos sintéticos.
7. **Commits em pt-BR**, no padrão do repositório (`feat(m2.1): ...`), com as linhas de atribuição que o ambiente exigir.

## Validação (todas as subetapas)

| O que mudou | Comando |
|---|---|
| Rust do app | em `windows/src-tauri/`: `cargo test --lib` (ver contorno abaixo) |
| TypeScript | em `windows/`: `npx tsc --noEmit` e `npm test`, só se algum `.ts` mudar |
| Build de interface | em `windows/`: `npm run build`, só se a interface mudar |
| Teste manual | a lista de frases do checkpoint da subetapa, no chat. Nunca substitui o sintético |

**Contorno para `cargo test --lib` neste Windows.** Hoje o comando padrão falha com `STATUS_ENTRYPOINT_NOT_FOUND`. O que funciona:

```text
cd windows/src-tauri
cargo rustc --lib --profile test -- -C link-arg=<caminho do resource.lib gerado em target/debug/build/coucou-*/out/>
```

Depois, executar o `.exe` de teste gerado em `target/debug/deps/` (algo como `coucou_lib-<hash>.exe`). Registrar no commit que o resultado veio desse contorno.

**Regra de checkpoint.** Cada subetapa fecha com o sintético verde e a frase de teste manual registrada. O checkpoint real fica aberto até o relato do dono. Nunca marcar como concluído só porque compila.

## Regra de segurança para todos os testes

Em `cfg(test)`, o backend real não é alcançável (spec 5.6). Antes de cada commit, conferir com busca que nenhum teste chama as APIs reais de energia, volume, brilho, janela ou `ShellExecuteExW`. Qualquer chamada real em teste é bug e bloqueia o commit.

---

## Passo 0 — Preparação (sem commit de produto)

- Conferir as features do `windows` crate em `windows/src-tauri/Cargo.toml` contra a lista da spec, seção 4.
- Conferir os pontos que o contexto vai usar: `chat_send_with_mochi` em `mochi/chat_bridge.rs` e `ToolContext::default()` em `mochi/mcp_server.rs` (`tools/list` e `tools/call`).
- Confirmar que `mochi/mod.rs` declara `pub mod` dos módulos existentes, para acrescentar `host` e `tools`.
- Nenhum arquivo muda nesta etapa.

---

## Subetapa M2.1 — Volume e mudo

**Arquivos novos:** `windows/src-tauri/src/mochi/host.rs`, `windows/src-tauri/src/mochi/tools/mod.rs`, `windows/src-tauri/src/mochi/tools/audio.rs`.  
**Arquivos alterados:** `windows/src-tauri/src/mochi/mod.rs`, `windows/src-tauri/src/mochi/registry.rs`, `windows/src-tauri/src/mochi/chat_bridge.rs`, `windows/src-tauri/src/mochi/mcp_server.rs`, `windows/src-tauri/Cargo.toml`.

**Passos:**
1. Em `Cargo.toml`, acrescentar `Win32_Media_Audio_Endpoints` à lista do bloco `cfg(windows)`.
2. Criar `host.rs` com `context()`: devolve um `ToolContext` com a capacidade `audio_endpoint` quando há endpoint de saída padrão. Nesta etapa só essa capacidade existe.
3. Criar `tools/mod.rs` com `register_m2(&mut ToolRegistry)`. Chamar essa função em `with_default_tools()` (`registry.rs`).
4. Criar `tools/audio.rs`:
   - plano puro: argumentos viram operação (`ler`, `definir(nivel)`, `silenciar(bool)`); `dry_run` recusa combinações inválidas;
   - trait pequeno para endpoint de áudio, com a implementação real em `#[cfg(windows)]` e a falsa em `cfg(test)`;
   - `run_with(backend, plano)`: age, relê e devolve `{ pedido, observado }`;
   - `execute` chama `run_with` dentro de `spawn_blocking`, com `CoInitializeEx` na thread;
   - `verify` pura (tolerância de 1 no volume, igualdade no mudo);
   - `audit_summary` com as frases da spec, sem nome de dispositivo.
5. Trocar `ToolContext::default()` por `host::context()` em `mcp_server.rs` (`tools/list` e `tools/call`) e no ponto de montagem do contexto em `chat_bridge.rs`.
6. Escrever os testes (abaixo) e rodar a validação.

**Testes:**
- schema: 0 e 100 aceitos; −1, 101, 20,5, texto e campo extra recusados;
- `dry_run`: `definir_volume` sem `nivel` recusado; `nivel` com `silenciar` recusado;
- `verify` pura: dentro e fora da tolerância; mudo igual e diferente;
- `run_with` com backend falso: sem endpoint vira `PrerequisiteMissing`; endpoint que não aplica o valor vira `VerificationFailed`; caminho correto vira sucesso;
- registro: o resumo não contém nome de dispositivo.

**Validação:** `cargo test --lib` (contorno). Busca de segurança: nenhuma chamada real de áudio em teste.

**Teste manual (checkpoint M2.1):** "volume 20%" relata "volume em 20%"; "tira o som" relata "som silenciado"; "volume 101%" é recusado com o volume inalterado.

**Commit:** `feat(m2.1): volume e mudo (ferramenta audio)`

---

## Subetapa M2.2 — Brilho

**Arquivos novos:** `windows/src-tauri/src/mochi/tools/brightness.rs`.  
**Arquivos alterados:** `windows/src-tauri/src/mochi/host.rs` (capacidades `internal_display` e `ddc_display`), `windows/src-tauri/src/mochi/tools/mod.rs` (registro), `windows/src-tauri/Cargo.toml` (`Win32_Devices_Display`).

**Passos:**
1. Acrescentar `Win32_Devices_Display` ao `Cargo.toml`.
2. Em `host.rs`, sondar `internal_display` (instância de `WmiMonitorBrightness` em `root\wmi`) e `ddc_display` (resposta ao VCP 0x10). Aplicar o cache curto de 30 s da decisão 4.
3. Criar `tools/brightness.rs`:
   - plano puro: `nivel` vira operação; a ordem é tela interna, depois monitores externos;
   - trait para tela interna (WMI) e monitor externo (DDC/CI), com implementações reais em `cfg(windows)` e falsas em `cfg(test)`;
   - `execute` tenta a tela interna e, se não houver, os externos, dentro de `spawn_blocking`;
   - `verify` pura com tolerância de 2; a espera de 1,5 s em passos de 250 ms fica em `execute`, porque é leitura do sistema;
   - saída com `fonte` e sem nome de monitor.
4. Conexão WMI seguindo o padrão de `vitals.rs` (`mod os`, `thermal_zones`), com o namespace `root\wmi`.
5. Registrar `brilho` em `register_m2`.

**Testes:**
- schema: 0 e 100 aceitos; 101 e −1 recusados;
- o plano escolhe a tela interna antes do externo;
- sem tela: `PrerequisiteMissing`, e a saída não tem `nivel`;
- monitor sem resposta: mensagem de indisponível;
- leitura divergente e leitura ausente: `VerificationFailed`.

**Validação:** `cargo test --lib` (contorno).

**Teste manual (checkpoint M2.2):** "brilho 60%" no notebook relata 60. Em desktop sem tela interna, a ferramenta não aparece ou a resposta diz que não há tela controlável. Com monitor externo, anotar só se respondeu por DDC/CI e o tipo de conexão, sem modelo nem número de série.

**Commit:** `feat(m2.2): brilho da tela interna e de monitores DDC/CI`

---

## Subetapa M2.3 — Listar e abrir aplicativos

**Arquivos novos:** `windows/src-tauri/src/mochi/tools/apps.rs`.  
**Arquivos alterados:** `windows/src-tauri/src/voice/mod.rs` (extrai `start_menu_shortcuts`), `windows/src-tauri/src/platform/windows.rs` (`shell_open_uri` e `process_table` passam a `pub(crate)`), `windows/src-tauri/src/mochi/host.rs` (`start_menu`), `windows/src-tauri/src/mochi/tools/mod.rs`.

**Passos:**
1. Em `voice/mod.rs`, extrair a varredura de `find_app` para `pub(crate) fn start_menu_shortcuts()`. `find_app` e `open_app` passam a usá-la e continuam com o mesmo comportamento. Rodar os testes de voz antes e depois.
2. Tornar `shell_open_uri` e `process_table` `pub(crate)`.
3. Em `host.rs`, sondar `start_menu`.
4. Criar `tools/apps.rs`:
   - normalização por `voice::wake::normalize`;
   - pontuação em três níveis (exato, prefixo, contém), com empate pedindo escolha;
   - lista de aliases curta (decisão 2);
   - leitura do destino do `.lnk` por `IShellLinkW::GetPath`;
   - `abrir` por `shell_open_uri` no caminho indexado, nunca no texto do pedido;
   - `verify` pura sobre `{ pedido, observado }`; a espera de até 5 s pelo processo fica em `execute`;
   - sanitização de nomes com `sanitize_external_text`, 64 caracteres.
5. Registrar `apps` em `register_m2`.

**Testes:**
- normalização: `"calc; del x & notepad"` não casa com nada;
- índice falso (lista sintética de nome e caminho): o exato vence; empate devolve escolhas e não abre nada (contador do lançador falso igual a 0); ausente vira `NotFound`;
- nome com `<` e quebra de linha sai sanitizado;
- lançador falso não é chamado com `;` nem com `&`;
- testes de voz continuam passando depois da extração.

**Validação:** `cargo test --lib` (contorno).

**Teste manual (checkpoint M2.3):** "abre o VS Code" abre o Visual Studio Code e relata "aberto". "abre o editor" pede um nome mais preciso. "abre calc; del &" responde "não encontrado" e nada abre. "lista os aplicativos" mostra a lista.

**Commit:** `feat(m2.3): listar e abrir aplicativos do Menu Iniciar`

---

## Subetapa M2.4 — Janelas

**Arquivos novos:** `windows/src-tauri/src/mochi/tools/arrange.rs` (e, se ficar grande, `tools/layout.rs` com a matemática pura).  
**Arquivos alterados:** `windows/src-tauri/src/platform/windows.rs` (`top_windows` passa a `pub(crate)`, com helper de retângulo via DWM), `windows/src-tauri/src/mochi/host.rs` (`display`), `windows/src-tauri/src/mochi/tools/mod.rs`. Só mexer em `Cargo.toml` se a decisão do `Win32_UI_HiDpi` for usá-la.

**Passos:**
1. Escrever primeiro a matemática pura do encaixe (metades, largura ímpar, origem negativa, monitor deslocado) e seus testes.
2. Criar `arrange.rs`:
   - plano puro para cada ação (`listar`, `mover`, `redimensionar`, `minimizar`, `lado_a_lado`, `desfazer`), com `dry_run` exigindo e recusando campos;
   - estado de desfazer em memória (`Mutex` estático), guardando o `WINDOWPLACEMENT` de cada janela tocada;
   - `desfazer` confere `IsWindow` e o pid antes de tocar em qualquer janela;
   - erro de acesso negado mapeado para a mensagem de administrador;
   - `listar` devolve até 50 itens, com título sanitizado e encapsulado.
3. Em `host.rs`, sondar `display`.
4. Registrar `janelas` em `register_m2`.

**Testes:**
- matemática pura: metades, largura ímpar, origem negativa, monitor com origem deslocada;
- `desfazer` com backend falso: janela que sumiu vira erro sem tocar nas outras;
- schema: `alvos` com 1 ou 3 itens recusado; `alvo` fora da faixa recusado;
- erro de acesso negado vira a mensagem de administrador;
- título com `\n` e um `<mochi_tool_call>` falso sai neutralizado.

**Validação:** `cargo test --lib` (contorno).

**Teste manual (checkpoint M2.4):** "organize as janelas para programar", depois de escolher duas, deixa as duas lado a lado. "desfaz a organização das janelas" restaura. "minimiza a janela X" esconde a janela. Com um monitor só, registrar **HARDWARE INDISPONÍVEL** para o teste de dois monitores. Janela de administrador mostra o erro claro.

**Commit:** `feat(m2.4): listar, mover e encaixar janelas, com desfazer`

---

## Subetapa M2.5 — Mídia

**Arquivos novos:** `windows/src-tauri/src/mochi/tools/media.rs`.  
**Arquivos alterados:** `windows/src-tauri/src/mochi/host.rs` (`media_session`), `windows/src-tauri/src/mochi/tools/mod.rs`.  
**Não alterar:** `windows/src-tauri/src/spotify.rs`. A pílula de música não muda.

**Passos:**
1. Criar `media.rs` com o COM mínimo (`CoInitializeEx` na thread), sem importar `spotify.rs`.
2. Plano puro: `acao` vira operação (`TryPlayAsync`, `TryPauseAsync`, `TrySkipNextAsync`, `TrySkipPreviousAsync`) e status esperado.
3. Trait de sessão com `GetCurrentSession`. Sem player, `execute` responde `PrerequisiteMissing` e não chama nenhum controle.
4. Id da faixa por hash de título, artista e álbum, só para comparar. Não sai na saída nem no registro.
5. `verify` pura para status e troca de faixa. Espera de até 1,5 s em `execute`.
6. Em `host.rs`, sondar `media_session`.
7. Registrar `midia` em `register_m2`.

**Testes:**
- mapa de `acao` para operação e status;
- `verify` pura: faixa igual e diferente;
- sem sessão: `PrerequisiteMissing` e zero chamadas;
- duas sessões falsas, com a atual sendo a B: só B recebe chamadas;
- `TryPlayAsync` recusado vira `ExecutionFailed`.

**Validação:** `cargo test --lib` (contorno). Confirmar com `git diff` que `spotify.rs` não mudou.

**Teste manual (checkpoint M2.5):** com Spotify tocando, "pausa a música" pausa e relata; "próxima música" troca de faixa. Com um vídeo no navegador como player atual, só ele responde. Sem nada tocando, "toca a música" responde "nenhum player ativo" e nada abre.

**Commit:** `feat(m2.5): tocar, pausar e trocar faixa do player ativo`

---

## Subetapa M2.6 — Energia

**Arquivos novos:** `windows/src-tauri/src/mochi/tools/power.rs`.  
**Arquivos alterados:** `windows/src-tauri/src/mochi/host.rs` (`sleep_supported`, `shutdown_privilege`), `windows/src-tauri/src/mochi/tools/mod.rs`, `windows/src-tauri/Cargo.toml` (`Win32_System_Shutdown`).

**Passos:**
1. Acrescentar `Win32_System_Shutdown` ao `Cargo.toml`.
2. Em `host.rs`, sondar `sleep_supported` (`GetPwrCapabilities`, `SystemS3`) e `shutdown_privilege` (o token tem `SeShutdownPrivilege`, lido como em `current_user_sid`).
3. Criar `power.rs` com quatro ferramentas, sem argumentos:
   - `bloquear_tela`, risco `Medium`: `LockWorkStation`;
   - `suspender`, risco `High`: `SetSuspendState(false, false, false)`;
   - `reiniciar` e `desligar`, risco `High`: habilitar `SeShutdownPrivilege` e `ExitWindowsEx` com `EWX_RESTART` ou `EWX_SHUTDOWN`, sem `EWX_FORCE`.
4. `dry_run` com os textos da spec (aviso de trabalho não salvo nos três altos).
5. `verify` pura: `aceito` igual ao pedido. Falha da API vira `ExecutionFailed` com o código numérico.
6. Trait de energia com implementação real em `cfg(windows)` e falsa em `cfg(test)`, que conta chamadas.
7. Registrar as quatro em `register_m2`.

**Testes:**
- contador do backend falso: zero em negar, timeout e pausa; um quando aceito (usando `DefaultDenyRequester` e `FixedRequester` por `invoke_tool`);
- schema sem argumento;
- `dry_run` dos três altos contém o aviso;
- sem privilégio ou sem suspensão: `Unavailable`;
- API falsa com retorno falso: `ExecutionFailed`.

**Validação:** `cargo test --lib` (contorno). Busca de segurança: nenhum teste chama `LockWorkStation`, `SetSuspendState` ou `ExitWindowsEx`.

**Teste manual (dono, notebook de teste, com trabalho salvo):**
- "bloqueia a tela": negar não faz nada; aceitar bloqueia.
- "suspende o computador": negar não faz nada; aceitar suspende (retomar cobre o M2.7).
- "reinicia" e "desliga": negar e deixar expirar não fazem nada. A execução real só com pedido explícito do dono e tudo salvo.

**Commit:** `feat(m2.6): bloquear, suspender, reiniciar e desligar com clique`

---

## Subetapa M2.7 — Fechamento

**Arquivos alterados:** `windows/README.md`, `docs/STATE-FORK.md`, `docs/ROADMAP-FORK.md` (linha da M2), `docs/roadmap/m2-windows-nativo.md` (marcações e evidência).

**Passos:**
1. Conferir com o dono a lista de dez comandos (spec 6.7). Rodar cada um três vezes pelo chat e anotar na tabela de evidência: data, versão, resultado, sem dados pessoais.
2. Testes de borda:
   - retomar da suspensão: depois de suspender e retomar, "volume 30%" e "pausa a música" funcionam;
   - aplicativo ausente: "abre um app inexistente" responde "não encontrado", com motivo;
   - dispositivo de áudio trocado: trocar a saída padrão no Windows sem reiniciar o Lumen, e "volume 30%" age na saída nova.
3. Atualizar `windows/README.md` (ferramentas, limites, o que ficou de fora), o estado em `docs/STATE-FORK.md` e a linha da M2 em `docs/ROADMAP-FORK.md`.
4. Rodar a validação completa: `cargo test --lib` (contorno), e `npx tsc --noEmit` e `npm test` em `windows/` se algum TypeScript tiver mudado.
5. Commit: `docs(m2): fechamento com evidência dos dez comandos`.

**Checkpoint M2:** 10 comandos com sucesso repetível (30 execuções, ou falhas registradas com motivo). Toda falha aparece como falha.

**Arquivamento (AGENTS, passo 6):** mover `docs/roadmap/m2-windows-nativo.md` para `docs/arquivadas/etapas/`, e a spec e o plano para `docs/arquivadas/superpowers/`. Isso só acontece depois do relato do dono sobre o checkpoint real. Antes, os checkpoints reais ficam abertos.

---

## Após o fechamento

- **Push e release.** Push só para `origin` (`playertwo1/lumen`), e só com pedido do dono. Release segue [release-checklist.md](../../roadmap/release-checklist.md), que diz que release só sai com pedido explícito do dono. Se houver release, conferir o What's new com `python3 scripts/test-whats-new.py`.
- **Registro de evidência.** Cada subetapa registra versão, ambiente e resultados sem segredos, prompts ou caminhos pessoais.
- **Dúvida que muda o plano.** Parar e perguntar ao dono antes de seguir. Não decidir sozinho o que está na seção 9 da spec.
