# Plano — Etapa 4: Celular Android, Relay e Login ChatGPT

1. **Estruturar `windows/src/settings/phone.ts`:**
   - Organizar `phoneSection` com cartão superior de ativação (`Link com o celular` + badge de status) e grade `.phone-workbench` quando ativo.
   - Refatorar visualmente `pairingRows`, `relayBlock`, os blocos de permissão (`detailsBlock`, `answersBlock`, `diffsBlock`, `usageBlock`, `recapBlock`), `servicesBlock` e `chatBlock` mantendo intactas todas as assinaturas, ordem de funções e invariantes verificadas por `phone-relay.test.mjs` e `phone-pairing-hint.test.mjs`.
2. **Refatorar `chatGptAccountSection` em `windows/src/settings/main.ts`:**
   - Substituir estilos inline e botão branco gigante pelas classes `.chatgpt-*` alinhadas ao Hub Lumen, preservando estados `busy`, erros, avisos de sessão e troca de conta ativa.
3. **Estilizar `.phone-*`, `.chatgpt-*` e harmonia `.hub-page #settings-root` em `windows/src/hub/hub.css`:**
   - Definir tokens/botões/inputs consistentes (`10px`, `#E99A3D`, `#F3EEE6`), contraste de placeholders e medida legível (`<= 70ch`), garantindo responsividade em `<= 1080px` e `<= 720px`.
4. **Validar:**
   - Criar testes sintéticos em `windows/tests/phone-chatgpt-hub.test.mjs` cobrindo `phoneSection` (pareamento, relay sem retorno de chave, permissões) e seção ChatGPT.
   - Executar `npx tsc --noEmit`, `npm test`, `npm run build`, `impeccable.cmd detect --json` (estático e CDP) e inspecionar capturas em `1440×900` e `860×740`.
