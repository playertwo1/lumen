> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../ROADMAP-FORK.md). Regras gerais e das fases M: [AGENTS.md](../../AGENTS.md).

## Fase M2b — Botões de comando (M)

Objetivo: dar comandos sem abrir o chat. Uma barra própria de botões, no mesmo estilo dos botões do ditado (Ditar, Gravar chamada, Abrir o Lumen), mas com outra escolha: cada botão dispara uma ferramenta do assistente com um clique.

Depende de: M1 (registro de ferramentas, permissões por risco, cartão de confirmação) e das ferramentas de M2 que cada botão usar. Começa logo depois da M2.

Regras desta fase:
- Um botão chama a **mesma ferramenta** que o chat chamaria, pelo mesmo registro e pelas mesmas permissões. Não existe caminho paralelo.
- Ação de risco médio ou alto continua pedindo o cartão de confirmação com clique. O botão nunca aprova por conta própria.
- O registro local anota o clique como qualquer outro comando.
- Visual novo segue o padrão dos botões do ditado (`pillButton`, `pbtn`); o que já existe não muda de aparência.
- Movimento segue [`core/motion.ts`](../../windows/src/core/motion.ts): sem animação decorativa com `prefers-reduced-motion`.

### M2b.1 — Decisões do dono (antes de codar)

- [x] Onde a barra aparece: **na própria pílula**, ao passar o mouse. Um botão novo na barra do ditado (raio) troca para a barra de comandos; "voltar" retorna ao ditado.
- [x] Botões da primeira versão: **volume e mudo, mídia, brilho, organizar janelas e bloquear tela** (as ferramentas `audio`, `midia`, `brilho`, `janelas` e `bloquear_tela` da M2).
- [x] A escolha é **do dono, nos Ajustes** (escolher e ordenar); entra a M2b.4.
- Checkpoint M2b.1: decisões do dono em 2026-11-21, registradas aqui.

### M2b.2 — Barra e um botão de ponta a ponta

- [x] Barra de botões a partir de uma lista declarada em Rust (`mochi/buttons.rs`: ícone, nome, ferramenta, argumentos); a página só envia o `id` do botão, nunca ferramenta nem argumentos.
- [x] Primeiro botão, "próxima faixa" (`midia`, risco baixo), pelo `invoke_tool` da M1; o resultado aparece no balão da pílula.
- [x] Testar: ferramenta ausente ou pausada deixa o botão desligado, com a razão na dica; duplo clique não executa duas vezes. *(Sintético: testes Rust em `buttons.rs`.)*
- Checkpoint M2b.2: um clique faz o que o mesmo pedido no chat faria, e aparece no registro local. *(Implementado em 2026-11-21. **Real, com a pílula e um player tocando: NOT_RUN**.)*

### M2b.3 — Os botões da primeira versão

- [x] Doze botões: mídia (anterior, tocar, pausar, próxima), volume (−10, +10, mudo), brilho (30% e 100%; a ferramenta não lê o nível, então não há passo), janelas (lado a lado de Codex, Claude Code e Antigravity já abertos; desfazer) e bloquear tela, que abre o cartão de confirmação.
- [ ] Teclado e leitor de tela: todo botão alcançável por Tab, com nome e dica. *(Botões têm `aria-label` e `aria-disabled`; falta conferir o Tab com a pílula real.)*
- [x] Testar a montagem de cada pedido (volume nos limites, mudo, janelas com menos de duas abertas) e o schema de todos. *(Sintético, `mochi/buttons.rs`. Um teste achou e corrigiu um deadlock no bloqueio de duplo clique.)*
- Checkpoint M2b.3: nenhuma ação de risco roda sem clique de confirmação. *(Implementado em 2026-11-21. **Real, com a pílula: NOT_RUN**.)*

### M2b.4 — Escolha dos botões (se decidida em M2b.1)

- [x] Em Ditado → Ajustes ("Botões de comando"), o dono liga, desliga e reordena os botões; a escolha é a preferência `commandButtons` do Flow (`null` = todos), sem segredo. O Rust ignora id desconhecido e repetido.
- [x] Testar: lista vazia, id que deixou de existir, repetido e ordem. *(Sintético, `buttons.rs`. A restauração do padrão grava `null`.)*
- Checkpoint M2b.4: a escolha sobrevive a reiniciar o Lumen. *(Implementado em 2026-11-21; a preferência é gravada como as outras do Flow. **Real, reiniciando o Lumen: NOT_RUN**.)*

### M2b.5 — Fechamento

- [x] Atualizar `windows/README.md` e o Estado no índice.
- Checkpoint M2b: comandos de uso frequente saem com um clique, sem abrir o chat, com as mesmas proteções. *(Implementada em 2026-11-21; o teste real do dono está em `STATE-FORK.md`: **NOT_RUN**. A etapa só é arquivada depois dele.)*
