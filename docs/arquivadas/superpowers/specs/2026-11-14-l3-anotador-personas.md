# L3 — Anotador de pensamentos e ideias e comentários de personas

## Objetivo

Converter o antigo Notetaker (gravador de reuniões) no **Anotador** do Lumen, permitindo transformar pensamentos e ideias em notas por **ditado de voz** (microfone apenas) ou **captura rápida em texto**, e gerar **comentários de personas** seguindo as regras do app celular Segunda Mente a partir de personas importadas manualmente até a chegada do sync (L6).

## Contrato

### L3.1 — Anotador (voz e texto rápido)

- **Escopo de captura:** `notes_start` passa a gravar somente a voz do dono (microfone local, igual a `notes_idea` / `Ctrl+Alt+I`), sem capturar áudio do sistema nem rotular entradas novas como gravação de chamadas/reuniões. Notas já salvas em `%LOCALAPPDATA%/.../Notes/<id>/note.json` continuam legíveis sem perda de campos (`speakers`, `lines`, `summary`, `keyPoints`, `todos`).
- **Captura rápida em texto:** novo comando Tauri `notes_create_text(title: Option<String>, text: String) -> Result<Note, String>` cria imediatamente uma nota `Ready` com a linha do dono (`speaker: "me"`), emite `notes-changed` e `note-created`, e dispara resumo + geração automática de comentários quando configurados.
- **Títulos e textos pt-BR:** notas de voz/texto recebem título inicial `"Ideia · MM-DD HH:MM"` (ou título informado pelo usuário) e interface totalmente em pt-BR (página **Anotador**, busca, edição de título, exportação Markdown incluindo comentários, confirmação de exclusão).
- **Provedor ativo do Lumen:** resumo automático e comentários de personas usam helper único (`chat::complete_oneshot`) que respeita o provedor configurado em Configurações — incluindo **Plano ChatGPT (L2)** via OAuth SIWC (`store: false`, sem gravação no histórico do chat), Claude (chave/CLI), OpenAI-compatível ou servidor local — sem exigir chave paga da Anthropic quando ChatGPT OAuth estiver ativo.

### L3.2 — Comentários das personas com regras do celular

- **Formato da persona importada:** armazenamento local em `Personas/personas.json` e `Personas/comment_settings.json` dentro do diretório local do app (nunca no repositório público nem em texto de exemplo com dados pessoais).
  - Suporte ao formato em **três partes separadas** (referência QwenPaw): `identity` (identidade e estilo), `userProfile` (perfil do usuário) e `rules` (regras/política de comentários).
  - Compatibilidade direta com exportações JSON do Segunda Mente (`id`, `name`, `tags`, `persona`, `enabled`, `isPrimaryCompanion` / `is_primary_companion`, `interestFilter` / `interest_filter`, `systemPromptOverride` / `system_prompt_override`, `postHistoryInstructions` / `post_history_instructions`, `mesExample` / `mes_example`), aceitando objeto único, lista de objetos ou `{ "characters": [...] }`.
- **Configurações de comentário (paridade com `CommentSettings`):**
  - `enableCharacterComment` (`bool`, padrão `true`).
  - `maxCommentCharacters` (`1..=5`, padrão `2`).
- **Seleção e roteamento:**
  - `@menção` (`@id` ou `@nome` autônomo) no texto da nota ou em resposta roteia diretamente para a persona mencionada com resposta obrigatória (`forceReply`).
  - Resposta do dono a um comentário (`replyToId`) roteia de volta à persona autora daquela thread.
  - Com `maxCommentCharacters <= 1`, usa seleção determinística por afinidade (`tags` +3, `interestFilter` / `Focus on` ignorando o bloco após `Ignore` / `não foque em` +2, sinais emocionais de saúde/desabafo/reflexão/emoção +4, desempate por `isPrimaryCompanion`), sem chamada extra ao modelo.
  - Com `maxCommentCharacters > 1`, usa uma chamada curta de roteamento ao modelo para escolher até `maxCommentCharacters` personas distintas em ordem de afinidade (com fallback determinístico para melhor afinidade/companheiro principal), e executa sequencialmente passando `<existing_comments>` para a próxima evitar repetição.
- **Regras de fala das personas:**
  - Prompt em pt-BR fiel ao `CommentAgentSkill` do celular: responder na voz da persona (nunca como aplicativo/assistente/terapeuta), comprimento proporcional ao conteúdo sem repetir/resumir a nota, evitar chavões mecânicos e conselhos não solicitados, no máximo uma pergunta natural, nunca inventar fatos ou datas/horários de tarefa ambíguos, e responder `[SKIP]` se a entrada for trivial/fora da política (exceto quando `forceReply` estiver ativo).
- **Garantia contra duplicação (Checkpoint L3):**
  - `Note` persiste `comments: Vec<NoteComment>` e `commentHash: Option<String>` (hash do conteúdo consolidado já comentado automaticamente).
  - Trava em memória por `note_id` (`commenting`) impede execuções paralelas sobre a mesma nota.
  - Ao concluir uma nota ditada/texto, se `commentHash` já corresponder ao conteúdo atual ou a persona já possuir comentário automático na nota, nenhuma segunda inserção é feita (inclusive após reinício ou clique repetido).
  - Comentário manual do dono (`notes_add_comment`) pode gerar resposta direcionada na thread (`replyToId`), limitado por `maxPersonaTurnsPerUserMessage = 4` por turno do dono.

### Animação P3 (L3)

- `CompassEngine::triggerNoteWrite()` em `compass-engine.ts`: gesto temporizado (`~1,45 s`) desenhando bloquinho e caneta escrevendo ao lado do personagem; volta a `busy = false` ao concluir e é suprimido sob `prefers-reduced-motion`. Dispara na ilha ao receber o evento `"note-created"`.

## Fora de escopo

- Sync automático de personas/notas com o Android (L6) e modelo persistente de Tarefas (L4).
- Dados pessoais, personas privadas reais ou caminhos locais em código, documentação ou fixtures.

## Aceite sintético

- Criar nota por voz (com áudio simulado / `NoteBuilder`) ou via `notes_create_text` gera nota pronta sem gravar áudio de sistema e dispara `"note-created"`.
- Importar JSON de personas sintéticas em formato 3 partes (`identity`, `userProfile`, `rules`) e formato Segunda Mente (`persona`) persiste corretamente e mantém campos completos.
- Seleção determinística (`maxCommentCharacters = 1`), roteamento por `@menção`, roteamento multi-persona (`maxCommentCharacters >= 2`) e construção de prompt com `<existing_comments>` passam em testes unitários Rust com servidor HTTP mock.
- Antiduplicação garante que duas execuções automáticas consecutivas sobre a mesma nota ditada não criam comentários duplicados nem na mesma persona nem em duplicidade de rodada.
- P3 (`triggerNoteWrite`) anima por tempo limitado em `CompassEngine`, respeita `prefers-reduced-motion` e retorna a `busy = false`.

## Aceite real

- Ditar uma ideia rápida (`Ctrl+Alt+I` ou botão do Anotador) e criar uma nota por texto rápido no Lumen usando a conta ChatGPT OAuth (ou provedor ativo), com ao menos uma persona importada manualmente: confirmar que a nota salva, o personagem anima escrevendo a nota, uma persona comenta de acordo com identidade/regras e **não duplica** comentários em reabertura/reinício.
