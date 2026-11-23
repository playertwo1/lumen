# Personagens do Lumen

Os personagens são desenhados em código (Canvas), em `windows/src/lumen/compass-engine.ts`. As imagens desta pasta foram geradas a partir desse código em 2026-11-18 e mostram o que o app desenha hoje. Quando um personagem mudar no código, gere as imagens de novo (ver "Como gerar" abaixo).

## Família

Todos compartilham contorno índigo `#26224B`, dois olhos ovais grandes com pupilas índigo, pontos de luz areia `#F4E8C9` e boca feita com uma curva curta. Nenhum usa logotipo, símbolo ou mascote oficial.

| Personagem | Pílula (ID) | Forma | Cor principal |
|---|---|---|---|
| Claude Code | `integration_claude` | Folha com canto dobrado e marcador | branco `#F5F6F8` |
| Claude Desktop | `agent_claude-desktop` | A mesma folha do Claude Code | terracota `#D97757` |
| Codex | `agent_codex` | Quadrado arredondado com uma aba lateral | turquesa `#2DD4BF` |
| ChatGPT | `ai_openai` | Balão de conversa redondo | verde `#10A37F` |
| Antigravity | `agent_antigravity` | Arco em "A" com degradê colorido e duas bolinhas flutuando | degradê (base magenta `#E879F9`) |
| Gemini CLI | `agent_gemini` | Dois volumes unidos com um rosto só | azul claro `#8AB4F8` |
| OpenCode | `agent_opencode` | Retângulo alto com arco no topo e boca larga | verde `#4ADE80` |
| Grok | `grok` | Cápsula chanfrada com visor e satélite em órbita | prata `#B8C5D6` |
| Hermes | `agent_hermes-desktop` | Corpo arredondado com asinhas e crista | roxo `#C084FC` |
| YouTube Music | `integration_youtubemusic` | Corpo com fones de ouvido | vermelho `#FF0033` |
| Email | `integration_mail` | Envelope | azul `#2F8CFF` |
| WhatsApp | `integration_whatsapp` | Dois balões de conversa escalonados | verde `#25D366` |
| Genérico | qualquer outro | Folha A4 com linhas de texto | areia `#F4E8C9` |

## Estados

Cada personagem tem 11 estados: Parado, Trabalhando (com monitor e teclado), Pensando, Procurando (lupa), Aguardando aprovação, Pergunta, Erro, Concluído, Limite temporário, Dormindo e Tonto. A silhueta e a cor do personagem não mudam entre estados; mudam os olhos, a boca e o destaque ao lado.

## Arquivos

- `lineup.png` e `lineup-transparente.png`: os 12 personagens parados, 96 px cada, com nome.
- `<personagem>-estados.png` e `<personagem>-estados-transparente.png`: os 11 estados de cada um (`claude-code`, `claude-desktop`, `codex`, `chatgpt`, `antigravity`, `gemini-cli`, `opencode`, `grok`, `hermes`, `youtube-music`, `email`, `generico`).
- `teste-22px.png`: os 12 em 22 px reais, o tamanho da pílula.
- `aprovadas-2026-11-13/`: as pranchas desenhadas à mão que o dono aprovou em 2026-11-13 (7 personagens, 8 estados). Ficam como registro do aceite e servem de comparação na página de conferência; não são mais o visual do app.
- `claude-marca-mascote-estudo*.png`: estudo isolado do Claude, nunca aprovado para o app.
- `prototipo/`: protótipo sprite × Canvas da L1.4b e a página de conferência `l14d1-conferencia.html`, com todos os estados e animações.

## Como gerar

Em `windows/`, rode `npm run dev` e abra `/design/personagens/prototipo/l14d1-conferencia.html`. A página importa o `BotEngine` do código do app; cada imagem desta pasta é o `BotEngine` desenhado num `<canvas>` (DPR 2, estado fixo, sem animação) e salvo como PNG.
