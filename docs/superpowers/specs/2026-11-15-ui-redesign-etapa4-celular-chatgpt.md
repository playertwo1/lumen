# Spec — Etapa 4: Celular Android, Relay e Login ChatGPT (Lumen Hub)

- **Escopo:** `windows/src/settings/phone.ts`, `chatGptAccountSection` e harmonia visual das seções em `windows/src/settings/main.ts`, além dos estilos `.phone-*`, `.chatgpt-*` e `#settings-root` no Hub em `windows/src/hub/hub.css`.
- **Objetivo:** alinhar **Celular Android / Relay** e **Login ChatGPT** ao sistema visual **Lumen Desktop** (`#000000` / `#0e0f11` / `#18191c`, marfim `#F3EEE6`, âmbar `#E99A3D`, raio `10px`, contraste `>= 4.5:1`, largura de leitura `<= 72ch`) sem alterar contratos IPC, fluxos OAuth/E2EE nem o CSS da Ilha.

## Decisões de interface

1. **Celular Android & Relay (`phone.ts`):**
   - Cabeçalho único de ativação (`Link com o celular`) com badge de estado vivo e switch alinhado.
   - Bancada em duas colunas (`.phone-workbench`, empilhando `<= 1080px`):
     - **Esquerda:** Pareamento (QR em moldura clara `168px`, link em `Cascadia Mono`, `Copiar link`, aviso persistente `copyLinkHint` após cópia e `Parear de novo`) + Relay E2EE ("Fora do Wi-Fi de casa" com estado em pílula semântica e formulário alinhado de endereço + chave write-only).
     - **Direita:** Permissões granulares com divisores hairline (respostas Claude Code, detalhes da sessão, arquivos alterados, uso de planos e resumo semanal) + grade 2 colunas de serviços (`servicesBlock`) e modelos de chat pelo celular (`chatBlock`).
2. **Login ChatGPT (`main.ts` -> `chatGptAccountSection` + harmonia `#settings-root`):**
   - Botão compacto âmbar `"Continuar com ChatGPT"` no cabeçalho (eliminando a barra branca de 100% de largura).
   - Linhas de conta com ponto semântico de status, rótulo/estado claro e ações proporcionais (`10px`, sem branco clínico).
   - No escopo `.hub-page #settings-root` (`hub.css`), harmonizar `.primary` (`--hub-accent`), botões (`10px`), contraste de `placeholder` (`>= 4.5:1`) e medida `.hint` (`max-width: 70ch`).
