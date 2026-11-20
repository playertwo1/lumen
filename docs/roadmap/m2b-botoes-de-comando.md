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

- [ ] Onde a barra aparece: na própria pílula (ao passar o mouse, como no ditado), na ilha, em atalho global, ou mais de um.
- [ ] Quais botões entram na primeira versão (candidatos das ferramentas de M2: volume, brilho, bloquear tela, abrir app, janelas, mídia).
- [ ] Se a escolha dos botões é fixa ou o dono monta a sua nos Ajustes (ordem, quais mostrar).
- Checkpoint M2b.1: lista final de botões e do lugar da barra, escrita neste arquivo.

### M2b.2 — Barra e um botão de ponta a ponta

- [ ] Barra de botões a partir de uma lista declarada (ícone, nome, ferramenta, argumentos), sem código por botão.
- [ ] Ligar um botão de risco baixo à ferramenta pelo registro da M1 e mostrar o resultado.
- [ ] Testar: ferramenta ausente ou pausada (pausa rápida) deixa o botão desligado, com a razão na dica; duplo clique não executa duas vezes.
- Checkpoint M2b.2: um clique faz o que o mesmo pedido no chat faria, e aparece no registro local.

### M2b.3 — Os botões da primeira versão

- [ ] Um botão por ferramenta escolhida na M2b.1; os de risco médio ou alto abrem o cartão de confirmação.
- [ ] Teclado e leitor de tela: todo botão alcançável por Tab, com nome e dica.
- [ ] Testar cada botão com a ferramenta funcionando e com ela falhando (erro claro, sem travar a barra).
- Checkpoint M2b.3: nenhuma ação de risco roda sem clique de confirmação.

### M2b.4 — Escolha dos botões (se decidida em M2b.1)

- [ ] Nos Ajustes, o dono escolhe e ordena os botões; a escolha fica no armazenamento local, sem segredo.
- [ ] Testar: lista vazia, botão de ferramenta que deixou de existir e restauração do padrão.
- Checkpoint M2b.4: a escolha sobrevive a reiniciar o Lumen.

### M2b.5 — Fechamento

- [ ] Atualizar `windows/README.md` e o Estado no índice.
- Checkpoint M2b: comandos de uso frequente saem com um clique, sem abrir o chat, com as mesmas proteções.
