# Plano — Avisos progressivos de cota (50%, 40%, 30%, 20%, 10% restantes)

**Spec:** [2026-11-24-limites-cota-avisos-design.md](../specs/2026-11-24-limites-cota-avisos-design.md)

## Arquivos a criar/alterar

1. **`windows/src/core/plan-alerts.ts` (novo, sem DOM):**
   - Define `PlanProvider = "claude" | "codex" | "agy"`, `PlanWindowKey = "fiveHour" | "sevenDay"`, patamares `[50, 40, 30, 20, 10]` e mapa de cores/níveis de urgência (`quotaThresholdStyle`).
   - Implementa `PlanThresholdTracker` com estado por janela (`resetsAt`, `lowestAlerted`), serialização/restauração JSON segura para `localStorage`, lógica de salto (`jump` para o menor patamar atingido), reinício de ciclo por `resetsAt` ou recarga e formatação pt-BR das mensagens (`formatPlanAlert`).
2. **`windows/src/core/compact.ts` e `windows/src/core/state.ts`:**
   - Adicionar `kind: "plan"` em `CompactCenterAction` (`{ kind: "plan"; provider: PlanProvider }`).
   - Adicionar `urgent?: boolean` em `CompactNotice` para aplicar destaque de cor no `#compact-center`.
   - Adicionar `planThresholdAlerts: boolean` (default `true`) em `Settings` e `DEFAULT_SETTINGS`.
3. **`windows/src-tauri/src/settings.rs`:**
   - Adicionar `pub plan_threshold_alerts: bool` (`default_true` via `Default` + fixture `CUSTOM` + teste de chaves camelCase).
4. **`windows/src-tauri/src/permission_notifications.rs` e `windows/src-tauri/src/lib.rs`:**
   - Adicionar `Command::ShowPlanAlert` no worker de Toasts do Windows, grupo `"plan_quota"`, XML `ToastGeneric` com `<audio silent="true"/>`, tag determinística por `${provider}_${windowKey}` e ativação emitindo `"plan-toast-open"` para a janela `"island"`.
   - Expor comando Tauri `#[cfg(windows)] plan_alert_toast_show`.
5. **`windows/src/core/bridge.ts`:**
   - Adicionar `Bridge.planAlertToast(...)`.
6. **`windows/src/views/usage.ts`, `windows/src/island/island.ts`, `windows/src/island/hooks.ts`, `windows/src/main.ts`:**
   - Criar fila de avisos na `Island` + suporte a `island.openPlan(provider)` + estilização da faixa `--notice-color` em `#compact-center` + listener de `"plan-toast-open"`.
   - Integrar `evaluatePlanThresholds` ao atualizar `Claude` (`setClaudePlanUsage`), `Codex` (`refreshCodexPlanUsage`) e `Antigravity` (`refreshAgyPlanUsage`).
   - Timer de 3 min para `Codex` / `Antigravity` quando ativos e disparo em `Stop` hook para `agent_codex` / `agent_antigravity`.
7. **`windows/src/style.css` e `windows/src/settings/main.ts`:**
   - Ler `craft-floor.md` imediatamente antes de editar UI/CSS.
   - Estilizar `#compact-center.has-notice-tint` usando `--notice-color` (borda sutil, fundo levemente tingido e cor de texto legível com contraste >= 4.5:1) e pulso suave quando `urgent` (`<= 20%`).
   - Adicionar toggle `"Avisar nos limites de cota (50% a 10% restantes)"` em Ajustes → Uso do plano.
8. **Testes (`windows/tests/plan-alerts.test.mjs`, `windows/tests/plan.test.mjs`, `windows/src-tauri/src/permission_notifications.rs`, `windows/src-tauri/src/settings.rs`):**
   - Cobertura completa de cruzamento sequencial 50→40→30→20→10, salto 100→15 disparando apenas uma vez no nível 20, reinício por `resetsAt`, janela expirada, todos os 3 provedores × 2 janelas, XML/ativação do Toast em Rust e chaves do `settings.rs`.

## Verificação planejada

- `cd windows && npx tsc --noEmit`
- `cd windows && npm test`
- `cd windows && npm run build`
- `cd windows/src-tauri && cargo test --lib --locked`
- `impeccable.cmd detect --json --changed` (após alterações de CSS/UI)
