# Lumen Windows: retomar alertas de cota após um aviso comum

**Base verificada:** `windows-v0.7.28` e `5d3ce5b4e2c63b243f40ee5dfbd68c12e3beb214` (`main`). O código da ilha é idêntico ao da 0.7.27. Este caso não é a correção da leitura de cotas via PATH da 0.7.28.

## Problema e evidência

Em [island.ts](../windows/src/island/island.ts#L864), quando um segundo alerta de cota aguarda na fila e aparece um aviso comum como “Nota salva”, `pushCompactNotice` limpa o marcador ativo e substitui o temporizador. Quando o aviso comum expira, o próximo alerta só seria retirado se ainda houvesse um marcador de cota ativo. Ele fica preso mesmo com a ilha livre. O cenário foi reproduzido com temporizadores simulados na lógica idêntica; não foi testado no aplicativo instalado nem no caminho de notificações nativas.

## Correção solicitada

Ao liberar a área de aviso, retomar a entrega dos alertas pendentes conforme a política da fila. Evitar duplicar ou descartar avisos pendentes por acidente, preservar pausa e deduplicação por provedor/janela, e não mudar o despacho independente de notificações nativas.

## Critérios de aceite

- Com temporizadores simulados, apresentar dois alertas de cota sintéticos, interromper o primeiro com “Nota salva” e deixar esse aviso expirar. O segundo aparece uma única vez e a fila progride.
- Cobrir sequência sem interrupção, alertas da mesma combinação provedor/janela e pausa, sem regressão na ordem esperada. Um aviso comum não deve perder sua duração por causa de um temporizador antigo.
- Rodar os testes Windows pertinentes com eventos sintéticos. Se houver Windows disponível, confirmar no app com alertas de demonstração; não consumir cotas reais para simular o fluxo.
