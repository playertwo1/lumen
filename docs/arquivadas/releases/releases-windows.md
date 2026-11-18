> Registro das releases Windows do fork, da mais nova para a mais antiga. O procedimento está em [release-checklist.md](../../roadmap/release-checklist.md). Cada release nova entra no topo.

# Releases Windows do fork

### Windows 0.7.18 — ferramenta principal: Claude Desktop, Orca, Antigravity e Codex

- [x] Validações locais: `python scripts/test-whats-new.py windows` (ok), `npx tsc --noEmit`, `npm run build`, `cargo test --lib` (`941` passaram, `10` ignorados). Testes JS: `761/761` (ajustados ao novo padrão, Claude Desktop).
- [x] Workflow `44357540100` (tag `windows-v0.7.18`, commit `0ffff6c`) passou (`success`) e publicou [Windows 0.7.18](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.18).
- [x] Release [Windows 0.7.18](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.18) publicada (não é rascunho) com os dois instaladores.
  - Digests do GitHub, iguais aos dos arquivos baixados: setup `Lumen-Windows-0.7.18-setup.exe` `48758E17EFBB974E7DE5A74345A7455B170DC370DA2689FC96ED6553017D2E4E`; MSI `Lumen-Windows-0.7.18.msi` `2F388921E896056D9429321155EB79C1F81DC08087592B3AB1973CA76897D4CD`.
- [x] SHA-256 dos arquivos baixados igual ao do GitHub; Defender sem detecções (varredura personalizada).
- [ ] Validação real no app instalado: pendente (dono testa a lista Ferramenta principal e o Claude Desktop como padrão).

### Windows 0.7.17 — organizar as janelas para programar (Codex, Claude Code e Antigravity)

- [x] Validações locais: `python scripts/test-whats-new.py` (ok), `cargo check` (backend real do Windows), `cargo test --lib` (`941` passaram, `10` ignorados, com o `resource.lib` linkado ao exe de teste). Sem mudança em TypeScript, então `tsc` e `npm test` não rodaram. Listagem dos apps da Store e abertura pelo AUMID (Calculadora) confirmadas num programa à parte.
- [x] Workflow `44305316315` (tag `windows-v0.7.17`, commit `b68b372`) passou e publicou [Windows 0.7.17](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.17).
- [x] SHA-256 dos arquivos baixados igual ao do GitHub; Defender sem detecções.
  - Setup `Lumen-Windows-0.7.17-setup.exe`: `41F5CC120D53C39700DAE67B80584923AE9017D0178099544331E64CD7C90E34`.
  - MSI `Lumen-Windows-0.7.17.msi`: `CA49EF670AC5CCACBCA46ACBF2F222BB731C392F361BBD8D4886A87BC4F05DFE`.
- [ ] Validação real no app instalado: pendente (dono testa "organize as janelas para programar" e o desfazer).

### Windows 0.7.16 — correções da M2.7 (brilho, campos soltos, Bloco de Notas)

- [x] Validações locais: `python scripts/test-whats-new.py` (ok), `npx tsc --noEmit`, `npm test` (`761/761`), `cargo test --lib` (`937` passaram, `10` ignorados, com o `resource.lib` linkado ao exe de teste). Brilho: `WmiSetBrightness` pela classe confirmado num programa à parte, mantendo o nível.
- [x] Workflow `44278921986` (tag `windows-v0.7.16`, commit `d0d722f`) passou e publicou [Windows 0.7.16](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.16).
- [x] SHA-256 dos arquivos baixados igual ao do GitHub; Defender sem detecções.
  - Setup `Lumen-Windows-0.7.16-setup.exe`: `18C6183853C5BD86EAB17126F06937DCF0EA543A896C50E0577F75B1EAC897ED`.
  - MSI `Lumen-Windows-0.7.16.msi`: `DEC623E1A26D4CB17199D71BE1A8CD0B42B75B5B830807E176936B7A175AA236`.
- [ ] Validação real no app instalado: pendente (dono repete os 10 comandos da M2.7).

### Windows 0.7.15 — ping pelo chat do ChatGPT como função nativa

- [x] Validações locais: `python scripts/test-whats-new.py` (ok), `npx tsc --noEmit`, `npm test` (`761/761`), `cargo test --lib` (`937` passaram, `10` ignorados, com o `resource.lib` linkado ao exe de teste). Teste real do ChatGPT: `ping` chamado 3 de 3, ida e volta em ~9 s.
- [x] Workflow `44246862486` (tag `windows-v0.7.15`, commit `8538e3d`) passou e publicou [Windows 0.7.15](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.15).
- [x] SHA-256 dos arquivos baixados igual ao do GitHub; Defender sem detecções.
  - Setup `Lumen-Windows-0.7.15-setup.exe`: `823943A5108F95F4A423A114540B6AACD2CE4076A10A7B0E20EDDC464C472541`.
  - MSI `Lumen-Windows-0.7.15.msi`: `E0A55E1831E692187B0FE9AC02F15A0501878B104D8B73D494C3E6F3BF692F8A`.
- [ ] Validação real no app instalado: pendente (dono).

### Windows 0.7.14 — M2 (controle nativo do Windows), ping pelo chat e 6 pílulas

