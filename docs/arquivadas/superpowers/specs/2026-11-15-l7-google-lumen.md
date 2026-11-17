# Spec — Fase L7: Google no Lumen (OAuth PKCE, Backup Drive, Google Calendar e P3)

**Data:** 2026-11-15  
**Fase:** [Fase L — Lumen](../../etapas/l7-google.md) (`L7.1`, `L7.2`, `L7.3`, P3)  
**Decisões relacionadas:** `D-054`, `D-055`, `D21`, `D24`

## Objetivo

Integrar a conta Google pessoal do usuário no Lumen Desktop (Windows) de ponta a ponta com isolamento de segredos, segurança contra gravações destrutivas e controle explícito do usuário:

1. **L7.1 — Login Google próprio (OAuth 2.0 PKCE + Credential Manager):**
   - Fluxo OAuth 2.0 PKCE com listener loopback local (`127.0.0.1:<porta>` em `/oauth2callback`).
   - Nenhuma credencial `client_id` ou `client_secret` é hardcoded no repositório (`D-055`): o usuário informa seu próprio `Client ID` (ou JSON exportado do Google Cloud Console `installed`/`web`) em Configurações, salvo no Credential Manager via `secrets.rs`, com fallback opcional às variáveis de ambiente `LUMEN_GOOGLE_CLIENT_ID` / `LUMEN_GOOGLE_CLIENT_SECRET`.
   - Verificação em tempo constante do parâmetro `state` (`subtle::ConstantTimeEq`).
   - Escopos mínimos necessários para perfil básico (`openid`, `email`, `profile`), Drive limitado aos arquivos/pastas criados pelo app (`https://www.googleapis.com/auth/drive.file`) e leitura/escrita de agenda (`https://www.googleapis.com/auth/calendar.events`).
   - Refresh de token serializado via `Mutex` (`REFRESH_LOCK`), sem gravar tokens no disco ou em logs.
   - Tokens e metadados ficam protegidos no `secrets.rs` em uma lista dedicada (`GOOGLE_INTERNAL_KEYS`) verificada por `is_internal()`, inacessível à webview e sem alterar `INTERNAL_KEYS` do `phone_link`.

2. **L7.2 — Backup semanal no Google Drive (`Segunda Mente/Lumen-PC`) e restauração segura:**
   - Hierarquia fixa no Drive (`D-054`, `D21`): pasta raiz `Segunda Mente` e subpasta isolada por dispositivo `Lumen-PC`, sem misturar arquivos com o `Galaxy-S25-Ultra`.
   - Pacote único versionado `lumen-backup-latest.json` (`schema_version: 1`) contendo tarefas (`tasks.json`, `tasks_sync.json`), notas (`Notes/<id>/note.json`), personas e preferências de comentários (`personas.json`, `settings.json`), settings gerais não-sensíveis do usuário e um digest `sha256` calculado sobre o payload canônico.
   - **Exclusões obrigatórias:** nenhum segredo do Credential Manager e nenhum arquivo de áudio `.wav` entram no backup.
   - Scheduler em background semanal (a cada 7 dias quando habilitado e autenticado), com disparo manual imediato (`google_backup_now`) e emissão do evento `"google-backup-completed"` para a Ilha e o Hub.
   - Restauração (`google_restore_backup`): baixa e valida `schema_version == 1` e o `sha256` antes de tocar no disco; grava um snapshot de segurança local (`BackupRestoreSafety/pre-restore.json`) antes de sobrescrever os arquivos; recarrega os stores em memória (`TaskManager::reload_from_disk`, `NoteTaker::reload_from_disk`) sem exigir reinício do app.

3. **L7.3 — Google Calendar (leitura + evento criado somente com confirmação explícita):**
   - Leitura de eventos futuros/próximos (`GET /calendar/v3/calendars/primary/events?singleEvents=true&orderBy=startTime...`).
   - Criação de evento a partir de uma tarefa existente exige:
     - `confirmed: true` explicitamente enviado após confirmação visual do usuário na interface;
     - data explícita na tarefa (`override_date` ou `due_date`);
     - recusa silenciosa/erro claro se não houver confirmação ou data;
     - nunca envia `attendees` (zero convites a terceiros) e envia `sendUpdates=none`;
     - nunca edita nem apaga eventos existentes no Google Calendar.

4. **Animação P3 da L7 (`triggerCloudBackup`):**
   - Em `CompassEngine` (`windows/src/lumen/compass-engine.ts`), ao receber `"google-backup-completed"`, a Ilha exibe uma nuvem estilizada guardando uma caixinha conectada com brilho suave azul/verde, respeitando `prefers-reduced-motion` (nenhuma animação executada quando reduzido).

## Escopo que NÃO faz parte da L7

- Acesso automático a contas corporativas / verificação de app público do Google (uso pessoal conforme `D-055`).
- Leitura ou envio de Gmail.
- Edição ou remoção automática de eventos no Google Calendar.
- Sync bidirecional de estado com a subpasta Android (`Galaxy-S25-Ultra`) no Drive.

## Verificação e Critério de Aceite

- **Automatizado (offline / sintético):**
  - `cargo test --lib` em `windows/src-tauri/` passando 100%, incluindo testes de `secrets`, `phone_link`, loopback HTTP mock para OAuth PKCE (rejeição de `state` inválido, troca de código, refresh), Drive (hierarquia `Segunda Mente/Lumen-PC`, exclusão de segredos/`.wav`, rejeição de SHA-256 inválido na restauração) e Calendar (leitura, recusa sem `confirmed: true` ou sem data, ausência de `attendees` e presença de `sendUpdates=none`).
  - `npx tsc --noEmit`, `npm test` e `npm run build` em `windows/` passando 100%.
- **Real (`Checkpoint L7`):**
  - Login real com a conta pessoal Google Cloud, upload/restauração real na pasta `Segunda Mente/Lumen-PC` e criação de evento confirmado no Calendar primário ficam explicitamente marcados como `NOT_RUN` até validação manual pelo dono em build local com credenciais reais.
