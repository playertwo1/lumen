# L1 — Cristal na abertura do Lumen

## Evidência e objetivo

Na instalação 0.7.9, a Busca do Windows ainda mostra a bússola embora o atalho Lumen aponte para `lumen.exe` 0.7.9, cujo recurso de ícone já é o cristal. A tela transitória antes da janela do app é do Shell do Windows e só exibe um ícone estático; a animação precisa aparecer na primeira janela real do Lumen.

Corrigir a atualização visual do atalho do Windows e mostrar a animação cristal aprovada ao abrir o Hub do Lumen; publicar como `0.7.10`.

## Escopo e aceite

- Ao iniciar o app, notificar o Shell de que o atalho Lumen do menu Iniciar mudou, para que a Busca e a tela transitória de abertura recarreguem o recurso de ícone do executável.
- Ao exibir o Hub pela primeira vez, tocar uma vez o GIF cristal de 0,8 s sobre a janela e então liberar a interface. Com `prefers-reduced-motion`, mostrar o PNG final estático pelo mesmo breve período.
- Não repetir a vinheta quando o Hub for escondido e exibido novamente durante a sessão.
- Manter a identidade do executável/atalho e os dados existentes. A release deve ser upgrade compatível com 0.7.9.
- Rodar verificações de versão/What's New, typecheck, suíte Node, Rust, build/pack, upgrade sintético MSI/NSIS e Defender conforme checklist de release.

## Arquivos e superfícies

- `windows/src/hub/shell.ts` e `windows/src/hub/hub.css` para a abertura animada do Hub.
- `windows/src-tauri/src/tray.rs` para notificar o Shell do atalho atual.
- Versões, notas, changelog, README, workflow e checklist da release `0.7.10`.

## Riscos e reversão

O cache do ícone da Busca e da tela transitória é do Windows. A notificação `SHCNE_UPDATEITEM` solicita a atualização do atalho; validar visualmente depois de instalar a release. A tela transitória do Windows não aceita a animação do app. Reverter o commit restaura a abertura e o atalho anteriores, sem migração de dados.