- [x] Pedido explícito do proprietário em 2026-11-18 ("Gera versão nova") depois da M2.1–M2.6 implementada.
- [x] Conteúdo: ferramentas `audio`, `brilho`, `apps`, `janelas`, `midia`, `bloquear_tela`, `suspender`, `reiniciar` e `desligar` (`ec9088c`–`99cf281`); instrução de ferramentas explícita para o ChatGPT e pausa rápida no MCP (`f54f5ef`, `ec9088c`); até 6 pílulas (`f96cc87`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.14: 3 items`), `npx tsc --noEmit`, `npm test` (`757/757`), `npm run build`, `cargo test --lib` (`936` passaram, `10` ignorados, com o `resource.lib` linkado ao exe de teste; rodado antes da troca do número de versão).
- [x] Preflight separado não rodado: o workflow da tag já testa o upgrade antes de publicar.
- [x] Workflow `44236804726` (tag `windows-v0.7.14`, commit `d7bdada`) passou e publicou [Windows 0.7.14](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.14) e `windows-latest`.
- [x] Defender examinou setup e MSI baixados: código 0, `found no threats`. SHA-256 iguais aos digests do GitHub:
  - Setup `Lumen-Windows-0.7.14-setup.exe`: `A27E8157DEB90A0C27D30B0DC094369B93970C8A950A0041CC4D4869EBFA8709`.
  - MSI `Lumen-Windows-0.7.14.msi`: `80C525292FF2EF702801D7300C7867CDC8B8EA2A6C25B44DE996E51864952941`.
- [ ] Proprietário validar: `ping` pelo chat (e se ainda está lento) e a M2.7 (10 comandos × 3, retomar da suspensão, app ausente, troca de áudio); **NOT_RUN**.

### Windows 0.7.13 — cristal na janela de abertura (a bússola vinha de `src/lumen/greeting.ts`)

- [x] Pedido explícito do proprietário em 2026-11-17 para corrigir de vez o ícone antigo ao iniciar e publicar (`playertwo1/lumen`).
- [x] Causa: `vite.config.ts` troca `src/mochi/greeting` por `src/lumen/greeting.ts` no Windows; esse arquivo ainda desenhava a bússola e a 0.7.11 corrigiu o arquivo do Coucou (não empacotado).
- [x] Validação visual no exe de release local (`lumen.exe` 0.7.13) comparada à instalação 0.7.11, com capturas da tela a cada 250 ms: instalada = 277–627 pixels da cor da bússola por frame; nova = 0, cristal animando e terminando estático.
- [x] Teste novo `tests/lumen-startup-crystal.test.mjs`; `npx tsc --noEmit` e `npm test` (`757/757`).
- [x] Workflow `44045097666` (tag `windows-v0.7.13`, commit `7a390db`) passou e publicou [Windows 0.7.13](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.13).
- [x] Defender examinou setup e MSI baixados: código 0, `found no threats`. SHA-256 iguais aos digests do GitHub:
  - Setup: `451871FEB1AA253B9D3E9EA419D371655C9751282FADE98929350E654559BED8`.
  - MSI: `1CF294F9ECF9E1D88B73D8418C7E9B4EBBF93E83154BAF133746249BAC6AFD21`.
- [ ] Proprietário validar na instalação atualizada; **NOT_RUN**.

### Windows 0.7.12 — L8 (ajustes do uso real) e Apagar/Spam da triagem

- [x] Pedido explícito do proprietário em 2026-11-17 ("Merge, push e release") depois da L8 implementada.
- [x] Conteúdo: L8.1 ditado segurando a tecla, L8.2 mãos livres, L8.3 Anotador → Tarefas, L8.4 motor da limpeza em Estilos, L8.5 Orca em Agentes e Abas ativas, L8.6 WhatsApp pela ponte do Orbit (só leitura) e Apagar/Spam na triagem do MailCraft (`6ca3609`).
- [x] Validações locais: `python scripts/test-whats-new.py windows`, `npx tsc --noEmit`, `npm test` (`755/755`), `npm run build`, `cargo test --lib` (`854` passaram, `10` ignorados; manifesto Common-Controls v6 embutido no exe de teste).
- [x] `scripts/test-lumen-upgrade.ps1` passou a ler a versão nova do `package.json` (o primeiro run `44036881245` falhou porque esperava `0.7.11`).
- [x] Workflow `44039200118` (tag `windows-v0.7.12`, commit `45147c9`) passou, com upgrade MSI/NSIS de `0.7.10` preservando dados sintéticos, e publicou [Windows 0.7.12](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.12) e `windows-latest`.
- [x] Microsoft Defender examinou os instaladores baixados da release: código 0 e `found no threats`. SHA-256 iguais aos digests do GitHub:
  - Setup `Lumen-Windows-0.7.12-setup.exe`: `F1FE648E0DA53B928DD35C85FE1193FE768FEE541A05F0B55B8F16763ABA911E`.
  - MSI `Lumen-Windows-0.7.12.msi`: `8EE6B10445EB0189D80074DC9285B126E452A61EF8CFB294712863FF595DBCAE`.
- [ ] `lumen.exe` e `lumen-hook.exe` de `windows-latest` continuam os da 0.7.9 (o workflow não os atualiza).
- [ ] Checkpoints reais L8.1–L8.6 e da triagem: **NOT_RUN**.

### Windows 0.7.11 — animação do cristal na saudação inicial

- [x] Pedido explícito do proprietário em 2026-11-16 para corrigir o ícone ainda antigo ao iniciar e publicar uma release versionada (`playertwo1/lumen`).
- [x] Saudação inicial da ilha no Lumen agora mostra `/lumen-crystal.gif` uma vez e termina no PNG; `prefers-reduced-motion` usa o PNG estático.
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.11: 1 item em 1 idioma`), `npx tsc --noEmit`, `npm test` (`750/750`), `npm run build`, `cargo test --lib` (`825` passaram, `10` ignorados; manifesto Common-Controls v6 embutido no exe de teste) e `npm run pack`.
- [x] Preflight Windows `44004540059` passou; upgrade sintético MSI/NSIS preservou os dados de usuário.
- [x] Microsoft Defender examinou os artefatos de preflight e os quatro arquivos de `windows-latest`; em todos, código 0 e `found no threats`.
- [x] SHA-256 dos assets finais (iguais aos digests do GitHub):
  - Setup `Lumen-Windows-0.7.11-setup.exe`: `8CB608BA8EB94F02EE66DE68AF0E4596490282A0FA7BCBB2D1C3AB4E84F0CC56`.
  - MSI `Lumen-Windows-0.7.11.msi`: `7226F72D870B83B65E909A74B4323F887CC151190C8DDB44C0E81EFA470CBEE1`.
  - App portátil no `windows-latest` (`lumen.exe`): `1C85CBFC71E11AF91A7FAFDC7088CEF69E2C52265F138691DDC1D20A23AA24E0`.
  - Hook no `windows-latest` (`lumen-hook.exe`): `EDBE691B65120072DFF141D32DB727137510B85C56ADBF92AE6EF08AF9BBA373`.
