# Roadmap do Lumen (fork Windows do Coucou)

Atualizado em 2026-11-27. Este arquivo é só o índice: diz onde cada etapa está e o que ela faz ou fez. O detalhe (caixas, checkpoints e evidências) fica no arquivo de cada etapa. Regras: [AGENTS.md](../AGENTS.md). Estado e pendências: [STATE-FORK.md](STATE-FORK.md).

## Abertas e a fazer — `docs/roadmap/`

Em ordem de execução. Uma etapa por vez; etapas pausadas só voltam com decisão do dono.

| Ordem | Etapa | Estado | Resumo |
|---:|---|---|---|
| 1 | [L8 — Ajustes do uso real](roadmap/l8-ajustes-uso-real.md) | **Da vez.** Implementada na `0.7.12`; validação real pendente | Corrige o que incomodou no uso das 0.7.8–0.7.10: ditado "segure uma tecla", mãos livres, Anotador → Tarefa, escolha do motor da limpeza, Orca em Agentes e Abas ativas, WhatsApp pela ponte do Orbit (só leitura). |
| 2 | [M2 — Controle nativo do Windows](roadmap/m2-windows-nativo.md) | **Em andamento** (spec e plano), em paralelo à validação da L8 por decisão do dono (2026-11-17) | Volume, brilho, apps, janelas, mídia e energia por APIs nativas, sem PowerShell montado por texto. |
| 2b | [M2b — Botões de comando](roadmap/m2b-botoes-de-comando.md) | Implementada na `main` (2026-11-21); teste real pendente | Barra de botões próprios, no estilo do ditado mas com outra escolha, que dispara comandos do assistente com um clique, sem abrir o chat. |
| 3 | [M3 — Obsidian](roadmap/m3-obsidian.md) | Pendente | Buscar, citar e capturar notas no cofre lendo os `.md` direto, sem plugin. |
| 4 | [M4 — Voz ligada às ferramentas](roadmap/m4-voz.md) | Pendente | Pedido falado chega às ferramentas de M2/M3, reaproveitando a voz que já existe; confirmação continua sendo clique. |
| 5 | [M5 — Computer use](roadmap/m5-computer-use.md) | Pendente | Operar a interface de apps pelo Agent Desktop (acessibilidade) quando não houver API nativa. |
| 6 | [M6 — Automação web](roadmap/m6-web.md) | Pendente | Navegar e ler páginas pelo Agent Browser, em perfil isolado e só em sites permitidos. |
| 7 | [M7 — Android](roadmap/m7-android.md) | Pendente | Pedir e aprovar ações do assistente pelo celular, pela ligação que já existe. |
| 8 | [M8 — Rotinas](roadmap/m8-rotinas.md) | Pendente | Sequências salvas de ferramentas ("modo programar"), aprovadas pelo dono. |
| 9 | [M9 — Memória, gateway e observabilidade](roadmap/m9-memoria-gateway.md) | Pendente | Avaliar e, se valer, adicionar memória por significado, LiteLLM e Langfuse. |
| 10 | [M10 — Release do assistente](roadmap/m10-release.md) | Pendente | Conferência final de segurança, recursos, documentação e instalação limpa antes de publicar. |
| — | [L6.5 — Fotos nas tarefas](roadmap/l6-5-fotos-tarefas.md) | A fazer depois; depende do contrato v1.1 da Segunda Mente | Levar ao Lumen as fotos que as tarefas ganharam no celular, sem perder anexos desconhecidos. |
| — | [L9 — Ponte com a placa ESP32-S3 AMOLED](roadmap/l9-placa-esp32.md) | Implementada na `main` (2026-11-27). Pareamento, tarefas, relay e mídia substituídos pela L10 (a placa não fala mais esse protocolo desde 2026-11-27); voz e painel seguem valendo | Pareamento por 6 dígitos, ponte de voz (STT/TTS WAV), sync de tarefas (LAN e relay AES-256-GCM), mídia do Windows e painel opcional de cota/agentes para a placa. |
| — | [L10 — Placa por MQTT](roadmap/l10-placa-mqtt.md) | Em andamento (2026-11-28); L10.1–L10.5 implementadas na `main`, falta L10.6 com broker real | Lumen, placa e celular juntos por um broker MQTT cifrado, dentro e fora de casa; substitui o pareamento de 6 dígitos, o `/v1/tasks/sync`, a sala `/v1/bridge` e o `/v1/media` da L9. Voz pela rede local e painel de uso continuam. |
| — | [L11 — Agentes e cotas para a placa](roadmap/l11-agentes-vibepulse.md) | Em andamento (2026-11-28); L11.1, L11.2 e L11.3 concluídas | Ideias do VibePulse (MIT) em Rust: monitor de agentes com modelo e esforço, segurar perguntas e permissões do Claude Code e do Codex até a placa ou o celular responderem, previsão de ritmo e Max Tracker; entrega pela rede local e pelo MQTT. |
| — | [Fase 0 — Validar o que já existe](roadmap/fase-0-validacao.md) | Pausada (D-053) | Validação real dos recursos do upstream 0.6 no Windows; faltam os registros por cenário. |
| — | [Fase 1 — Recursos do Mac](roadmap/fase-1-recursos-mac.md) | Pausada (D-053) | Trazer recursos do Mac: arrastar sobre janela, anexar janela da frente, captura de tela, "sente o PC". |
| — | [Fase 2 — Melhorias do Windows](roadmap/fase-2-melhorias-windows.md) | Pausada (D-053) | Notificações nativas, modo jogo, custo por sessão, pílula do Git, aba do terminal, histórico, lançador, vigia de builds e outras. |
| — | [Fase 2B — Paridade entre agentes](roadmap/fase-2b-paridade-agentes.md) | Pausada (D-053) | Claude, Codex, Antigravity e Hermes com as mesmas funções; herdr e Orca como ambientes. |
| — | [Fase 2C — Pílulas e posição livre](roadmap/fase-2c-pilulas-posicao.md) | Pausada (D-053) | Escolher pílulas visíveis, Grok Bot como agente e arrastar a ilha no topo. |
| — | [Parqueado](roadmap/parqueado.md) | Sem escopo | Ideias que só voltam com escopo e decisão do dono (Segunda Mente no Windows, ponte Notetaker → Drive, conectores, rotina por demonstração, ideias do MyBrain, do Speakr e do Leon) e o que foi rejeitado. |
| — | [Checklist de release](roadmap/release-checklist.md) | Procedimento | Passos para publicar uma versão Windows, só com pedido explícito. |

