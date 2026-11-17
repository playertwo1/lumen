# STATE do fork — ponto de retomada

**Atualizado:** 2026-11-17 · **Última release:** [`windows-v0.7.13`](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.13) · **Etapa da vez:** [L8 — Ajustes do uso real](roadmap/l8-ajustes-uso-real.md)

Este arquivo guarda só o estado atual, o que espera o dono e a próxima ação. O que cada etapa faz está no [ROADMAP-FORK](ROADMAP-FORK.md); o histórico deste arquivo até a reorganização está em [arquivadas/state/](arquivadas/state/state-ate-2026-11-17.md).

## Onde estamos

| Etapas | Estado |
|---|---|
| L0–L7, M0 | Concluídas e arquivadas (checkpoints reais de L1–L7 fechados até 2026-11-15) |
| L8 | Todas as subetapas implementadas e publicadas na `0.7.12`; validação real pendente, uma por vez |
| M1 | Implementada e validada sinteticamente em 2026-11-16; checkpoint real pendente. M2 só depois da L8 e do checkpoint M1 |
| L6.5 | A fazer depois do contrato v1.1 da Segunda Mente |
| 0, 1, 2, 2B, 2C | Pausadas (D-053) |

## Próxima ação

1. O dono testa a L8 no notebook, uma subetapa por vez (L8.1 → L8.6), e relata. Cada relato fecha ou reabre o checkpoint correspondente em [l8-ajustes-uso-real.md](roadmap/l8-ajustes-uso-real.md).
2. Depois da L8: checkpoint real da M1 e, com ele fechado, a M2.

## Pendências do dono (validação real)

- **L8.1–L8.6** e o Apagar/Spam da triagem do MailCraft (`0.7.12`): roteiros nos checkpoints da L8.
- **Cristal na abertura** (`0.7.10`, `0.7.11` e `0.7.13`): conferir, depois de atualizar, o ícone no Menu Iniciar, a janela de abertura e a saudação da ilha.
- **Checkpoint M1:** no notebook, o chat conversa, mostra o estado e não executa nada fora da lista habilitada; uma CLI externa lista e chama o `ping` pelo MCP.

## Pendências técnicas conhecidas

- L8.4 ainda sem os testes com mock de cada motor; L8.5 deixou o fluxo de hooks do Orca para depois (ver o arquivo da L8).
- `lumen.exe` e `lumen-hook.exe` do `windows-latest` continuam os da `0.7.9`: o workflow de release não os atualiza.