- [x] Merge enviado para `origin/main` em `1dd26de` (inclui `d083612`); tag `windows-v0.7.11`; workflow `44007478316` concluiu com sucesso, publicou [Windows 0.7.11](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.11) e atualizou `windows-latest`.
- [ ] Proprietário validar visualmente a saudação na instalação atualizada do Windows; continua **NOT_RUN**.

### Windows 0.7.10 — cristal no ícone de abertura e atualização do ícone no Windows Search

- [x] Pedido explícito do proprietário em 2026-11-16 para corrigir o ícone/abertura e publicar uma release versionada (`playertwo1/lumen`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.10: 1 item em 1 idioma`), `npx tsc --noEmit`, `npm test` (`750/750`), `npm run build`, `cargo test --lib` (`825` passaram, `10` ignorados; manifesto Common-Controls v6 embutido no exe de teste) e `npm run pack`.
- [x] Preflight Windows `43981119101` passou; upgrade MSI/NSIS de `0.7.9` para `0.7.10` preservou os dados sintéticos.
- [x] Microsoft Defender examinou os quatro artefatos finais publicados em `windows-latest`: código 0 e `found no threats`. Os hashes dos instaladores finais diferiram do preflight e foram examinados após o download.
- [x] SHA-256 dos artefatos finais:
  - Setup `Lumen-Windows-0.7.10-setup.exe`: `26EA6A62C20CBB4F131C8710759D864F80EAD399CA7F9B7B213AE1B9ABE6CD5E`.
  - MSI `Lumen-Windows-0.7.10.msi`: `CE7E2E83A6F21DDD60647618D27C8FFE4E1C32482DE79298684AB76E17421EE0`.
  - App portátil `lumen.exe`: `1C85CBFC71E11AF91A7FAFDC7088CEF69E2C52265F138691DDC1D20A23AA24E0`.
  - Hook `lumen-hook.exe`: `EDBE691B65120072DFF141D32DB727137510B85C56ADBF92AE6EF08AF9BBA373`.
- [x] `main` integrada e enviada (`ed68389`); tag `windows-v0.7.10` publicada; workflow `43984761236` concluiu com sucesso e atualizou [Windows 0.7.10](https://github.com/playertwo1/lumen/releases/tag/windows-v0.7.10) e `windows-latest`.
- [ ] Proprietário validar visualmente após instalar/atualizar no Windows, incluindo pesquisa do Menu Iniciar e abertura do Hub. Continua **NOT_RUN**.

- [x] CHANGELOG contém seção desta versão e README tem linha na tabela de versões.
- [x] `windows/src/whats-new/whats-new.json` tem 1 item em 14 línguas; `python scripts/test-whats-new.py windows` passa.
- [x] A versão é idêntica em `windows/package.json`, `windows/package-lock.json`, `windows/Cargo.toml`, nas entradas `coucou` e `coucou-hook` de `windows/Cargo.lock` e em `windows/src-tauri/tauri.conf.json` (0.6.5).
- [x] Build/installer não assinado gerado e SHA-256 conferido; não comprar nem exigir certificado. Release `windows-v0.6.5`, setup `baf5092fdfaf1b7034a2a31a4d7184f782dd00b8dfba7aace870a09586ac05e9`, MSI `2f062e8c430e9a63f9882a3d17ce7d0b8eb86b240b7289e21cd2e6cd5c2c61d1`.
- [x] Antes da tag, executar manualmente `Windows` em `windows.yml` no branch de release; baixar os artefatos e examiná-los com Microsoft Defender. Preflight `43346487966`; Defender sem detecções.
- [x] Enviar a tag `windows-vX.Y.Z` ao fork. A tag `windows-v0.6.5` foi enviada; o evento não iniciou um run automaticamente.
- [x] Conferir a execução, a release e os assets em `playertwo1/coucou`; workflow manual no tag `43348587966` concluiu com sucesso e publicou [Windows 0.6.5](https://github.com/playertwo1/coucou/releases/tag/windows-v0.6.5).
- [x] Comparar os SHA-256 dos instaladores publicados com o preflight. Os hashes diferiram; os assets finais foram baixados, conferidos com os digests do GitHub e examinados diretamente pelo Defender sem detecções (evento 1001, exame `{9AD2DD74-E9E4-4DCE-8E77-D2954870AA9F}`).
- Nunca executar esta lista de publicação sem o proprietário pedir release.

### Windows 0.7.9 — Claude Code via `claude.cmd` corrigido na limpeza do ditado, no chat sem chave e nos resumos (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-16 para fazer commit e release (`playertwo1/lumen`, publicação local `no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.9: 2 items in 1 languages`), `cargo test --lib` (`825` passaram, `10` ignorados; manifesto Common-Controls v6 embutido no exe de teste com `mt.exe`), execução real ignorada `a_real_run_takes_a_multi_line_system_prompt` contra o `claude.cmd` do npm (respondeu `pong`) e `npm run pack`. Interface sem mudança desde a 0.7.8 (`750/750`).
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.9-setup.exe`: `74ECA2D8E66A39FD1DD5FB76C7FAE55196E915C45C2F830F0CF5B3A1E57C181C`.
  - MSI `Lumen-Windows-0.7.9.msi`: `9274098DD9D1982524D3E3EE7EC58542FACA14C44C799D3FCB3E1BB0748C9E5A`.
  - App portátil `lumen.exe`: `1C85CBFC71E11AF91A7FAFDC7088CEF69E2C52265F138691DDC1D20A23AA24E0`.
  - Hook `lumen-hook.exe`: `EDBE691B65120072DFF141D32DB727137510B85C56ADBF92AE6EF08AF9BBA373`.
- [x] Push para `origin` (`playertwo1/lumen`), tag `windows-v0.7.9`, publicação via `gh release create windows-v0.7.9` e atualização da release móvel `windows-latest`.
- [ ] Proprietário validar no notebook: ditar com a limpeza ligada e ver o texto revisado (sem `clean-up skipped` no log). Continua **NOT_RUN**.

### Windows 0.7.8 — Whisper Large v3 Turbo no ditado, texto ditado mantido na área de transferência, ícone cristal e Fase M1 do Mochi (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-16 para fazer commit e release (`playertwo1/lumen`, publicação local `no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.8: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`750/750`), `cargo test --lib` (`824` passaram, `9` ignorados; o exe de teste precisou do manifesto Common-Controls v6 embutido com `mt.exe`, falha de ambiente que também ocorre sem a mudança), `cargo test` em `windows/hook/` (`42/42`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.8-setup.exe`: `2F95354F3E53168CA2A17E49B375D9EF8A87856A18B0D6704C69172CB8EC47EE`.
  - MSI `Lumen-Windows-0.7.8.msi`: `69A05C6E7DEB867FDC8A253A71795A63BC0CB78DE63236B2867AAB89B08D1055`.
  - App portátil `lumen.exe`: `7221AD2B93486A69DD8709295D3D8562DD5302C9873DF324DA3537B5F1E7402F`.
  - Hook `lumen-hook.exe`: `28A727DA8A485EBE3ECF38200E4A28FC350F27ACD70B78471EEC220B8D8ACD4F`.
- [x] Push para `origin` (`playertwo1/lumen`), tag `windows-v0.7.8`, publicação via `gh release create windows-v0.7.8` e atualização da release móvel `windows-latest`.
- [ ] Proprietário validar no notebook: baixar o Whisper, ditar em português com Parakeet e com Whisper (precisão e velocidade), texto colado no campo e Ctrl+V colando de novo. Continua **NOT_RUN**.

### Windows 0.7.7 — Notch e pílula arrastáveis até a borda, pílula Em cima/Escondida e engrenagem direto nos Ajustes (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-16 para fazer commit e gerar uma release `.exe` versionada (`playertwo1/lumen`, publicação local `no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.7: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`727/727`), `cargo test --lib` (`755` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.7-setup.exe`: `9BD4880EB7915CCB3886D3618D82578062D3E1DA34F4AC2EC3C7E0D3E76CFB46`.
  - MSI `Lumen-Windows-0.7.7.msi`: `3218CF1B026F83E501695210CF4A9ED16821817B887BE2A66D28385350DEDCA2`.
  - App portátil `lumen.exe`: `318868DEEA24C405E319D878D1DFF5526BA5CEF3DD38CAB28C824610276855D1`.
  - Hook `lumen-hook.exe`: `41AD04B1C85E3EFE544DC7D371B1DDD1C35894B7A2692465360D5896D6F6FED0`.
- [x] Push para `origin` (`playertwo1/lumen`), tag `windows-v0.7.7`, publicação via `gh release create windows-v0.7.7` e atualização da release móvel `windows-latest`.
- [ ] Proprietário validar no notebook: arrastar o notch e a pílula até cada borda, Posição da pílula Em cima/Escondida e a engrenagem abrindo Ajustes → Geral. Continua **NOT_RUN**.

### Windows 0.7.6 — Triagem do MailCraft no lugar do cartão do Outlook (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-16 para fazer commit/merge na `main` e publicar uma nova versão `.exe` no GitHub (`playertwo1/lumen`, publicação local `no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.6: 2 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`727/727`), `cargo test --lib` (`752` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.6-setup.exe`: `D685FF8FB192EAB876F12C5CA2D8F12DA270CF7E021B67F1CEED7B0CA4208EAD`.
  - MSI `Lumen-Windows-0.7.6.msi`: `F1607230412A2C54585CCABE1E2CEEEF9A3A2A45055A4C84D94A517261E0E13E`.
  - App portátil `lumen.exe`: `81441EE741A7DEDEC40847F0D3F5AA108CDC769F2085387A94F4889150303AA5`.
  - Hook `lumen-hook.exe`: `B0B3420E993324041CBC516346B381E4C99099A1B017539941060C06B0798F5D`.
- [x] Push para `origin` (`playertwo1/lumen`), tag `windows-v0.7.6`, publicação via `gh release create windows-v0.7.6` e atualização da release móvel `windows-latest`.
- [ ] Proprietário validar no notebook a Triagem do MailCraft com a caixa real (classificação, recomendações e botões Importante/Marcar lida). Continua **NOT_RUN**.

### Windows 0.7.5 — Atalhos no Notch aberto, Limpar avisos do Mail, seta Abrir funcional e QR de pareamento ampliado (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-15 para consolidar as alterações na `main`, gerar o instalador `.exe` / release e publicar no GitHub (`playertwo1/lumen`, publicação local `no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.5: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`726/726`), `npm run build`, `cargo test --lib` (`748` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.5-setup.exe`: `2FAEC0AA83EC10CFBEA94D1DD9A90AFD3FED244723D8F48CDC023D8423933E3E`.
  - MSI `Lumen-Windows-0.7.5.msi`: `7E2C884B971CE9D3AA73490A2BEBF512C006F083967BA42A0BDFD767A2D0CE01`.
  - App portátil `lumen.exe`: `843B89BECA1145EC27C8FDDA0D42A5A68DAC40FE4A9858836DFB4568915286FC`.
  - Hook `lumen-hook.exe`: `99F0B7FBCB91125B8852F20C0580EE334FF211BFCC8BDA5ED2C4E48089C39037`.
- [x] Push para `origin` (`playertwo1/lumen`), tag `windows-v0.7.5`, publicação via `gh release create windows-v0.7.5` e atualização da release móvel `windows-latest` (`Lumen-Windows.msi` e `Lumen-Windows-setup.exe`).
- [ ] Proprietário validar no notebook: novos atalhos de Anotador e E-mail no Notch aberto, botão Limpar nos avisos do Mail, seta `↗` Abrir com MailCraft/Codex/Claude/Antigravity em foco e leitura do QR code ampliado de pareamento. Continua **NOT_RUN**.

### Windows 0.7.4 — Google no Lumen (L7), redesign editorial do Hub, Notch 2A e personas vivas (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-15 para consolidar as alterações na `main`, gerar os instaladores/executáveis versionados e publicar no GitHub (`playertwo1/lumen`, publicação local `no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.4: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`721/721`), `npm run build` (`tsc --noEmit && vite build`), `cargo test --lib` (`743` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.4-setup.exe`: `ED9D15AF5FFFCD868B151388FF7F19FD984895CF01351BD6ECABEC0B7732BA22`.
  - MSI `Lumen-Windows-0.7.4.msi`: `4209C39D5A80EF226710C8250BE010522401CC95715F551CECF530DA82F2E420`.
  - App portátil `lumen.exe`: `2DCFBF6E48A0605D60C180527E2D946471DC37314F5E25F9468A37E149BF2B98`.
  - Hook `lumen-hook.exe`: `C78837624E06153EF61E5EC7EE3CD2D425942EC22872E47C35A7050ACCE0F5FC`.
- [x] Publicação local via `gh release create windows-v0.7.4` em `playertwo1/lumen` e atualização da release móvel `windows-latest` (`Lumen-Windows.msi` e `Lumen-Windows-setup.exe`).
- [ ] Proprietário validar no uso real: login OAuth Google, backup/restauração no Drive (`Segunda Mente/Lumen-PC`), criação confirmada de eventos no Calendar (`L7`) e novo Notch 2A + Hub redesenhado no notebook. Continua **NOT_RUN**.

### Windows 0.7.3 — L6.2 pareamento via relay, L6.3 sync E2EE de tarefas, L6.4 cancelamento cruzado e animação P3 (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-15 para criar e publicar a release `0.7.3` localmente (`no-github-actions`) para validação da ponte com a Segunda Mente (`v1.0.48+133`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.3: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`699/699`), `npm run build` (`tsc --noEmit && vite build`), `cargo test --lib` (`727` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`), `npm test` em `android/relay/` (`26` worker + `37` node) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - Setup `Lumen-Windows-0.7.3-setup.exe`: `630AD859149763006F69A0A145B38FC25D71AEEFAFDEF3D5E44CB56E91B95DDE`.
  - MSI `Lumen-Windows-0.7.3.msi`: `A411712EFB7DE4976D819EA6902821EC2AAD7853ED4680A627689099C834E973`.
  - App portátil `lumen.exe`: `D7D50848180453E995599D8431A786875D3083D8A6AC10AB3EABD9495C039819`.
  - Hook `lumen-hook.exe`: `982A7356E5E3333D7C817E7CC3FE458B8D02C296306630AD77CE550D157B91EA`.
- [x] Publicação local via `gh release create windows-v0.7.3` em `playertwo1/lumen` e atualização da release móvel `windows-latest` (`Lumen-Windows.msi` e `Lumen-Windows-setup.exe`).
- [ ] Proprietário instalar a `0.7.3` no notebook e validar ponta a ponta o pareamento e sync de tarefas via relay com o Galaxy S25 Ultra (`v1.0.48+133`). Continua **NOT_RUN**.

### Windows 0.7.2 — L4 Tarefas, L5 MailCraft/Outlook, L6.1 Relay DO SQLite e heróis Lumen (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-14 para criar e publicar a release `0.7.2` localmente (`no-github-actions`).
- [x] Validações locais: `python.exe scripts/test-whats-new.py windows` (`0.7.2: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`692/692`), `npm run build` (`tsc --noEmit && vite build`), `cargo test --lib` (`701` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`), `npm test` em `android/relay/` (`26` worker + `37` node) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - MSI `Lumen-Windows-0.7.2.msi`: `8F64D8D37E3162F8D3912F44C52A27645BB3FDCE123D0DA098B0852FAE594DCB`.
  - Setup `Lumen-Windows-0.7.2-setup.exe`: `09A2FB08F1C9ECA30DB5DB07112D06895924CA14FA9A40D094790EB4805C8EED`.
  - App portátil `lumen.exe`: `EE76731F8E9C84C184B46407FA296003ECFEBEED5F9D99CA555EBF252334309C`.
  - Hook `lumen-hook.exe`: `E6FFA12AB1B94BE9A64859C8C789467136D74A414DCE3C1794AC2F5DFB59FD97`.
- [x] Publicação local via `gh release create windows-v0.7.2` em `playertwo1/lumen` e atualização da release móvel `windows-latest` (`Lumen-Windows.msi` e `Lumen-Windows-setup.exe`).
- [ ] Proprietário instalar a `0.7.2` (ou atualizar da `0.7.1` via `playertwo1/lumen`) e validar no uso real L3 (ditado e comentário sem duplicar), L4 (captura `Ctrl+Alt+K` e Toast nativo) e L5 (MailCraft resumo seguro e rascunho sem envio). Continua **NOT_RUN**.

### Windows 0.7.1 — 15 Personas da Segunda Mente, Sync via ADB e novos companheiros Canvas (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-14 para versionar como `0.7.1`, incorporar catálogo oficial das 15 personas da Segunda Mente com suporte YAML/Tavern, sync via celular Android (ADB) e novos companheiros Canvas (Grok, Hermes Desktop, ajustes no Antigravity e Genérico), empacotar e publicar localmente (`no-github-actions`).
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.1: 3 items in 1 languages`), `npx tsc --noEmit`, `npm test` (`681/681`), `npm run build` (`tsc --noEmit && vite build`), `cargo test --lib` (`680` passaram, `9` ignorados), `cargo test` em `windows/hook/` (`37/37`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou individualmente os quatro artefatos gerados: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos:
  - MSI `Lumen-Windows-0.7.1.msi`: `00F47FDA309C2965671456D3C8BEA26196017C6798D046A1BD3B71E62EB559C9`.
  - Setup `Lumen-Windows-0.7.1-setup.exe`: `AD18558C87CA5E138717C635EA9E6E132FED0CC1BBFE3DB52D7095D430514FA2`.
  - App portátil `lumen.exe`: `EDBE3C95FEA5948B57747D020E653647DD0D29B9918A24D1CC3D83DCFDCAB15C`.
  - Hook `lumen-hook.exe`: `B08BF0C549EFA0A19A4B10D72FD4FD97E3126E7ECEE7FDAC4E16715F78547CA0`.
- [x] Publicação local via `gh release create windows-v0.7.1` em `playertwo1/lumen` e atualização da release móvel `windows-latest` (`Lumen-Windows.msi` e `Lumen-Windows-setup.exe`).
- [ ] Proprietário instalar a `0.7.1` (ou atualizar da `0.7.0` via `playertwo1/lumen`), validar as 15 personas, sincronização ADB e novos companheiros no notebook. Continua **NOT_RUN**.

### Windows 0.7.0 — Etapa L3 (Anotador, personas e animação P3) e links completos para `playertwo1/lumen` (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-14 para versionar como `0.7.0`, apontar links/atualizador para o repositório próprio `playertwo1/lumen`, empacotar e publicar localmente (`no-github-actions`).
- [x] `update.rs` já apontava para `playertwo1/lumen` desde a `0.6.18`; `share.ts` e `whats-new.ts` foram atualizados para `playertwo1/lumen` nesta release. `lumen.exe` não contém a string `playertwo1/coucou` e contém `playertwo1/lumen`.
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.7.0: 3 items in 1 languages`), `npm run build` (`tsc --noEmit && vite build`), `npm test` (`676/676`), `cargo test --lib` (`676` passaram, `8` ignorados), `cargo test` em `windows/hook/` (`37/37`) e `npm run pack`.
- [x] Microsoft Defender (`MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`) examinou os quatro arquivos: código 0 e `found no threats`.
- [x] SHA-256:
  - MSI `Lumen-Windows-0.7.0.msi`: `FB7D6D625A598DF3CDA3E84C1171BB5F3C366BA12155AB30316ECD8BF5FD1F87`.
  - Setup `Lumen-Windows-0.7.0-setup.exe`: `E053F9A40B2EFE48683BB519F14AEC435882CB460DED4234FB13F848B4D26CF2`.
  - App `lumen.exe`: `1AA310EB0708E0A5FAAADE78C39683B627129909FF87E5616389FB522311DAC2`.
  - Hook `lumen-hook.exe`: `A9312C960360CC68A57828040519ACD122D969B46BD9BA6550363265C701C43E`.
