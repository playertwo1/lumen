# Plano — Ícone cristal do Lumen

1. Adaptar o gerador existente para desenhar o cristal facetado violeta e núcleo dourado nos formatos atuais, e exportar um PNG transparente simplificado para a bandeja.
2. Usar o PNG da bandeja no `TrayIconBuilder` e manter a troca temporária para azul enquanto há atualização disponível.
3. Adicionar ao Hub um overlay de abertura de 0,8 s baseado no cristal escolhido, acionado só na primeira exibição da sessão, com estado final imediato em movimento reduzido.
4. Regenerar assets e executar os comandos de validação do projeto para TypeScript, build e Rust.
5. Revisar diff, atualizar o estado da Fase L sem fechar validação real, e registrar um commit da subfase.
