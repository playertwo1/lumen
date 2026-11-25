# STATE do fork — ponto de retomada

**Atualizado:** 2026-11-24 · **Última release:** [`windows-v0.7.27`](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.27) · **Etapas da vez:** [L8 — Ajustes do uso real](roadmap/l8-ajustes-uso-real.md) e [M2 — Controle nativo do Windows](roadmap/m2-windows-nativo.md) (validação do dono)

Este arquivo guarda só o estado atual, o que espera o dono e a próxima ação. O que cada etapa faz está no [ROADMAP-FORK](ROADMAP-FORK.md); o histórico deste arquivo até a reorganização está em [arquivadas/state/](arquivadas/state/state-ate-2026-11-17.md).

## Onde estamos

| Etapas | Estado |
|---|---|
| L0–L7, M0, M1 | Concluídas e arquivadas (M1 fechada em 2026-11-17 por decisão do dono, com parte do teste real dispensada; escopo pontual de triagem e resumo do MailCraft na L5 reaberto em 2026-11-23/24 e publicado na `0.7.23`/`0.7.24`, aguardando relato real) |
| L8 | Todas as subetapas implementadas e publicadas na `0.7.12`; validação real pendente, uma por vez |
| M2 | M2.1–M2.6 implementadas e publicadas na `0.7.14` (2026-11-18); a `0.7.15` corrige o ping pelo chat e a `0.7.16` as falhas do 1º relato da M2.7. No 2º relato os 10 comandos funcionaram; a `0.7.17` ajusta "organizar para programar" (Codex, Claude Code e Antigravity lado a lado), confirmado no 3º relato. Faltam os casos de borda da M2.7 |
| M2b | M2b.1–M2b.4 implementadas na `main` em 2026-11-21 (botões de comando na pílula, escolha e ordem nos Ajustes); fecha a M2b.5 com o teste real do dono. Publicada na `0.7.19` (2026-11-22); teste real pendente |
| L6.5 | A fazer depois do contrato v1.1 da Segunda Mente |
| 0, 1, 2, 2B, 2C | Pausadas (D-053) |

## Próxima ação

1. O dono testa a `0.7.25` instalada (avisos progressivos de cota 50%–10% no Claude/Codex/Antigravity na ilha e no Windows Toast; triagem completa com **Excluir** ao lado de **Abrir** nos prioritários e resumo rápido no MailCraft + ditado com Whisper) e a L8 no notebook, uma subetapa por vez (L8.1 → L8.6), e relata. Cada relato fecha ou reabre o checkpoint correspondente em [l8-ajustes-uso-real.md](roadmap/l8-ajustes-uso-real.md).
2. M2.7: os 10 comandos estão fechados (3º relato, `0.7.17`). Falta o dono testar retomar da suspensão (pedir "volume 20%" depois de acordar o notebook), app ausente ("abre o xyzzy" deve dizer que não encontrou) e troca de dispositivo de áudio (trocar a saída e pedir "volume 20%"). Depois disso: atualizar o `windows/README.md` e arquivar a M2. O relato fecha ou reabre os checkpoints em [m2-windows-nativo.md](roadmap/m2-windows-nativo.md).

## Pendências do dono (validação real)

- **Hook atualizado com sessões abertas (`0.7.27`)**: depois de instalar, sem fechar os agentes, o `coucou-hook.exe` em `%LOCALAPPDATA%\Coucou\bin` deve ter a data da instalação.
- **Personagens trabalhando (`0.7.26`)**: com a principal em Claude Desktop, rodar o Claude Code no Orca e ver o personagem principal trabalhando; com várias sessões ao mesmo tempo, uma terminar não pode parar o personagem. Conferir se o `coucou-hook.exe` em `%LOCALAPPDATA%\Coucou\bin` foi atualizado (data de hoje).
- **Avisos progressivos de cota (`0.7.25`)**: validar no app instalado os avisos de 50%, 40%, 30%, 20% e 10% restantes (5h e semana) para Claude, Codex e Antigravity, o clique no Windows Toast abrindo o card correspondente e o toggle em Configurações → Uso do plano.
- **MailCraft (triagem completa, Excluir nos prioritários, resumo rápido e master-detail) + ditado com Whisper** (`0.7.23`–`0.7.24`): conferir na versão instalada se a triagem analisa todas as mensagens (ex.: ~54), se o botão **Excluir** ao lado de **Abrir** confirma em dois cliques e move para Excluídos com **Desfazer**, se o resumo rápido cobre toda a caixa com Markdown legível e timestamp, se `⋯` / `Limpar grupo` / rascunho encadeado funcionam e se o ditado com Whisper não pede para baixar o Parakeet.
- **WhatsApp e MailCraft na ilha** (`0.7.21`): conferir faixa e mascote do WhatsApp, leitura e abertura da mensagem no MailCraft e o foco ao navegar pelo teclado e com NVDA, usando a versão instalada.
- **M2b.6 — comando de voz** (`0.7.22`): apertar Ctrl+Alt+V e falar "brilho a 20%" e depois "tirar som" no microfone; a pílula mostra o resultado. A tecla muda em Notch → Atalhos.
- **M2b — botões de comando** (`main`, desde `0.7.19`): passar o mouse na pílula, clicar o raio e testar os 12 botões (mídia, volume, brilho, janelas lado a lado, bloquear tela); conferir se a barra cabe na pílula, inclusive nas bordas esquerda e direita; esconder e reordenar em Ditado → Ajustes e reiniciar o Lumen para ver se a escolha fica.

- **L8.1–L8.6** e o Apagar/Spam da triagem do MailCraft (`0.7.12`): roteiros nos checkpoints da L8.
- **Cristal na abertura** (`0.7.10`, `0.7.11` e `0.7.13`): conferir, depois de atualizar, o ícone no Menu Iniciar, a janela de abertura e a saudação da ilha.
- **`ping` pelo chat**: na `0.7.14` o ChatGPT quase sempre recusava (ferramentas só como texto no prompt). Corrigido na `main` em 2026-11-18 (funções nativas da Responses API; 3 de 3 no teste real e no modo dev, ~9 s a ida e volta). Saiu na `0.7.15`; conferir no app instalado.
- **M2 no notebook** (`0.7.14`): volume, brilho (e se o monitor externo responde por DDC/CI), apps, janelas, mídia e energia; conferir se o notebook é "Modern Standby" e se `suspender` aparece.

## Pendências técnicas conhecidas

- L8.4 ainda sem os testes com mock de cada motor; L8.5 deixou o fluxo de hooks do Orca para depois (ver o arquivo da L8).
- Na `0.7.25`, `lumen.exe`, `lumen-hook.exe`, `Lumen-Windows-setup.exe` e `Lumen-Windows.msi` em `windows-v0.7.25` e `windows-latest` foram compilados e publicados localmente via CLI (`CARGO_PROFILE_RELEASE_LTO=thin` e `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=4`); o script sintético de upgrade `scripts/test-lumen-upgrade.ps1` ainda não rodou para `0.7.25`.
- Resolvida: o exe de teste da lib saía com `0xc0000139` (sem o manifesto de Common Controls v6, que o `tauri-build` só embute no binário). Corrigido em `windows/src-tauri/src/lib.rs` (`global_asm!` em `cfg(test)`); `cargo test --lib` passa sem RUSTFLAGS e o "Windows CI" está verde no `main` (commit `3c1034a`).