- [ ] Proprietário instalar a `0.7.0` (ou atualizar da `0.6.18` via `playertwo1/lumen`), testar o Checkpoint L3 real no notebook e confirmar o fluxo de atualização em `playertwo1/lumen`. Continua **NOT_RUN**.

### Windows 0.6.18 — personagens (L1.4) e repositório próprio (publicação local)

- [x] Pedido do proprietário em 2026-11-14: publicar as correções dos personagens e sair do fork. Repositório novo público `playertwo1/lumen`, com o histórico e as tags do fork. O remote `origin` passa a ser `playertwo1/lumen`; o fork antigo virou o remote `fork`.
- [x] `update.rs` procura releases em `playertwo1/lumen`; a string antiga não aparece mais no `lumen.exe`. A 0.6.18 também é publicada em `playertwo1/coucou`, como ponte para os apps até a 0.6.17.
- [x] Validações locais: `python scripts/test-whats-new.py windows`, `npx tsc --noEmit`, `npm test` (674/674), `npm run build`, `cargo test --lib` (670 passaram, 8 ignorados) e `npm run pack`. GitHub Actions desabilitado no fork; build local.
- [x] Microsoft Defender, assinaturas `1.463.288.0`, examinou os quatro binários: código 0 e `found no threats`.
- [x] SHA-256:
  - MSI `Lumen-Windows-0.6.18.msi`: `B3AD2D04B2E8765B1DBC2B0D5CAAA1ADBF85153A38F411C9C66AC363016931A8`.
  - Setup `Lumen-Windows-0.6.18-setup.exe`: `16C9977BB697DE870BAFABC81E96F42B08C166D579AB4E1CAABF25178EF78FAC`.
  - App `lumen.exe`: `F1D67C33FC919A120CA9DF48331F4959D3C571A3156864D4683A8A0B32F61FEF`.
  - Hook `lumen-hook.exe`: `0587AF505AD0A2516522A1312883B72FE35710AD94141B1BD93D20EA7BDC976D`.
