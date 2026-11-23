# Plano — limites do Antigravity no notch

Spec: `docs/superpowers/specs/2026-11-23-antigravity-plano-notch.md`

1. `windows/src-tauri/src/agy_plan.rs` (novo): acha `agy` (`platform::cli_candidates`), confere versão, roda `/usage` com 30 s, sem console, reduz a resposta aos campos usados; `parse_version` e `limits_only` com testes.
2. `lib.rs`: `mod agy_plan;`, comando `agy_plan_usage`, registro no `invoke_handler`.
3. `settings.rs`: `show_antigravity_plan_in_notch` (campo, padrão, lista de chaves editáveis e exemplo de teste).
4. `core/state.ts`: `showAntigravityPlanInNotch`, `agyPlanUsage`, `planDetailIsAgy`. `core/bridge.ts`: `agyPlanUsage`.
5. `core/plan.ts`: `parseAgyPlan` (pior bucket por janela), textos e subtítulo.
6. `views/usage.ts` e `views/views.ts`: pílula, card, refresh com cache; `island.ts`: zera `planDetailIsAgy` junto com o do Codex.
7. `settings/main.ts`, `i18n/extra.json` (o `style.css` não precisou mudar: `both-plans` já vale para duas ou mais pílulas).
8. Testes: `tests/plan.test.mjs`. Verificação: `npx tsc --noEmit`, `npm test`, `cargo test --lib`, `npm run tauri dev`.
