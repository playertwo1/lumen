> Parte do roadmap do fork. Índice e ordem: [ROADMAP-FORK.md](../ROADMAP-FORK.md). Regras gerais e das fases M: [AGENTS.md](../../AGENTS.md).

## Fase M2 — Mochi: controle nativo do Windows (M)

Objetivo: comandos de rotina do Windows por APIs nativas, sem PowerShell montado por texto e sem Agent Desktop.

Depende de: Checkpoint M1.

Cada subfase entrega uma ferramenta completa: schema, risco, execução, verificação, testes e uma frase de teste manual no chat.

### M2.1 — Volume e mudo (risco baixo)

- [x] Ler e definir volume master (0–100) e mudo pela API de áudio.
- [x] Depois de agir, ler o estado e devolvê-lo.
- [x] Testar: valor fora da faixa recusado; sem dispositivo de áudio vira erro claro.
- Checkpoint M2.1: "volume 20%" e "tira o som" funcionam e relatam o valor final. *(Sintético em 2026-11-17: ferramenta `audio` em `mochi/tools/audio.rs`, sondagem em `mochi/host.rs`; suíte Rust `864 passed; 0 failed`. A pausa rápida passou a cortar também os pedidos pelo MCP. **Real no chat: NOT_RUN**.)*

### M2.2 — Brilho (risco baixo)

- [x] Brilho da tela interna do notebook pela interface nativa.
- [x] Monitor externo só quando o monitor responder por DDC/CI; caso contrário, responder "indisponível neste monitor".
- [x] Testar em desktop sem tela interna: erro honesto, nenhum valor inventado. *(Sintético, com backend falso.)*
- Checkpoint M2.2: brilho muda onde é suportado; hardware testado fica anotado. *(Sintético em 2026-11-17: ferramenta `brilho` em `mochi/tools/brightness.rs` (WMI `root\wmi` e DDC/CI VCP 0x10), sondagem com cache de 30 s em `mochi/host.rs`; suíte Rust `877 passed; 0 failed`. **Real no notebook e em monitor externo: NOT_RUN**.)*

### M2.3 — Listar e abrir aplicativos (risco baixo)

- [x] Listar aplicativos instalados.
- [x] Abrir somente um aplicativo escolhido dessa lista; nunca um caminho ou comando vindo do pedido.
- [x] Testar: nome ambíguo pede escolha; aplicativo ausente vira erro; nome com `;` ou `&` não executa nada.
- Checkpoint M2.3: "abre o VS Code" abre o aplicativo certo. *(Sintético em 2026-11-17: ferramenta `apps` em `mochi/tools/apps.rs`, varredura do Menu Iniciar compartilhada com a voz (`voice::start_menu_shortcuts`); desinstaladores nunca são listados nem abertos; atalho sem `.exe` conferível (Store) abre mas relata "não dá para confirmar"; suíte Rust `895 passed; 0 failed`. **Real no chat: NOT_RUN**.)*

### M2.4 — Janelas (risco baixo)

- [x] Listar janelas; mover, redimensionar, minimizar e encaixar lado a lado.
- [x] Guardar as posições anteriores e oferecer "desfazer" da última organização.
- [x] Tratar monitores com escalas e origens diferentes; janela elevada (administrador) vira erro claro. *(Em minimizar o Windows não informa a recusa; vira "a janela não ficou como o pedido".)*
- [ ] Com um só monitor, registrar HARDWARE INDISPONÍVEL para o teste de dois monitores.
- Checkpoint M2.4: "organize as janelas para programar" arruma e "desfaz" restaura. *(Sintético em 2026-11-17: ferramenta `janelas` em `mochi/tools/arrange.rs`; ids da listagem conferidos com o processo antes de agir; desfazer em memória; suíte Rust `912 passed; 0 failed`. **Real no chat: NOT_RUN**.)*

### M2.5 — Mídia (risco baixo)

- [ ] Tocar, pausar, próxima e anterior, reutilizando a sessão de mídia que a pílula de música já usa.
- [ ] Sem player ativo: responder isso, sem abrir nada.
- Checkpoint M2.5: os comandos agem no player ativo e em nenhum outro.

### M2.6 — Energia (risco médio e alto)

- [ ] Bloquear a tela: risco médio.
- [ ] Suspender, reiniciar e desligar: risco alto, clique toda vez, com aviso de trabalho não salvo.
- [ ] Testar: negar não faz nada; timeout não faz nada.
- Checkpoint M2.6: nenhuma ação de energia ocorre sem clique; o registro mostra a decisão.

### M2.7 — Fechamento

- [ ] Escolher 10 comandos de rotina; executar cada um 3 vezes pelo chat.
- [ ] Testar: retomar da suspensão, aplicativo ausente, dispositivo de áudio trocado.
- [ ] Atualizar `windows/README.md` e o Estado no índice.
- Checkpoint M2: 10 comandos com sucesso repetível; toda falha aparece como falha, com motivo.