- [ ] Proprietário atualizar uma instalação 0.6.17 pelo atualizador do app (ponte pelo fork) e confirmar que a 0.6.18 procura a próxima versão em `playertwo1/lumen`. Continua **NOT_RUN**.

### Windows 0.6.17 — chat com internet e localização (publicação local)

- [x] Pedido do proprietário em 2026-11-14 para empacotar e, em seguida, autorização para publicar. Build local (GitHub Actions desabilitado). Os SHA-256 dos instaladores foram conferidos de novo e o Defender reexaminou o MSI e o setup sem ameaças antes da publicação. A tag aponta para o commit empacotado; a `main` recebeu depois um merge com as correções dos personagens (L1.4), ainda fora desta build.
- [x] Validações locais: `python3 scripts/test-whats-new.py windows`, `npx tsc --noEmit`, `npm test` (673/673), `npm run build`, `cargo test --lib` (670 passaram, 8 ignorados) e `npm run pack`.
- [x] Microsoft Defender examinou os quatro arquivos com `MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`: código 0 e `found no threats`.
- [x] SHA-256:
  - MSI `Lumen-Windows-0.6.17.msi`: `CA10E5338648F97663E3DA3C762E53834C1315456CBDF92C9135E47F22DB1ED4`.
  - Setup `Lumen-Windows-0.6.17-setup.exe`: `460611D9E2CF3DC4D123A524C98828C7D57D8377F9387C4C63BE20968684CB94`.
  - App `lumen.exe`: `5DACC5782177D6E0FAFF9AE17623E0A0EDC19560FF628F86F995D83AAFB2E2DF`.
  - Hook `lumen-hook.exe`: `180DDDB1139A02D9D21ED8A36CF7FD6C0A2F5AB34337EF106B4C4FF22DB7ED0E`.
