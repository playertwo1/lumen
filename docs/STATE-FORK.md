# STATE do fork — ponto de retomada

**Atualizado:** 2026-11-18 · **Última release:** [`windows-v0.7.17`](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.17) · **Etapas da vez:** [L8 — Ajustes do uso real](roadmap/l8-ajustes-uso-real.md) e [M2 — Controle nativo do Windows](roadmap/m2-windows-nativo.md) (validação do dono)

Este arquivo guarda só o estado atual, o que espera o dono e a próxima ação. O que cada etapa faz está no [ROADMAP-FORK](ROADMAP-FORK.md); o histórico deste arquivo até a reorganização está em [arquivadas/state/](arquivadas/state/state-ate-2026-11-17.md).

## Onde estamos

| Etapas | Estado |
|---|---|
| L0–L7, M0, M1 | Concluídas e arquivadas (M1 fechada em 2026-11-17 por decisão do dono, com parte do teste real dispensada) |
| L8 | Todas as subetapas implementadas e publicadas na `0.7.12`; validação real pendente, uma por vez |
| M2 | M2.1–M2.6 implementadas e publicadas na `0.7.14` (2026-11-18); a `0.7.15` corrige o ping pelo chat e a `0.7.16` as falhas do 1º relato da M2.7. No 2º relato os 10 comandos funcionaram; a `0.7.17` ajusta "organizar para programar" (Codex, Claude Code e Antigravity lado a lado) |
| L6.5 | A fazer depois do contrato v1.1 da Segunda Mente |
| 0, 1, 2, 2B, 2C | Pausadas (D-053) |

## Próxima ação

1. O dono testa a L8 no notebook, uma subetapa por vez (L8.1 → L8.6), e relata. Cada relato fecha ou reabre o checkpoint correspondente em [l8-ajustes-uso-real.md](roadmap/l8-ajustes-uso-real.md).
2. M2.7: o dono atualiza para a `0.7.17` e testa "organize as janelas para programar" e o desfazer (3 vezes), mais retomar da suspensão, app ausente e troca de áudio. O relato fecha ou reabre os checkpoints em [m2-windows-nativo.md](roadmap/m2-windows-nativo.md).

## Pendências do dono (validação real)

- **L8.1–L8.6** e o Apagar/Spam da triagem do MailCraft (`0.7.12`): roteiros nos checkpoints da L8.
- **Cristal na abertura** (`0.7.10`, `0.7.11` e `0.7.13`): conferir, depois de atualizar, o ícone no Menu Iniciar, a janela de abertura e a saudação da ilha.
- **`ping` pelo chat**: na `0.7.14` o ChatGPT quase sempre recusava (ferramentas só como texto no prompt). Corrigido na `main` em 2026-11-18 (funções nativas da Responses API; 3 de 3 no teste real e no modo dev, ~9 s a ida e volta). Saiu na `0.7.15`; conferir no app instalado.
- **M2 no notebook** (`0.7.14`): volume, brilho (e se o monitor externo responde por DDC/CI), apps, janelas, mídia e energia; conferir se o notebook é "Modern Standby" e se `suspender` aparece.

## Pendências técnicas conhecidas

- L8.4 ainda sem os testes com mock de cada motor; L8.5 deixou o fluxo de hooks do Orca para depois (ver o arquivo da L8).
- `lumen.exe` e `lumen-hook.exe` do `windows-latest` continuam os da `0.7.9`: o workflow de release não os atualiza.