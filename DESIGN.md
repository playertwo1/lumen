---
name: Lumen Desktop (Segunda Mente Companion)
description: Sistema visual editorial escuro para o Lumen no Windows, alinhado à Segunda Mente no Android (#000000, #F3EEE6, #E99A3D, Inter) e à qualidade das telas originais do Hub.
colors:
  hub-void: "#000000"
  hub-panel: "#0e0f11"
  hub-surface: "#141518"
  hub-card: "#18191c"
  hub-field: "#1f2024"
  hub-line: "rgba(243, 238, 230, 0.08)"
  hub-line-strong: "rgba(243, 238, 230, 0.14)"
  hub-ink: "#F3EEE6"
  hub-body: "#DED9D1"
  hub-secondary: "#AAA6A0"
  hub-muted: "#706D68"
  hub-accent: "#E99A3D"
  hub-accent-soft: "rgba(233, 154, 61, 0.14)"
  hub-on-accent: "#160D02"
  hub-green: "#34D399"
  hub-red: "#FF5A4E"
  island-card: "#141518"
typography:
  display:
    fontFamily: "Inter, Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "28px"
    fontWeight: 700
    lineHeight: 1.12
    letterSpacing: "-0.02em"
  headline:
    fontFamily: "Inter, Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 700
    lineHeight: 1.22
    letterSpacing: "-0.015em"
  title:
    fontFamily: "Inter, Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: "Inter, Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Inter, Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    letterSpacing: "0.08em"
  mono:
    fontFamily: "Cascadia Mono, Consolas, ui-monospace, monospace"
    fontSize: "12px"
    fontWeight: 400
rounded:
  sm: "8px"
  md: "10px"
  lg: "14px"
  xl: "18px"
  panel: "22px"
  pill: "999px"
spacing:
  xs: "6px"
  sm: "8px"
  md: "12px"
  lg: "18px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.hub-accent}"
    textColor: "{colors.hub-on-accent}"
    rounded: "{rounded.md}"
    padding: "8px 14px"
  button-secondary:
    backgroundColor: "rgba(243, 238, 230, 0.08)"
    textColor: "{colors.hub-ink}"
    rounded: "{rounded.md}"
    padding: "8px 14px"
  card-surface:
    backgroundColor: "{colors.hub-card}"
    rounded: "{rounded.lg}"
    padding: "18px"
  input-field:
    backgroundColor: "{colors.hub-field}"
    textColor: "{colors.hub-ink}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
  inspector-drawer:
    backgroundColor: "{colors.hub-surface}"
    rounded: "{rounded.lg}"
    padding: "18px"
---

# Design System: Lumen Desktop (Segunda Mente Companion)

## Overview

**Creative North Star: "O Diário de Mesa — Calmo, Editorial e Preciso"**

Lumen é o irmão desktop da **Segunda Mente**: um instrumento silencioso de trabalho diário em escuro profundo, onde a leitura respira e a interface nunca disputa atenção com o pensamento do usuário. A direção visual preserva a base sólida que já funciona bem nas telas originais do Hub (barra lateral em preto absoluto, painel escuro acolhedor, heróis com o elenco do Lumen e listas limpas estilo Things/Linear) e eleva tudo o que foi adicionado no fork — a começar por **Tarefas**, e depois as adições de **Anotador/Personas**, **MailCraft**, **Celular Android** e cartões de tarefa no **Chat** — para o mesmo padrão editorial.

O desktop adapta a calma editorial do mobile para uso real com mouse, teclado e janelas largas no Windows: menos caixas dentro de caixas, contraste quente em marfim sobre fundos escuros, e um único ponto de foco quando uma proposta de IA ou detalhe de item exige decisão humana.

**Key Characteristics:**
- Preto puro na moldura lateral e painéis escuros neutros de baixo ruído, sem sombras pesadas nem gradientes decorativos.
- Texto em marfim quente (`#F3EEE6`) e secundários quentes (`#AAA6A0`) para conforto prolongado de leitura no Windows.
- Cobre/âmbar (`#E99A3D`) reservado exclusivamente para seleção ativa, confirmação e ação principal da tela.
- Arquitetura **Master-Detail** em telas operacionais para ocupar janelas largas com propósito (lista respirando no centro + inspetor/revisor à direita apenas quando necessário).
- Zero poluição repetitiva por item: metadados óbvios ficam no cabeçalho ou grupo, jamais repetidos em três pílulas coloridas por linha.

## Colors

Uma paleta escura, quente e silenciosa que traz a atmosfera da Segunda Mente para o desktop, substituindo a mistura antiga de roxo, azul e laranja saturado por papéis rígidos de cor.

### Primary
- **Âmbar Cobre (Primary Accent)**: cor única de ação principal (`Propor`, `Confirmar`, `Salvar`), item/aba selecionada e anel de foco no Hub (`hub-accent`), alinhada diretamente ao acento da Segunda Mente no Android.

### Secondary
- **Verde Sincronia (Positive State)**: reservado estritamente ao indicador de conectividade/sincronia E2EE na barra de estado ou feedbacks conclusivos discretos (`hub-green`), sem aparecer em cada linha comum de tarefa.
- **Coral Alerta (Danger / Overdue)**: reservado para tarefas atrasadas, erros reais e ações destrutivas (`Excluir`), com superfície sutil no repouso (`hub-red`).

### Tertiary
- **Âmbar Suave (Surface Wash / Focus Tint)**: lavagem translúcida (`hub-accent-soft`) usada em seleções ativas de linha, banners de ambiguidade ("IA propõe, usuário confirma") e destaque sutil do item aberto no inspetor.

### Neutral
- **Preto Absoluto (`hub-void`)**: fundo da janela principal, sidebar lateral e base da Ilha flutuante.
- **Ardósia Escura (`hub-panel`)**: superfície do painel principal arredondado à direita da sidebar.
- **Obsidiana Elevada (`hub-surface` / `hub-card`)**: superfícies de listas agrupadas e do painel inspetor lateral.
- **Grafite de Campo (`hub-field`)**: fundo exclusivo de campos editáveis, inputs e seletores customizados.
- **Marfim Quente (`hub-ink`) / Marfim Suave (`hub-body`)**: hierarquia principal de leitura sem branco clínico estourado.
- **Cinza Quente (`hub-secondary` / `hub-muted`)**: rótulos auxiliares, horários, metadados e placeholders.
- **Divisor Sutil (`hub-line` / `hub-line-strong`)**: separadores de `1px` entre linhas e contorno discreto de superfícies.

### Named Rules
**The One Voice Rule.** Apenas o cobre/âmbar (`hub-accent`) funciona como cor de foco e ação principal no Hub; resquícios de roxo ou azul em busca, foco de campo ou botões neutros estão banidos.
**The Quiet Row Rule.** Nenhuma linha de lista carrega selos para estados padrão (`Normal`, `Sem data`, `Sincronizado`); badges em linha só aparecem quando algo exige atenção (atraso, prioridade elevada, conflito ou pendência local real).

## Typography

**Display Font:** Inter, com fallback nativo do Windows (`Segoe UI Variable Text`, `Segoe UI`, `system-ui`).
**Body Font:** Inter, com fallback nativo do Windows (`Segoe UI Variable Text`, `Segoe UI`, `system-ui`).
**Label/Mono Font:** Cascadia Mono (fallback `Consolas`, `ui-monospace`) para códigos de pareamento, atalhos e trechos técnicos.

**Character:** Editorial limpa, neutra e legível em monitores de alta ou média densidade no Windows, evitando caixa alta excessiva e pesos pesados sem necessidade.

### Hierarchy
- **Display** (`700`, `28px`, `line-height: 1.12`, `-0.02em`): reservado para heróis de seção ou cabeçalho principal quando a página possui faixa introdutória com o elenco do Lumen.
- **Headline** (`700`, `20px`, `line-height: 1.22`, `-0.015em`): títulos das páginas operacionais (`Tarefas`, `Anotador`, `Mail`, detalhe aberto no inspetor).
- **Title** (`600`, `14px`, `line-height: 1.4`): título de tarefa, cartão de pessoa/nota e cabeçalhos de painel inspetor.
- **Body** (`400`, `13.5px`, `line-height: 1.5`): leitura de notas, descrições, mensagens e formulários; limitar colunas de texto corrido a `65–75ch`.
- **Label** (`600`, `11px`, `+0.08em`): marcadores de grupo de navegação e cabeçalhos de seção (`HOJE`, `PRÓXIMAS`, `FLOW`, `NOTCH`), sem exagero de `letter-spacing`.

### Named Rules
**The Tabular Time Rule.** Horários, contagens, prazos e alertas usam `font-variant-numeric: tabular-nums` para alinhamento vertical impecável em listas e tabelas.

## Layout

O Hub mantém a moldura de duas áreas que funciona bem nas telas originais — barra lateral fixa (`228px`, recolhível ou compactada em janelas estreitas `<= 960px`) em preto absoluto e painel arredondado (`panel`) à direita —, mas elimina o buraco vazio das telas largas substituindo a coluna única estreita por grids adaptativos:

- **Telas operacionais (Tarefas, Anotador, Mail):** estrutura **Master-Detail responsiva**. O espaço principal ocupa a largura disponível com conforto (`max-width: 1280px` ou grid fluido `minmax(0, 1fr)` + painel inspetor de `340px–380px` à direita).
- **Lista limpa sem inspetor ocioso:** quando nenhuma proposta de IA está em revisão nem nenhum item está selecionado para edição, a coluna central ocupa o espaço com respiro equilibrado (sem metade direita vazia em branco/preto morto), ou mantém resumo/atalho discreto de apoio. Ao acionar `Propor` ou selecionar um item para editar, o **painel inspetor à direita** abre de forma dedicada, com controles dimensionados para desktop.
- **Ilha vs. Hub:** a Ilha no topo da tela continua sendo uma faixa compacta de captura instantânea (`Ctrl+Alt+K`), lembrete acionável (`taskReminder`) e estado; classes e regras CSS de layout apertado da Ilha nunca são embutidas dentro das páginas do Hub.

### Named Rules
**The Surface Boundary Rule.** A Ilha (`<= 640px`) e o Hub (janela larga desktop) compartilham tokens de cor e voz, nunca o mesmo HTML/CSS de formulário comprimido.

## Elevation & Depth

Profundidade **tonal e plana por padrão**, sem sombras pesadas nem brilho decorativo atrás de caixas comuns (`hub-void` → `hub-panel` → `hub-card` / `hub-surface` → `hub-field`). Camadas são separadas por mudança sutil de tom e contorno hairline (`1px solid`), reservando elevação por sombra apenas para overlays reais (popovers de seleção, menus contextuais e diálogos de confirmação).

### Shadow Vocabulary
- **Overlay Surface (`popover-shadow`)** (`box-shadow: 0 12px 32px rgba(0, 0, 0, 0.52)`): exclusivo para popovers flutuantes, date-pickers customizados e modais sobrepostos.

### Named Rules
**The Flat-By-Default Rule.** Cartões e painéis em repouso nunca projetam sombra; contraste de superfície + borda sutil (`1px`) bastam para separar regiões.

## Shapes

Vocabulário enxuto e previsível de raios para evitar a colcha de retalhos anterior:
- **Painel Principal:** raio amplo (`22px`) herdado da identidade original do Hub.
- **Agrupadores, Hero e Painel Inspetor:** raio médio-grande (`14px`–`18px`).
- **Botões, Campos, Linhas selecionadas e Controles de Desktop:** raio uniforme (`10px`), nunca misturando `7px`, `8px` e `9px` lado a lado.
- **Pílulas e Avatares (`999px`):** restritos a filtros segmentados curtos, badges de status realmente excepcionais e contadores.

## Components

### Buttons
- **Shape:** cantos consistentes (`10px`), altura confortável (`34px–36px`), transição sutil (`120ms ease`) de fundo sem saltos bruscos.
- **Primary:** fundo âmbar cobre (`hub-accent`), texto escuro quente (`hub-on-accent`), `font-weight: 600`; usado no máximo uma vez por região de ação (`Propor`, `Confirmar tarefa`, `Salvar`).
- **Secondary / Ghost:** fundo translúcido marfim (`rgba(243, 238, 230, 0.08)`) em ações secundárias (`Sincronizar agora`, `Editar`); ghost sem borda para ações discretas de linha que intensificam no `:hover` ou foco.
- **Danger:** texto coral suave (`hub-red`) sobre fundo sutil (`rgba(255, 90, 78, 0.12)`), intensificando no `:hover`.

### Chips / Filter Tabs
- **Style:** usados apenas para alternância de visão/filtro (`Todas`, `Hoje`, `Próximas`, `Atrasadas`, `Sem data`, `Concluídas`) ou escolha rápida dentro do inspetor desktop (`Hoje`, `Amanhã`, `9h`, `14h`, `20h`).
- **State:** ativo em `hub-accent-soft` com borda âmbar sutil e texto `hub-ink`; nunca branco puro sólido nem verde chamativo para chip de formulário.

### Cards / Containers & Task List
- **Corner Style:** container agrupado único (`14px`) por bucket (`HOJE`, `PRÓXIMAS`, `SEM DATA`), mantendo linhas internas separadas apenas por divisor fino (`1px solid hub-line`) em vez de um card pesado por tarefa.
- **Internal Padding:** `14px 18px` por linha de item, com ações secundárias discretas à direita (`opacity: 0.65` em repouso, `1` no hover/foco) e badge de status apenas quando necessário.

### Inputs / Fields
- **Style:** fundo `hub-field`, borda `1px solid hub-line-strong`, raio `10px`, `color-scheme: dark`, texto `hub-ink`.
- **Focus:** transição limpa de borda para `hub-accent` e anel sutil âmbar (`0 0 0 2px rgba(233, 154, 61, 0.22)`), substituindo o foco roxo antigo.

### Inspector Drawer (Master-Detail Desktop)
- **Role:** painel lateral direito (`340px–380px`) que aparece quando há proposta gerada pela IA ("Confirmar proposta antes de salvar") ou quando o usuário seleciona um item existente para editar.
- **Composition:** cabeçalho claro + alerta de ambiguidade (quando aplicável) + título editável + grade limpa de 2 colunas para Data/Hora e Prioridade/Aviso/Repetição com controles proporcionais de desktop + rodapé fixo com `Descartar` e `Confirmar`.

### Navigation
- **Sidebar (`228px`):** mantém a organização que já funciona bem (`FLOW`, `NOTCH`, `MAIL`) em `hub-void`, busca rápida no topo com foco âmbar, item ativo em superfície suave marfim e rodapé limpo sem sobreposição de texto da versão/status.

## Do's and Don'ts

### Do:
- **Do** preservar a estrutura base de navegação, os heróis com personagens do Lumen e o nível de acabamento das páginas originais (`Ditado`, `Estatísticas`, `Flow`) ao reformular as telas adicionadas no fork.
- **Do** exibir propostas da IA ("IA propõe, usuário confirma") e edição de tarefas no Hub dentro de um **painel inspetor/revisor desktop** estruturado em grade alinhada, nunca no componente compacto da Ilha.
- **Do** garantir que a janela maximizada distribua o espaço com equilíbrio (Master-Detail ou grade de colunas), sem deixar metade da tela vazia à direita.
- **Do** manter acessibilidade plena (`prefers-reduced-motion`, foco de teclado nítido em âmbar, contraste forte de `#F3EEE6` em fundos escuros e `color-scheme: dark`).

### Don't:
- **Don't** repetir selos redundantes (`Sem data`, `Normal`, `Sincronizado`) em toda linha quando a seção ou a barra superior já comunicam esse estado.
- **Don't** empilhar 5+ chips arredondados e `<select>` nativos comprimidos lado a lado dentro de um único card largo, nem usar botão branco sólido em formulários do Hub.
- **Don't** misturar focos roxos (`#6e56cf`), banners azuis (`#3b82f6`) e acentos laranja na mesma visão operacional.
- **Don't** quebrar o layout da Ilha (`taskReminder` / `taskCapture` / `taskConfirm` em `640px`) ao estilizar o Hub: mantenha CSS da Ilha e do Hub estritamente isolados.