As fases M vêm depois da L8 e seguem a ordem M1 → M10; uma não começa antes do fechamento da anterior, salvo onde o arquivo dela disser. Origem das fases M: [questionário de 2026-11-09](questionario/2026-11-09-mochi-assistant-roadmap.md).

## Concluídas e arquivadas — `docs/arquivadas/etapas/`

| Etapa | Fechada em | Resumo do que fez |
|---|---|---|
| [L0 — Fechar o que estava quase pronto](arquivadas/etapas/l0-fechar-quase-pronto.md) | 2026-11-13 | Inventário das fases 0, 1 e 2: fechou o que tinha evidência e pausou o resto (D-053). |
| [L1 — Renomear para Lumen](arquivadas/etapas/l1-renomear-lumen.md) | 2026-11-15 | Nome, identifier e migração do Coucou para o Lumen; personagens originais por agente em Canvas; ícone cristal (até a `0.7.13`). |
| [L2 — Login do ChatGPT](arquivadas/etapas/l2-login-chatgpt.md) | 2026-11-14 | Sign in with ChatGPT (PKCE) e chat pelo plano ChatGPT, sem chave paga. |
| [L3 — Anotador](arquivadas/etapas/l3-anotador.md) | 2026-11-15 | Notas por voz ou texto, resumo e comentários das personas sem duplicar. |
| [L4 — Tarefas](arquivadas/etapas/l4-tarefas.md) | 2026-11-15 | Lista de tarefas no Hub, captura por voz e atalho com confirmação, avisos Toast com Adiar/Concluir. |
| [L5 — E-mail pessoal](arquivadas/etapas/l5-email-pessoal.md) | 2026-11-15 | MailCraft resume a caixa e prepara rascunho, nunca envia; Outlook clássico como opção. |
| [L6 — Sync com a Segunda Mente](arquivadas/etapas/l6-sync-segunda-mente.md) | 2026-11-15 | Caixa de correio no Worker, pareamento QR, cifra ponta a ponta e sync de tarefas com o celular. |
| [L7 — Google](arquivadas/etapas/l7-google.md) | 2026-11-15 | Login Google, backup semanal no Drive com restauração verificada e eventos no Calendar com confirmação. |
| [M1 — Fundação do assistente](arquivadas/etapas/m1-fundacao.md) | 2026-11-17 | Pedido → cérebro → ferramenta → resultado com o `ping`: registro de ferramentas, permissões por risco, cartão de confirmação, registro local, servidor MCP local e pausa rápida. Parte do teste real dispensada pelo dono. |
| [M0 — Descoberta e licenças](arquivadas/etapas/m0-descoberta.md) | 2026-11-15 | Licenças, cérebro por provedor e provas de conceito do assistente, sem código de produto. |
| [Fase 3 — Identidade própria](arquivadas/etapas/fase-3-identidade.md) | 2026-11-13 | Decidida e absorvida pela L1. |
| [0.4 — Hook do Claude Code e Terminal](arquivadas/etapas/fase-0-4-hook-claude-terminal.md) | 2026-11-09 | Permitir, Negar e sem clique validados na ilha, com o foco da aba certa. |
| [1.2 — Arquivo → rascunho no Outlook](arquivadas/etapas/fase-1-2-rascunho-outlook.md) | 2026-11-11 | Soltar arquivo abre rascunho no Outlook clássico com anexo, sem enviar. |
| [1.3 e 1.4 — Cripto e Cal.com](arquivadas/etapas/fase-1-3-e-1-4-descartadas.md) | 2026-11-11 | Descartadas pelo dono. |

Também em `docs/arquivadas/`: o [STATE antigo](arquivadas/state/state-ate-2026-11-17.md), o [registro de releases](arquivadas/releases/releases-windows.md), a [decisão do ícone cristal](arquivadas/decisoes/2026-11-16-icone-cristal-lumen.md), as [pesquisas](arquivadas/pesquisas/) já aproveitadas e as [specs e planos](arquivadas/superpowers/) das etapas concluídas.
