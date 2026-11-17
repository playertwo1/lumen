# L8.1 — Ditado "segure uma tecla" inconsistente

Origem: relato do dono no notebook (Windows 11, tecla Alt da esquerda): "tem hora que funciona, tem hora que não".

## Problema (achado lendo o código, sem reproduzir no Windows)

1. **Up perdido deixa a lógica presa.** `KeyLogic::key_down` tratava todo Down com a fase `Holding` como repetição automática (`Holding => None`). Se o Up não chegasse, a escuta continuava até a próxima tecla, e o novo pressionar não fazia nada.
2. **Silêncio sem aviso.** `mod.rs::finish` descartava a fala (menos de 0,25 s de áudio ou pico abaixo de 0,004) só com `pill → idle`: sem mensagem e sem log. O tempo de abrir o microfone (`begin`) conta no tempo da tecla, mas não no áudio.
3. **Resultado vazio e modelo não pronto** (`process`) também voltavam a repouso calados.
4. **Ditado durante "pensando".** A flag `busy` era escrita e nunca lida. Ao terminar o ditado anterior, o worker punha a pílula em `idle` mesmo com um ditado novo ouvindo.

Hipóteses do roadmap conferidas no código: (1) o gancho nunca engole o Alt (`on_key` devolve `own = false` para modificadores) e `mask_menu` evita o menu; não há evidência de consumo pelo Windows. (3) `begin` confere só a presença dos arquivos (`dictation_ready`); o carregamento espera no `load_lock`, então não cai em "baixe o modelo". Confirmação no Windows fica para o relato real.

## Decisão

- `KeyLogic` guarda `held` e `last_down`. Um Down com a tecla marcada como presa e intervalo maior que `STUCK_GAP` (1,5 s) é Up perdido: nova ação `Action::Renew`, que cola o que foi dito até ali e começa outra escuta. A repetição automática (até 0,4 s entre Downs) continua ignorada.
- Pílula diz o motivo, em pt-BR direto: sem áudio, pouco áudio, nada entendido, modelo não pronto.
- `busy` vira contador `pending`. Ao fim de um ditado, a pílula só volta a repouso se nenhum outro ditado está em andamento e se ela ainda mostra "pensando" (`settle`).
- `coucou.log`: cada Down/Up que decide algo, Other/Escape que cancela, duração e pico da fala, microfone aberto, resultado da colagem. Sem texto ditado, sem nome de janela e sem tecla de digitação comum.

## Arquivos

- `windows/src-tauri/src/flow/logic.rs`: `Action::Renew`, `STUCK_GAP`, campos `held`/`last_down`, `key_down`, `key_up`, testes.
- `windows/src-tauri/src/flow/mod.rs`: `run` (Renew, log de decisão), `begin` (log), `finish` (feedback, `pending`), `settle`, `process` (feedback, log do resultado).

## Interfaces

- `Action::Renew`: "a última escuta é colada e uma nova começa".
- `logic::STUCK_GAP: f64 = 1.5`.
- `fn settle(app, pending: &AtomicUsize)`.

## Testes

- Up perdido seguido de novo Down: `Renew`, e o Up depois dá `Finish`.
- Autorepeat (30 Downs a cada 0,4 s): nenhuma ação nova.
- Alt+Tab: cancela, o Up seguinte não faz nada, e um novo Down começa um ditado normal.
- Os testes existentes (segurar, toque duplo, um toque, Esc, atalho) continuam passando.

## Aceite

- `cargo test --lib flow` passa (resultado no relatório).
- Limite conhecido: um Up perdido com nova tecla em menos de 1,5 s ainda é tratado como repetição até o próximo Up.
- Checkpoint real (dono): 20 ditados seguidos em 3 programas sem falha silenciosa. **Real: NOT_RUN.**
