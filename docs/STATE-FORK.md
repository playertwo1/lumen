# STATE do fork — ponto de retomada

**Atualizado:** 2026-11-14 · **Última release:** [`windows-v0.7.2`](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.2) (Lumen; L4 Tarefas, L5 E-mail MailCraft/Outlook, L6.1 Relay DO SQLite e heróis Lumen no Hub empacotados) · **Fase da vez:** [Fase L — Lumen + Segunda Mente](roadmap/fase-l-lumen.md), L0 concluída; L1.4a e L1.4d-1/d-2/d-3/d-4 aprovadas visualmente; protótipo L1.4b entregue; Canvas escolhido na L1.4c; L1.3 e L1.4 reais em avaliação; Checkpoint L2 fechado em 2026-11-14 (`0.6.16`); L3.1, L3.2 e animação P3 concluídas sinteticamente (`windows-v0.7.1`); L4, L5, heróis do Hub e L6.1 integrados na `main` e empacotados em `windows-v0.7.2` no repositório próprio `playertwo1/lumen` (aguardando Checkpoints reais L3, L4 e L5 no notebook)

Este arquivo guarda só o estado atual e a próxima ação. Fases, checkpoints e evidências ficam nos arquivos de `docs/roadmap/`, e a ordem fica no [ROADMAP-FORK](ROADMAP-FORK.md).

## Estado

| Fase | Estado |
|---|---|
| 0, 1, 2 | Pausadas (D-053); ver inventário L0 na Fase L |
| L | Em andamento. L0 concluída; L1.1 B escolhida; L1.2 implementada; L1.3 aprovada sinteticamente; avaliação real da migração e L1.4 pendente; L1.4a e L1.4d-1/d-2/d-3/d-4 aprovadas visualmente; L1.4b entregue com limites de medição registrados; Canvas escolhido na L1.4c em 2026-11-13; L2.1/L2.2 e Checkpoint L2 fechados em 2026-11-14 (`0.6.16`); L3.1, L3.2 e animação P3 concluídas sinteticamente em 2026-11-14 (`windows-v0.7.1`, aguardando Checkpoint L3 real); L4.1–L4.4 e animações P3 concluídas sinteticamente (`main`, aguardando Checkpoint L4 real); L5.1, L5.2 e animação P3 concluídas sinteticamente (`main`, aguardando Checkpoint L5 real); L6.1 (caixa de correio persistente E2EE DO SQLite no relay) concluída sinteticamente (`windows-v0.7.2`); **L6.2 (Passo 5 da ponte: pareamento QR + `k_mailbox` + revogação `/revoke` ao parear de novo)** implementada e testada sinteticamente na branch `feat/l62-pareamento-revogacao`; próxima subfase após ok/merge: Passo 6 (`F12-03` na Segunda Mente) e depois L6.3 (Passo 7) |
| 2B, 2C | Pausadas (decisão do dono, 2026-11-13) |
| M0–M10 | Viram "Lumen assistente"; começam depois do Checkpoint L7 |
| 3 (identidade) | Incorporada à L1 |

## Trabalho em andamento fora da `main`

- **`feat/l62-pareamento-revogacao` (Passo 5 / L6.2, aguardando ok de merge do dono):** QR de pareamento completo (`docs/RELAY_LINK.md` §2), bloco do relay visível por padrão nas Configurações, `k_mailbox` validado contra `android/relay/test-vectors-mailbox.json` e revogação `POST /v1/mailbox/<room>/revoke` ao clicar em "Parear de novo" antes de rotacionar `room` e `K`. Validação sintética: `cargo test --lib phone_link` (`206 passed`, `0 failed`, `2 ignored` de E2E manual), `windows/` `npx tsc --noEmit` + `npm test` (`692 passed`, `0 failed`) e `android/relay/` `npm test` (`26` worker + `37` Node). **Real: NOT_RUN** (aguarda Worker implantado pelo dono e Passo 6 no Galaxy S25 Ultra).
- Worktrees e branches antigas (`coucou-git-pill`, `coucou-permission-toast-2-1a`, `coucou-l14d2-canvas-personagens`, `add_grok_hermes_characters`, `feat/l4-tarefas`, `feat/l5-email-mailcraft`, `feat/l6-sync-segunda-mente`) foram integradas ou limpas em 2026-11-14.
- **L4 (`3cb66a78`):** implementação completa (L4.1–L4.4 + animações P3 no `CompassEngine`). **Real: NOT_RUN** (aguarda teste de voz e Toast nativo no notebook).
- **L5 (`f598ec32`):** MailCraft principal via canal de controle loopback HMAC ou MCP stdio sem `--allow`, resumo via `chat::complete_oneshot` com `<mail_data untrusted="true">`, rascunho via `compose.saveDraft` sem jamais enviar e Outlook clássico secundário. **Real: NOT_RUN** (aguarda teste com MailCraft real no notebook). Spec: `docs/superpowers/specs/2026-11-14-l5-email-mailcraft.md`.
- **L6.1 (`87d09466`):** caixa de correio persistente E2EE no `android/relay/` com Durable Objects SQLite e especificação da L6. Próxima etapa no desktop: L6.2+.
- O preflight 0.6.12 `43402455643` foi cancelado; a tag existe, mas não há release publicada.

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

- **Checkpoint L5 (E-mail pessoal):** testar na próxima build com MailCraft em execução (`--control 7991` ou MCP) o resumo da caixa de entrada (confirmando animação P3 da carta pousando na mão) e o preparo de rascunho sem envio.
- **Checkpoint L4 (Tarefas):** testar na próxima build local captura por voz (`Ctrl+Alt+K` ou Hub → Tarefas) e disparo real de aviso Toast no Windows (`Adiar`, `Concluir`, `Abrir`).
- **Checkpoint L3 (Anotador):** testar na próxima build local uma nota ditada e confirmar geração de comentário de persona sem duplicação.
- Informar os resultados reais pendentes da instalação da 0.6.13 (MSI/NSIS, settings, hooks, permissões e atualizador).
- Validar no uso real os novos personagens e animações no app (L1.4), além de L1.3.
- Validações reais abertas nas fases 0, 1 e 2 (ver inventário da L0 na Fase L).
