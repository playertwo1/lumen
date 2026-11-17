# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- **Usuário principal (confirmado):** proprietário usando **Windows 11** no notebook/PC no dia a dia, em conjunto com o app companheiro **Segunda Mente** (Android).
- **[P1 — aguardando confirmação rápida se também segue A]:** uso pessoal diário sem apelo de marketing nem fluxo genérico de onboarding (ou considerar outros usuários técnicos do fork no GitHub).

## Product Purpose

Lumen é o cliente desktop (Tauri 2 + WebView2 + Rust/TypeScript) da **Segunda Mente** e central local de produtividade para Windows:
- **Ilha flutuante (topo da tela):** captura rápida (`Ctrl+Alt+K` para tarefas), acompanhamento de agentes de IA/CLI, chat multimodal, ditado por voz (`Parakeet`), avisos acionáveis e animações expressivas (P1–P3) dos personagens originais do Lumen.
- **Hub (janela principal — P2 confirmado: A):** área de trabalho real no desktop para revisar, organizar e editar **Tarefas**, **Anotador** (com personas) e **Mail** (MailCraft / Outlook clássico), além de reunir Ditado, Estatísticas, Dicionário, Trechos, Estilos e Configurações (incluindo pareamento QR com o Android).

## Positioning

- **100% local-first e privado:** armazenamento local em disco (`Todo/todo.json`, `Todo/sync.json`, notas e histórico), segredos somente no cofre do Windows (Credential Manager), zero telemetria e nenhuma rede automática fora do que o usuário configurou.
- **Controle explícito e segurança por design:** e-mails nunca são enviados sozinhos, permissões exigem clique explícito, tarefas/datas ambíguas nunca são gravadas sem confirmação e o relay E2EE usa chave que nunca sai em claro para o servidor.
- **Objetivo do redesign (P3 confirmado: A + B):** tornar a gestão diária de **Tarefas e Anotador** no Hub fluida, rápida e agradável em janela larga, eliminando o ruído visual para acompanhar a mesma qualidade visual das demais telas do ecossistema e da **Segunda Mente** (Android).

## Operating Context

- Executado no Windows em **duas superfícies muito diferentes**:
  1. **Ilha flutuante:** faixa compacta no topo da tela e cartões rápidos (~640 px de largura máxima em estados expandidos), com passagem de clique fora da forma ativa.
  2. **Hub e Ajustes:** janela desktop tradicional redimensionável (frequentemente maximizada ou em janela larga), operada com mouse e teclado, que hoje mistura classes/ritmo da Ilha compacta dentro de telas largas.
- Idioma ativo da Fase L: **pt-BR**.

## Capabilities and Constraints

- **Fluxos ativos principais no Hub (`Flow` / `Notch` / `Mail`):** Ditado, Anotador (`notes`), Tarefas (`tasks`), Estatísticas, Dicionário, Trechos, Estilos, Ajustes, Geral, Abas ativas, Agentes, Chat, `OK Lumen`, Integrações, Celular Android (pareamento QR), Atalhos e Mail.
- **Restrições duráveis do projeto:**
  - Não alterar comportamentos de segurança/privacidade: "IA propõe, usuário confirma" (datas ambíguas em tarefas), "nada enviado sozinho" (e-mails/permissões), sem telemetria.
  - Não quebrar contratos de eventos IPC Tauri (`"task-reminder"`, `"task-dismiss"`, `"task-sync-state"`, etc.) nem IDs de pílulas publicados.
  - Assets/personagens próprios do Lumen (`LICENSE-ASSETS.md`): proibido reutilizar nome, ícone, sons ou personagem Mochi/Coucou do upstream; o ícone do app é a Bússola e os personagens da cena do Lumen são originais em Canvas.

## Brand Commitments

- **Nome:** Lumen.
- **Identidade de marca:** ícone da Bússola + trupe de personagens originais em Canvas (já aprovados visualmente em L1.4a–L1.4d, mantendo distinção do Coucou/Mochi original).
- **Ecossistema irmão:** Segunda Mente (Android, fundo AMOLED `#000000`, texto marfim `#F3EEE6`, acento âmbar/cobre `#E99A3D`, estilo editorial calmo em `segundamente/DESIGN.md`). **Decisão confirmada (D1 = A):** o desktop herda diretamente essa linguagem visual e tipografia (`Inter` + fallbacks nativos) adaptadas ao Windows, preservando o shell base e o acabamento das telas originais do Hub.

## Evidence on Hand

- Código-fonte real em `windows/src/` (`hub/`, `views/`, `island/`, `lumen/`, `settings/`) e backend Rust em `windows/src-tauri/src/`.
- Captura real do usuário da tela **Tarefas** em janela maximizada (mostrando o cartão "Confirmar proposta antes de salvar", barra de status "Sincronizado", chips/selects compactados, grande área vazia à direita e recorte/aperto no rodapé da barra lateral).
- Documentação de arquitetura e decisões do fork em `AGENTS.md`, `docs/ROADMAP-FORK.md`, `docs/STATE-FORK.md`, as etapas em `docs/roadmap/` e `docs/arquivadas/etapas/` e `LICENSE-ASSETS.md`.

## Product Principles

1. **Clareza operacional acima de enfeite:** quem abre o Hub quer agir rápido (criar/confirmar tarefa, ler nota, configurar agente/pareamento) com leitura imediata.
2. **IA propõe, humano decide:** confirmações explícitas precisam de hierarquia clara, sem virar formulário confuso ou bloqueado.
3. **Privacidade visível sem ruído:** status local vs sincronizado deve inspirar confiança em um olhar, sem poluir toda linha repetidamente.
4. **Respeito ao contexto de cada superfície:** a Ilha pede economia extrema; a janela do Hub pede escala, proporção e conforto de desktop.

## Accessibility & Inclusion

- Suporte explícito a `prefers-reduced-motion` nas animações de Canvas (`CompassEngine` e personagens).
- Contraste legível em tema escuro no Windows, navegação clara por teclado (`Ctrl+Alt+K`, `Ctrl+P`, foco visível) e `color-scheme: dark` para controles nativos sem popups brancos quebrados.
- **[Em aberto no questionário]:** preferência por escala de fonte ajustável / modo de densidade (confortável vs compacto).