- [ ] Proprietário instalar a 0.6.17 e testar o clima com a conta ChatGPT real e a cidade salva. Continua **NOT_RUN**.

### Windows 0.6.16 — correção do login ChatGPT (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-14. O GitHub Actions está desabilitado no repositório (`HTTP 422: Actions has been disabled`), então a build e a publicação foram locais, como na 0.6.15.
- [x] Causa corrigida: o Credential Manager do Windows recusa valores acima de 2560 bytes e os tokens OAuth passavam disso. `secrets.rs` divide valores longos em partes de 800 caracteres.
- [x] Validações locais: `python scripts/test-whats-new.py windows`, `npx tsc --noEmit`, `npm test` (673/673), `npm run build`, `cargo test --lib` (665 passaram, 8 ignorados) e `npm run pack`.
- [x] Microsoft Defender, assinaturas `1.463.288.0`, examinou individualmente os quatro binários com `MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`: código 0 e `found no threats`.
- [x] SHA-256 dos artefatos publicados:
  - MSI `Lumen-Windows-0.6.16.msi`: `5EB4DF72741D61B74D68612037AE93666ADF32BBCAE89D415848E208222E298C`.
  - Setup `Lumen-Windows-0.6.16-setup.exe`: `FE95D36E1523E2E2F6664650D7A68BEFEA36B32AF020DC6326741CB04F752AA8`.
  - App `lumen.exe`: `E32BF18F43F672710CD5706ACB73815C7C5DB45ACC4000398D239FEC7871834C`.
  - Hook `lumen-hook.exe`: `90E489E359E46C1DC97FD71158843B7BCEE37B73368EC4AA20996DE5802A5B54`.
