# STATE do fork — ponto de retomada

**Atualizado:** 2026-11-14 · **Última release:** [`windows-v0.6.16`](https://github.com/playertwo1/coucou/releases/tag/windows-v0.6.16) (Lumen; L2 aprovado) · **Fase da vez:** [Fase L — Lumen + Segunda Mente](roadmap/fase-l-lumen.md), L0 concluída; L1.4a e L1.4d-1/d-2/d-3/d-4 aprovadas visualmente; protótipo L1.4b entregue; Canvas escolhido na L1.4c; L1.3 e L1.4 reais em avaliação; Checkpoint L2 fechado em 2026-11-14 (`0.6.16`); L3.1, L3.2 e animação P3 implementadas e validadas sinteticamente (aguardando Checkpoint L3 real no notebook)

Este arquivo guarda só o estado atual e a próxima ação. Fases, checkpoints e evidências ficam nos arquivos de `docs/roadmap/`, e a ordem fica no [ROADMAP-FORK](ROADMAP-FORK.md).

## Estado

| Fase | Estado |
|---|---|
| 0, 1, 2 | Pausadas (D-053); ver inventário L0 na Fase L |
| L | Em andamento. L0 concluída; L1.1 B escolhida; L1.2 implementada; L1.3 aprovada sinteticamente; avaliação real da migração e L1.4 pendente; L1.4a e L1.4d-1/d-2/d-3/d-4 aprovadas visualmente; L1.4b entregue com limites de medição registrados; Canvas escolhido na L1.4c em 2026-11-13; L2.1/L2.2 e Checkpoint L2 fechados em 2026-11-14 (`0.6.16`); L3.1, L3.2 e animação P3 concluídas sinteticamente em 2026-11-14 (aguardando Checkpoint L3 real); próxima etapa após aceite L3: L4 (depende da F11-01 na Segunda Mente) |
| 2B, 2C | Pausadas (decisão do dono, 2026-11-13) |
| M0–M10 | Viram "Lumen assistente"; começam depois do Checkpoint L7 |
| 3 (identidade) | Incorporada à L1 |

## Trabalho em andamento fora da `main`

- Worktree `../coucou-git-pill`, branch `codex/windows-git-pill`: pílula Git somente leitura.
- Worktree `../coucou-permission-toast-2-1a`, branch `codex/windows-permission-notifications-eval`: avaliação de notificações de permissão.
- Branch `codex/windows-permission-toast-synthetic`.
- Worktree `../coucou-lumen-identity`, branch `codex/l1-lumen-identidade`: commit `c3b2af2` está em `origin/main`; tag e release `windows-v0.6.13` publicadas. CI `43424866776` e CI da release `43428181413` passaram nos testes sintéticos MSI/NSIS; Defender não detectou ameaças nos artefatos do preflight nem nos instaladores finais. L1 aguarda avaliação real do proprietário.
- Na worktree principal há alterações locais ainda não commitadas do app sintético (`windows/synthetic/`, testes e specs de 2026-11-12). Elas são do dono e ficaram fora do commit de reestruturação de 2026-11-13.
- As worktrees acima e as alterações locais do app sintético não foram alteradas nesta sessão. O preflight 0.6.12 `43402455643` foi cancelado; a tag existe, mas não há release publicada.

## Próxima ação

1. O proprietário relatou que a identidade da 0.6.13 aparece ("aparece tudo"). Ainda faltam os resultados por cenário da migração real: tipo de instalação (MSI ou NSIS), settings, hooks, permissões e atualizador.
2. **Decisões do proprietário (2026-11-13 e 2026-11-14):** sem autorização escrita para usar o Mochi, os personagens do Lumen são originais; a Bússola continua somente como ícone do app. O aceite visual explícito cobre a lineup, as pranchas estáticas e as subetapas L1.4d-1, L1.4d-2, L1.4d-3 e L1.4d-4 (incluindo os ajustes de `working` com computador e `PC Memória` com pente horizontal de RAM em `afc7552`). As aprovações visuais não fecham o checkpoint real L1.4 nem validam o app em uso real; os limites de medição da comparação continuam registrados.
3. **Checkpoint L2 fechado (2026-11-14):** login e chat ChatGPT reais aprovados pelo proprietário na `0.6.16`.
4. **L3 implementada sinteticamente (2026-11-14):**
   - **L3.1 (Anotador):** captura de voz só pelo microfone (`notes_start`/`notes_idea`) e criação rápida em texto (`notes_create_text`), usando `chat::complete_oneshot` com o provedor ativo do chat (`chatgpt` OAuth, Anthropic, compatíveis, CLI, local).
   - **L3.2 (Personas + Comentários):** importação manual (`personas_import`, formato 3 partes QwenPaw e `CharacterModel`), roteamento por afinidade/LLM/`@menção`/fio de resposta, limite de turnos e trava contra duplicação (`commentHash` + `characterId` + lock em memória).
   - **Animação P3:** evento `"note-created"` aciona `triggerNoteWrite()` em `CompassEngine`, respeitando `prefers-reduced-motion`.
   - **Validação sintética:** `npx tsc --noEmit`, 676 testes Node (`fail 0`, `skipped 0`), `npm run build` (`vite build`) e 671 testes Rust (8 ignorados) passaram em 2026-11-14. Falta o **Checkpoint L3 real** no notebook (ditar nota real e confirmar comentário de persona sem duplicar).
5. Achados do QwenPaw registrados em [research/qwenpaw.md](research/qwenpaw.md) e marcados "(QwenPaw)" em L2, L3, L4, L6, M0, M1, M5, M8 e M9.

## Pendências do dono

- **Checkpoint L3 (Anotador):** testar na próxima build local uma nota ditada e confirmar geração de comentário de persona sem duplicação.
- Informar os resultados reais pendentes da instalação da 0.6.13 (MSI/NSIS, settings, hooks, permissões e atualizador).
- Validar no uso real os novos personagens e animações no app (L1.4), além de L1.3.
- Validações reais abertas nas fases 0, 1 e 2 (ver inventário da L0 na Fase L).
