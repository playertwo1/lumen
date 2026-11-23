# M2b — Comando de voz por atalho (spec)

**Pedido do dono (2026-11-23):** na seção "Botões de comando", um atalho de tecla deve iniciar um comando falado. Exemplos: "brilho a 20%" ajusta o brilho para 20%; "tirar som" silencia o áudio.

## Diagnóstico

"Botões de comando" (Ditado → Ajustes) só escolhe e ordena os botões da **pílula**: eles aparecem ao passar o mouse na pílula e clicar no raio. Não há tecla nem voz ligada a eles, e os botões têm níveis fixos (brilho 30% e 100%), então "brilho a 20%" não tinha caminho. Os botões ficam como estão; o comando de voz entra **ao lado**, sem substituir nada.

## Comportamento

- Atalho global novo `voiceCommand`, padrão **Ctrl+Alt+V** ("voz"), ligado por padrão. Troca-se em **Notch → Atalhos**, como os outros (gravar outra tecla, desligar). V não é caractere de AltGr nos layouts verificados.
- Apertar uma vez: a pílula mostra "ouvindo"; o Lumen grava até a pessoa parar de falar (1,2 s de silêncio) ou até 8 s. Se o tempo acabar no meio de uma fala, vale o que foi gravado.
- O texto vem do modelo de ditado local (Parakeet ou Whisper). Nada sai do computador.
- O texto é interpretado **localmente**, sem IA, por regras simples em pt-BR:
  - "brilho" (também "birlho", "brilo") + número em dígitos ("20", "20%") ou por extenso ("vinte por cento", "trinta e cinco") → ferramenta `brilho` com `nivel` (0–100).
  - "tirar/tira o som", "silenciar", "mudo", "sem som" → ferramenta `audio` com `silenciar`.
  - "voltar/volta o som", "ligar o som", "reativar o som" → `audio` com `reativar_som`.
- A execução passa por `MochiRuntime::invoke_tool`, como os botões e o chat: pausa rápida, schema, permissões, cartão de confirmação e registro local. As duas ferramentas são de risco baixo.
- O resultado (ou "não entendi: <frase>") aparece no balão de voz da pílula por alguns segundos.
- Uma segunda pressão enquanto escuta é ignorada.

## Fora do escopo

Volume em número, outros comandos por voz, comando pelo "OK Lumen" (M4), tecla segurada (push-to-talk).

## Aceite

Apertar Ctrl+Alt+V, falar "brilho a 20%" e ver o brilho em 20%; falar "tirar som" e ver o som mudo.
