# L1 — Ícone cristal do Lumen

## Objetivo

Aplicar o cristal violeta com núcleo dourado escolhido pelo proprietário à identidade do app Windows: ícone de aplicativo e bandeja, mais uma animação de abertura curta de 0,8 s.

## Escopo e aceite

- Substituir a bússola gerada em `windows/src-tauri/icons/lumen` pelo cristal aprovado; manter os tamanhos e formatos consumidos pelo Tauri.
- Criar uma versão simplificada e transparente para a bandeja, legível em 32 px, sem o tile arredondado do ícone de aplicativo.
- Ao exibir o Hub pela primeira vez na sessão, animar por 0,8 s a formação do cristal e o brilho do núcleo; não atrasar a interação e respeitar `prefers-reduced-motion` mostrando o cristal final estático.
- Manter a aparência e o comportamento da ilha/personagem, do menu da bandeja e do indicador azul de atualização.
- Validar gerador, TypeScript/build e testes Rust aplicáveis. A validação visual no Windows real continua necessária.

## Arquivos e superfícies

- `windows/scripts/gen-icons.mjs`; recursos gerados em `windows/src-tauri/icons/lumen/`.
- `windows/src-tauri/src/tray.rs` para o asset de bandeja.
- `windows/src/hub/shell.ts` e `windows/src/hub/hub.css` para a animação de abertura do Hub.
- `windows/src-tauri/tauri.windows.conf.json` já consome os arquivos gerados.

## Riscos e reversão

O ícone é recurso do pacote e o Windows pode manter cache visual; confirmar a aparência após reinstalar/atualizar localmente. Reverter o commit restaura a bússola e a experiência anterior sem migração de dados.