- [ ] Proprietário validar o login real com ChatGPT (L2.1). Continua **NOT_RUN** até esse teste.

### Windows 0.6.15 — L1.4d-3/d-4 e L2.2 Chat com login ChatGPT (publicação local)

- [x] Pedido explícito do proprietário em 2026-11-14 para publicar `windows-v0.6.15` 100% localmente (`no-github-actions`), sem executar GitHub Actions nem rodar `test-lumen-upgrade.ps1` destrutivo no perfil local.
- [x] Validações locais: `python scripts/test-whats-new.py windows` (`0.6.15: 2 items in 1 languages`), `npm run icons`, `npx tsc --noEmit`, `npm test` (673/673), `cargo test --manifest-path src-tauri/Cargo.toml --lib` (664 passaram, 8 ignorados) e `npm run pack` (`Lumen-Windows-0.6.15.msi` 12,45 MB e `Lumen-Windows-0.6.15-setup.exe` 8,95 MB).
- [x] Microsoft Defender, assinaturas `1.463.288.0` (13/11/2026 15:38:53), examinou individualmente os quatro binários/instaladores locais com `MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`; os quatro retornaram código 0 e `found no threats`.
- [x] SHA-256 dos artefatos locais publicados:
  - MSI `Lumen-Windows-0.6.15.msi`: `BBCEC30907F2E01C1052D240BD8EC71E8D84866666AAF5C3E91194065A7EF7FE`.
  - Setup `Lumen-Windows-0.6.15-setup.exe`: `4BAD63F08E16291F3DFB0229430DA8C3E5CB69A021D7510BFBC7C09FBCBE2E47`.
  - App `lumen.exe`: `D352F490358D3B7CB3A860B2F49D3EE858EACC02789E3310689FD871EF2F7B5E`.
  - Hook `lumen-hook.exe`: `4A172A24F8C782313814907AB437638B9F1FF02D36A90F175A4709D824418F64`.
- [x] Publicação local via `gh release create windows-v0.6.15` e atualização dos assets de `windows-latest` (`Lumen-Windows.msi` e `Lumen-Windows-setup.exe`) apontando para os binários desta build. Release: https://github.com/playertwo1/coucou/releases/tag/windows-v0.6.15.
- [ ] Proprietário validar no uso real: personagens e animações P1/P2 (`L1.4`), cenários da migração (`L1.3`) e login + chat ChatGPT (`L2.1` + `L2.2`). Testes reais seguem **NOT_RUN** até essa verificação.

### Windows 0.6.14 — L2.1 Sign in with ChatGPT

