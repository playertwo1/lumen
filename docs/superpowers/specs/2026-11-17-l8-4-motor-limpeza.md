# L8.4 — Estilos: escolher quem faz a limpeza

## Problema
A limpeza do ditado (`flow/cleanup.rs`) manda sempre o texto ao Claude
(`claude::complete`). O usuário quer escolher outro motor, como ChatGPT/Codex.

## Decisão
- Nova preferência `cleanupEngine` no `Prefs` do Flow, do tipo `SummaryEngine`
  (o mesmo enum dos resumos do Anotador). Padrão `claude` = comportamento atual.
  Opções no seletor: Claude, ChatGPT/Codex, Antigravity/Gemini e "Mesmo provedor do Chat".
  `off` existe no enum, mas não é oferecido: a limpeza liga e desliga pelo toggle.
- Claude continua no caminho atual (`claude::complete` com o modelo do ditado e
  os timeouts de 4 s na API / 10 s no relay). Os outros motores usam
  `chat::complete_summary`, a mesma cadeia dos resumos (OAuth, CLI, chave Google,
  chat ativo). Nenhum cliente HTTP novo.
- Motor não conectado: o texto cru é colado e a causa vai ao log, sem chamar motor.
- Limite total de 10 s para os motores de CLI/OAuth (`tokio::time::timeout`),
  porque a CLI do Codex e a do Gemini não aceitam timeout próprio aqui.
- Aviso da aba Estilos: texto de Claude fica como está (`t()`); para os outros
  motores, frases novas em pt-BR dizendo o destino do texto (nunca a voz).
- Antigravity/Gemini tem caminho pronto (CLI e chave Google), então entra.

## Arquivos
- `windows/src-tauri/src/flow/store.rs`: campo `cleanup_engine` (`cleanupEngine`), padrão Claude, teste.
- `windows/src-tauri/src/chat.rs`: `summary_ready(settings, engine)`, sem rede.
- `windows/src-tauri/src/flow/cleanup.rs`: `ready`, `wants_engine`, `finish`, `clean` recebe o motor e os ajustes.
- `windows/src-tauri/src/flow/mod.rs`: `process` lê o motor e os ajustes.
- `windows/src/core/flow.ts`, `windows/src/hub/shell.ts`: tipo e padrão.
- `windows/src/hub/pages/styles.ts`: seletor "Quem faz a limpeza" e aviso por motor.

## Interfaces
- `cleanup::clean(raw, style, app, words, engine, &Settings) -> String`
- `cleanup::ready(engine, &Settings) -> bool`
- `chat::summary_ready(&Settings, SummaryEngine) -> bool`

## Testes
- Rust: padrão `Claude` e leitura de `cleanupEngine` (com alias `codex`) em `store.rs`;
  `wants_engine` falso para "Do jeito que foi dito", "off" e texto curto;
  `finish` com erro, resposta estranha e resposta boa.
- Node: `tests/flow-editors.test.mjs`, trocar o motor grava `cleanupEngine` e muda o aviso.

## Fora deste item
- Mocks de rede por motor (não há costura de teste nos módulos de CLI/OAuth).
- Título e toggle "Limpar com o Claude" e a frase do cabeçalho continuam como estão (t()).

## Aceite
- Padrão idêntico ao atual; trocar o motor não muda o que é salvo fora de `cleanupEngine`.
- Falha, motor sem login ou tempo esgotado: o texto cru é colado, com causa no log.
- Estilo "Do jeito que foi dito" e motor "off" nunca chamam motor.
