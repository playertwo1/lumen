# Limites de 5 h e semanal do Antigravity no notch

## Problema
O notch mostra os limites de 5 h e semanais do Claude e do Codex (pílulas no cabeçalho e card). O Antigravity também tem essas duas janelas, mas o Lumen não as exibe.

## Fonte (descoberta em 2026-11-23, só leitura)
- Não existe arquivo local nem porta utilizável: o `language_server` do Antigravity recebe o token por stdin, e os logs só trazem "quota reached" depois de estourar.
- O Orca resolve com a CLI: `agy -p /usage --output-format json --print-timeout 20s` (`agy` 1.1.11 ou mais novo, `AGY_CLI_DISABLE_AUTO_UPDATE=true`). Nesta máquina (agy 1.5.15) a resposta traz `command.name == "usage"` e `command.data.groups[].buckets[]` com `window` (`"5h"` ou `"weekly"`), `remaining_fraction` (0 a 1), `reset_time` (ISO 8601) e `disabled` opcional. Tokens gastos: 0.
- Em `agy` antigo, `/usage` é tratado como prompt e gasta cota. Por isso o Lumen confere `agy --version >= 1.1.11` antes e só aceita a resposta se `command.name == "usage"`.
- Mesmo padrão do Codex: o Lumen não lê credencial nem faz chamada de rede própria; o `agy` usa o login dele. Nada é guardado, então não há segredo para o Credential Manager.

## Decisão
- Nova pílula "Antigravity NN%" no cabeçalho da visão geral, depois da do Codex, e um card igual ao do Codex (5 horas, Semana).
- Cada janela mostra o **pior** bucket entre os grupos de modelos (Gemini, Claude e GPT, Argon): maior % usado. Bucket `disabled` é ignorado (o limite de 5 h não vale quando o semanal estourou).
- Desligada por padrão (`showAntigravityPlanInNotch`), com interruptor em Configurações → Plano. Roda só quando a pílula aparece ou é clicada, no máximo uma vez por vez, 30 s de limite, 60 s de cache, nunca pausado (`PAUSED`).
- `agy` ausente, antigo, sem login, lento ou resposta fora do formato: a pílula fica "Antigravity —", sem erro na tela.
- Frases novas em `windows/src/i18n/extra.json`, 13 línguas.

## Fora de escopo
Detalhe por grupo de modelo, bucket do Argon (só semanal, entra na janela semanal), compacta da ilha, telefone e voz.

## Aceite
- `cargo test --lib` e `npm test` passam, com testes novos de parse (`plan.test.mjs`) e do leitor Rust.
- Com o app aberto e o interruptor ligado, a pílula mostra os percentuais reais do `agy /usage`.
- Rollback: desligar o interruptor; nenhuma migração, nada instalado.
