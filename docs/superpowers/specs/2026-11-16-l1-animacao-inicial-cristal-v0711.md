# L1 — Animação do cristal na abertura do Lumen

## Evidência e objetivo

Ao iniciar o Lumen 0.7.10, a ilha exibe a arte antiga de abertura. O executável, o atalho e o ícone da bandeja já usam o cristal; a saudação visível na ilha ainda passa pelo desenho legado.

Na saudação da build Windows Lumen, mostrar a animação GIF do cristal por 0,85 s e então manter a imagem PNG final até a saudação terminar. Com `prefers-reduced-motion`, mostrar a imagem estática desde o início. O Coucou original continua com sua saudação atual.

## Escopo e aceite

- A saudação da ilha Lumen usa os assets de cristal já incluídos no app.
- O GIF toca uma vez; a tela permanece no cristal final até o encerramento da saudação.
- Movimento reduzido usa a imagem estática.
- Nenhum asset, animação ou saudação das outras plataformas é alterado.
- Validar typecheck, testes, build, Rust, ícones, empacotamento e upgrade sintético `0.7.10` → `0.7.11`; revisar visualmente a execução local.
- A confirmação visual após a instalação publicada continua pendente do proprietário.

## Arquivos e risco

- `windows/src/mochi/greeting.ts`: trocar apenas o desenho da saudação da build Lumen e limitar a reprodução do GIF.
- Versões e materiais de release para `0.7.11`.
- Sem migração de dados. Se a arte não aparecer, a saudação pode ficar vazia durante o primeiro carregamento do asset; a validação local precisa conferir o início frio.