- [x] Autorização recorrente do proprietário para publicar cada etapa depois dos testes sintéticos, registrada em `docs/ROADMAP-FORK.md`.
- [x] Validações locais: `npx tsc --noEmit`, `npm test` (659/659), `npm run build`, `cargo test --manifest-path windows/Cargo.toml -p coucou --lib` (660 passaram, 8 ignorados) e `python scripts/test-whats-new.py windows`.
- [x] Workflow manual `Windows` no branch `codex/l2-chatgpt-oauth`, commit `fb3dabafc8558584c5a2ba55e81077e654e66d0a`: CI `43477378762` concluiu com sucesso. Build, testes, notas, ícones, empacotamento e upgrade sintético MSI/NSIS passaram.
- [x] Microsoft Defender, assinaturas `1.463.281.0` (13/11/2026), examinou individualmente os quatro artefatos baixados com `MpCmdRun.exe -Scan -ScanType 3 -File <arquivo> -DisableRemediation`; os quatro retornaram código 0 e “found no threats”. A verificação personalizada da pasta também concluiu sem detecções (evento 1001, exame `{15EB8CCB-EF55-4D39-8DDF-4032CB8A1B5E}`).
- [x] SHA-256 dos artefatos do preflight:
  - MSI `Lumen-Windows-0.6.14.msi`: `41F3852B4F967C7CE0FA5455EED6D3ACA49A98C449E3F9DC6421A60ADA240CB0`.
  - Setup `Lumen-Windows-0.6.14-setup.exe`: `49556A2C5650CC9C519CCB073AB87B3CE614CBA8BF2686361E37B283D55A165F`.
  - App portátil `lumen.exe`: `326B17A034865EBFEB1D9B178404E487CBEA6EDE1C9B22E8155FF388A1609495`.
  - Hook `lumen-hook.exe`: `49C7725C391D4BAA1B9B36198ADCC043C48A58E5D5BAE33034F352F1C25771C9`.
- [x] Integrado na `main` no commit `3129f5f7036cf46ceffec66dbd0bbc85857f53c5`; tag `windows-v0.6.14` publicada. Workflow da release [`43482202733`](https://github.com/playertwo1/coucou/actions/runs/43482202733) concluiu com sucesso, incluindo testes Node/Rust, empacotamento, upgrade sintético MSI/NSIS, publicação e atualização de `windows-latest`. Release: https://github.com/playertwo1/coucou/releases/tag/windows-v0.6.14.
- [x] Baixados os assets finais; os SHA-256 locais conferem com os digests do GitHub. Os instaladores finais foram examinados individualmente pelo Defender, assinaturas `1.463.281.0` (13/11/2026), sem ameaças detectadas.
  - Setup `Lumen-Windows-0.6.14-setup.exe`: `7C9453C21D7051CEF5E97ED345E6AED2E59F2A1B44A4EA224A1A9B5226027E9E`.
  - MSI `Lumen-Windows-0.6.14.msi`: `E7DC4037F732454D1C1E701BB67BA3445149036A0C57E941990F0746F693238E`.
- [x] `windows-latest` aponta para os mesmos assets finais; os digests publicados são idênticos aos da release `windows-v0.6.14`.
- [ ] Proprietário validar login, consentimento, persistência, duas contas, troca e saída. Login real permanece **NOT_RUN** até esse teste.

### Windows 0.6.13 — Lumen

- [x] Pedido de publicação autorizado pelo proprietário; identidade Bússola de papel escolhida.
- [x] Validações locais: `npx tsc --noEmit`, `npm test` (659/659), `npm run build`, `cargo test --lib` (650 passaram, 8 ignorados) e `python scripts/test-whats-new.py`.
- [x] Preflight CI `43424866776` no commit `252639a1efcb670fc16d2b6f5f267688887b430b`: build/empacotamento, testes do app e Rust, notas de release, ícones e upgrades sintéticos MSI/NSIS passaram.
- [x] Microsoft Defender com assinaturas `1.463.281.0` examinou os artefatos baixados do preflight; nenhum arquivo foi detectado (evento 1001, 2026-11-13).
- [x] SHA-256 dos artefatos do preflight:
  - MSI `Lumen-Windows-0.6.13.msi`: `A73368E15A3A2DA184C933F8939D48BB43946307C5BA387EFBA28D3E43C77674`.
  - Setup `Lumen-Windows-0.6.13-setup.exe`: `E98CC0713117828F1E14E9F7A70EE4E26326C9CD61FD7C1BDA4549B4172A5F3A`.
  - App portátil `lumen.exe`: `36F318E2EA17528E9C749E1710E38D7B0B9A007EB9F021D5C9729433CE3818CC`.
  - Hook `lumen-hook.exe`: `3295B2EC302610524E58C3BB32AA84FC8D5CAAA4DD05DCEEABB3764EEA5CBF82`.
- [x] Commit `c3b2af2` enviado a `main`; tag `windows-v0.6.13` publicada. CI de release `43428181413` concluiu com sucesso: https://github.com/playertwo1/coucou/actions/runs/43428181413. Release: https://github.com/playertwo1/coucou/releases/tag/windows-v0.6.13.
- [x] Assets finais baixados; SHA-256 local confere com os digests do GitHub. Os hashes diferem do preflight, então os instaladores finais foram examinados separadamente pelo Defender; nenhuma ameaça detectada (assinaturas `1.463.281.0`, evento 1001, 2026-11-13).
  - Setup `Lumen-Windows-0.6.13-setup.exe`: `FC60E4148044527FC78A4977EDA9B06636591D5C00CBD3D9BC2DF41C55F9D6B9`.
  - MSI `Lumen-Windows-0.6.13.msi`: `856D5CE0C08E51D236233803DF8C01766C6358B61178CBF977A371691E23EEB6`.
- [x] Release móvel `windows-latest` atualizada e verificada: contém somente `Lumen-Windows-setup.exe` e `Lumen-Windows.msi`; os dois assets antigos `Coucou-Windows-*` foram removidos dessa tag móvel. A release histórica `windows-v0.6.11` permanece disponível.
- [x] `README.md` e `windows/README.md` apontam os downloads, atualizações e nomes dos instaladores para Lumen. A tabela herdada de interações no guia Windows está identificada como não validada no app instalado.
- [x] Os instaladores não têm assinatura Authenticode (`NotSigned`); o fluxo não exige certificado e o Windows pode mostrar um aviso do SmartScreen.
- [ ] Proprietário instalará e avaliará no uso real; L1 permanece em avaliação até esse retorno.
