# Decisão — ícone do Lumen: cristal luminoso

**Data:** 2026-11-16  
**Estado:** cristal integrado sinteticamente ao app Windows; validação visual real pendente.

## Escolha

O proprietário gostou das duas animações exploratórias e escolheu o segundo conceito — o cristal violeta com núcleo dourado — como direção do ícone do Lumen. Ele substitui a Bússola escolhida originalmente na L1.1. A estrela luminosa fica guardada como alternativa, mas não é a direção selecionada para o projeto.

Esta decisão cobre o ícone e sua animação de abertura. Ela não altera por si só os personagens, sons ou outras partes da identidade já existentes.

## Artefatos de referência

- [GIF da estrela luminosa](../../output/lumen-icon-animations/estrela-luminosa.gif) e [SVG animado](../../output/lumen-icon-animations/estrela-luminosa.svg).
- [GIF do cristal Lumen](../../output/lumen-icon-animations/cristal-lumen.gif) e [SVG animado](../../output/lumen-icon-animations/cristal-lumen.svg).

## Implementação

Os quatro arquivos são referências visuais salvas no Google Drive. A integração usa o cristal no ícone do pacote Windows e uma versão simplificada na bandeja. O GIF aprovado aparece na primeira abertura do Hub em cada sessão; com `prefers-reduced-motion`, o Hub mostra a imagem final estática. A versão `0.7.10` também notifica o Windows para atualizar o ícone do atalho Lumen no Menu Iniciar quando o app inicia. O splash nativo do Windows continua sendo estático; a animação de 0,8 s ocorre dentro do Hub. A spec e o plano iniciais são `docs/superpowers/specs/2026-11-16-l1-icone-cristal-app.md` e `docs/superpowers/plans/2026-11-16-l1-icone-cristal-app.md`; a correção de abertura está em `docs/superpowers/specs/2026-11-16-l1-icone-cristal-abertura-v0710.md` e `docs/superpowers/plans/2026-11-16-l1-icone-cristal-abertura-v0710.md`. Validação sintética e preflight Windows passaram; a confirmação visual após instalar/atualizar no Windows continua pendente.
