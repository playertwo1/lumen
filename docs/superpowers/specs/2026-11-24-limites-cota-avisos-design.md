# Spec — Avisos progressivos de cota (50%, 40%, 30%, 20%, 10% restantes)

**Data:** 2026-11-24 · **Escopo:** Codex, Claude e Antigravity (janelas de 5h e semanal)

## 1. Objetivo

Avisar o usuário automaticamente sempre que a cota **restante** de **5 horas** (`fiveHour`) ou **semanal** (`sevenDay`) de **Codex**, **Claude** ou **Antigravity** atingir os patamares **50%, 40%, 30%, 20% e 10%** antes de zerar, usando cores com urgência crescente quanto mais próximo de 0% ficar, tanto na **Ilha** quanto por **Toast nativo do Windows**.

## 2. Regras de cálculo, patamares e cores

Para cada janela (`fiveHour` = `5h`, `sevenDay` = `semana`) de cada provedor (`claude`, `codex`, `agy`):

- `remainingPct = Math.max(0, 100 - effectivePct(window, now))`
- Se `window.resetsAt <= now`, `effectivePct` vale `0` (`remainingPct = 100`), portanto nenhum aviso dispara para janela expirada.
- Patamares avaliados em ordem decrescente de urgência: `[10, 20, 30, 40, 50]`.
  O patamar ativo é o menor `t` tal que `remainingPct <= t`:

| Restante | Nível | Cor hex | Rótulo de urgência | Som (bússola) | Duração na Ilha |
|---:|---|---|---|---|---|
| `<= 50%` | 1 | `#FACC15` | Atenção | `blip` | 8 s |
| `<= 40%` | 2 | `#F59E0B` | Alerta | `blip` | 8 s |
| `<= 30%` | 3 | `#F97316` | Elevado | `rate` | 10 s |
| `<= 20%` | 4 | `#F4505E` | Alto | `rate` | 12 s |
| `<= 10%` | 5 | `#FF2E4C` | Crítico | `error` | 12 s |

## 3. Deduplicação, saltos e reinício de ciclo

- Chave de estado por janela: `${provider}:${windowKey}` (`claude:5h`, `claude:semana`, `codex:5h`, `codex:semana`, `agy:5h`, `agy:semana`), persistida em `localStorage` (`lumen.planThresholdAlerts.v1`).
- Cada entrada guarda `{ resetsAt: number, lowestAlerted: number }`.
- **Novo ciclo:** quando `Math.abs(window.resetsAt - entry.resetsAt) > 120_000` (2 min de tolerância para jitter de relógio) ou quando `remainingPct > entry.lowestAlerted + 5` (recarga/crédito aplicado), o estado daquela janela é limpo e um novo aviso pode disparar.
- **Salto entre patamares:** se a cota cair de 65% direto para 18% restante, dispara **apenas um aviso** (o mais urgente atingido, `20%`) e registra `lowestAlerted = 20`, evitando rajada de 4 avisos seguidos.
- **Múltiplas janelas ao mesmo tempo:** uma fila ordenada por maior urgência exibe um aviso por vez na faixa da Ilha (sem sobrescrever o anterior silenciosamente) e cada alerta envia seu Toast nativo agrupado por `${provider}_${windowKey}`.

## 4. Superfícies de notificação

1. **Ilha (faixa compacta `#compact-center` + reveal):**
   - Texto focado no quanto falta até zerar, mantendo as pílulas do cabeçalho intactas em `% usado`:
     - Ex.: `"Codex · 5h: restam 20%"` ou `"Claude · Semana: restam 10%"` (se `remainingPct <= 0`, `"Codex · 5h: cota zerada"`).
   - Ponto colorido + borda/tinta sutil na cor exata do patamar (`--compact-notice-color` em `style.css`), com pulso discreto nos níveis críticos (`<= 20%`).
   - Se a ilha estiver recolhida (`hidden`), revela em modo compacto silenciosamente (`revealSilently()`) para não duplicar o som do alerta.
   - Clique na faixa abre a ilha expandida na visão geral com o card do respectivo plano aberto (`island.openPlan(provider)`).
2. **Toast nativo do Windows (`permission_notifications.rs`):**
   - Novo comando Tauri `#[cfg(windows)] plan_alert_toast_show` no grupo `plan_quota` com tag estável `plan_${provider}_${windowKey}`, substituindo alertas anteriores da mesma janela na Central de Ações em vez de empilhar.
   - Título: `"Lumen · Cota em {threshold}% ({Provider})"` (ou `"Cota crítica · {Provider}"` em `<= 10%`).
   - Corpo: `"{Janela}: restam {remaining}% antes de zerar."`
   - `<audio silent="true"/>` para evitar sobrepor o toque da Ilha.
   - Clique no Toast emite `plan-toast-open` para a janela `island`, abrindo o card daquele plano.

## 5. Atualização em segundo plano (Codex e Antigravity)

- **Claude:** segue em tempo real via hook `StatusLine` (`setClaudePlanUsage`).
- **Codex e Antigravity:**
  - Timer periódico a cada **3 minutos** (`180_000 ms`) chama `refreshCodexPlanUsage()` / `refreshAgyPlanUsage()` quando `planThresholdAlerts` estiver ativo, `!State.paused`, e a respectiva pílula estiver habilitada (`showCodexPlanInNotch` / `showAntigravityPlanInNotch`).
  - Fim de turno (`Stop` hook em `hooks.ts`): quando `agent_codex` ou `agent_antigravity` conclui um turno e `planThresholdAlerts && !State.paused`, dispara `refreshCodexPlanUsage()` / `refreshAgyPlanUsage()` (protegido por `codexIsStale` de 60 s e trava `inFlight`).

## 6. Preferência em Ajustes

- Campo `planThresholdAlerts: boolean` (padrão `true`) em `Settings` (`windows/src/core/state.ts` e `windows/src-tauri/src/settings.rs`).
- Toggle em `settings/main.ts` na seção **Uso do plano**: `"Avisar nos limites de cota (50% a 10% restantes)"`.

## 7. Contratos preservados e Rollback

- `planColor(pct)` e os rótulos/barras atuais das pílulas continuam intactos para não quebrar contratos e testes existentes (`plan.test.mjs`).
- `Settings` usa `#[serde(rename_all = "camelCase", default)]`, mantendo compatibilidade retroativa com `settings.json` antigo; rollback é reverter o commit sem migração destrutiva de arquivos.
