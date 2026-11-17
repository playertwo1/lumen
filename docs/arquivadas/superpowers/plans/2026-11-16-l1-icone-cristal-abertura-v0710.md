# Plano — Cristal na abertura do Lumen

1. Exibir o GIF aprovado uma vez quando o Hub aparecer pela primeira vez, com PNG para movimento reduzido; deixar o Shell do Windows usar o ícone estático do atalho durante sua própria tela transitória.
2. No setup Rust do Windows, localizar os atalhos Lumen do menu Iniciar do usuário e do sistema e enviar `SHCNE_UPDATEITEM` para cada um existente.
3. Preparar a versão 0.7.10 em manifests, changelog, What's New, README e texto de publicação.
4. Rodar testes sintéticos e build local; gerar pacote, validar MSI/NSIS, executar preflight GitHub e Defender, publicar a release versionada e atualizar `windows-latest` conforme o checklist autorizado.
5. Commitar e integrar na `main`, enviar commits e tag ao `origin`; registrar evidência e deixar a verificação visual após instalação aberta.
