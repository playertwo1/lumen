# L8.2 — Mãos livres (toque duplo) não funciona

Origem: relato do dono: "mãos livres não está funcionando". A tela mostra "Alt da esquerda Alt da esquerda" e "Mãos livres com um toque" desligado.

## Problema (achado lendo o código, sem reproduzir no Windows)

1. **Repetição finaliza a mãos livres.** Depois do toque duplo (`Lock`), se a segunda tecla fica pressionada além da repetição automática do Windows (cerca de 0,5 s), os Downs de repetição chegam em `Phase::Locked`, e `key_down` respondia `Finish`: a fala acabava sozinha. Em `Idle`, a mesma repetição, depois de um Finish com a tecla ainda pressionada, iniciava um ditado novo.
2. **Janela do toque duplo curta.** `DOUBLE_TAP_WINDOW` (0,40 s) é contada do Up do primeiro toque. O padrão do Windows para duplo clique é 500 ms entre cliques (Down a Down). Com toques de cerca de 0,1 s, o intervalo Up → Down pode passar de 0,40 s, e o segundo toque vira um ditado novo ou nada.
3. **Pílula em mãos livres.** Já existe: a fase `locked` usa o visual `handsFree` (`windows/src/pill/main.ts`), com o botão ✓ "Colar · ou pressione <tecla>". Não foi alterada.

Os intervalos reais do dono (medidos com o toque duplo no notebook) ainda não foram coletados; o checkpoint real fica para ele.

## Decisão

- `DOUBLE_TAP_WINDOW` passa de 0,40 s para 0,50 s, igual ao padrão do Windows para duplo clique.
- Em `Locked` e `Idle`, um Down que é repetição da tecla (`held` e intervalo curto, vindo da L8.1) não faz nada. Só um Down depois de solta finaliza (`Locked`) ou começa (`Idle`).
- Sem mudança em `mod.rs`, `os_windows.rs` nem na pílula.

## Arquivos

- `windows/src-tauri/src/flow/logic.rs`: `DOUBLE_TAP_WINDOW`, `key_down` (ramos `Idle` e `Locked`), testes.

## Testes (sequências temporizadas em `KeyLogic`)

- Toque duplo dentro da janela (segundo toque 0,45 s depois do Up) trava. Fora (0,6 s) não trava.
- Toque duplo com "um toque" ligado: o primeiro toque curto já trava, o próximo finaliza.
- Toque duplo com a segunda tecla segurada e repetindo: não finaliza; ao soltar, o toque seguinte finaliza.
- Finish após Lock: o Down que finaliza, com a tecla segurada e repetindo, não reinicia um ditado.
- Os testes existentes (segurar, toque único, Esc, atalho, Alt+Tab) continuam passando.

## Aceite

- `cargo test --lib flow` passa (resultado no relatório).
- Checkpoint real (dono): toque duplo inicia, fala longa de 30 s e novo toque cola; o mesmo no modo de um toque. **Real: NOT_RUN.**
