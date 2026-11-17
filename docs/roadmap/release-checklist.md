> Procedimento de release do fork. Índice: [ROADMAP-FORK.md](../ROADMAP-FORK.md). O registro de cada release publicada fica em [arquivadas/releases/releases-windows.md](../arquivadas/releases/releases-windows.md).

# Checklist de release Windows (só com pedido explícito do proprietário)

A autorização recorrente de 2026-11-12 cobre publicar cada etapa concluída depois dos testes sintéticos. Publicar não fecha checkpoint real.

1. [ ] Pedido do proprietário registrado (data e o que entra na versão).
2. [ ] Validações locais: `python scripts/test-whats-new.py windows`, em `windows/` `npx tsc --noEmit`, `npm test` e `npm run build`, em `windows/src-tauri/` `cargo test --lib`.
3. [ ] CHANGELOG com a seção da versão, linha na tabela de versões do README e What's new em `windows/src/whats-new/whats-new.json`.
4. [ ] A versão é a mesma em `windows/package.json`, `windows/package-lock.json`, `windows/Cargo.toml`, nas entradas `coucou` e `coucou-hook` de `windows/Cargo.lock` e em `windows/src-tauri/tauri.conf.json`. `scripts/test-lumen-upgrade.ps1` lê a versão nova do `package.json`.
5. [ ] Preflight: rodar o workflow `Windows` (`windows.yml`) no branch da release, baixar os artefatos e examiná-los com o Microsoft Defender. O upgrade sintético MSI/NSIS tem de preservar os dados.
6. [ ] Merge na `main`, push para `origin` (`playertwo1/lumen`) e tag `windows-vX.Y.Z`. Conferir o workflow da tag, a release e os assets (`Lumen-Windows-X.Y.Z-setup.exe`, `Lumen-Windows-X.Y.Z.msi`) e o `windows-latest`.
7. [ ] Baixar os instaladores finais, conferir o SHA-256 com os digests do GitHub e examiná-los com o Defender (os hashes costumam diferir do preflight). Instaladores sem assinatura Authenticode; não comprar certificado.
8. [ ] Registrar a release no topo de [releases-windows.md](../arquivadas/releases/releases-windows.md): pedido, validações, workflow, hashes e o que fica **NOT_RUN** para o proprietário validar. O que ficar pendente entra também nas pendências do [STATE](../STATE-FORK.md).

Nenhuma release usa o nome ou os assets do Coucou depois da L1.
