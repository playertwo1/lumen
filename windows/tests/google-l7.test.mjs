import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals, sent } from "./tauri.mjs";

Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "Windows NT" },
});
globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__ ??= { unregisterListener() {} };

const initialDom = installFakeDom();
const defaultInvoke = internals.invoke;
let responder = null;
let pageCtx = null;

internals.invoke = async (cmd, args) => {
  const res = await defaultInvoke(cmd, args);
  if (cmd !== "plugin:event|listen" && responder) {
    return responder(cmd, args);
  }
  return res;
};

beforeEach(() => {
  calls.length = 0;
  responder = null;
  pageCtx = null;
});

afterEach(() => {
  pageCtx?.onLeave?.();
  pageCtx = null;
  responder = null;
  calls.length = 0;
});

const { GoogleBridge } = await import("../src/core/bridge.ts");
const { tasksPage } = await import("../src/hub/pages/tasks.ts");

function connectedGoogleStatus(overrides = {}) {
  return {
    configured: true,
    clientIdPreview: "12345678...usercontent.com",
    hasClientSecret: false,
    connected: true,
    email: "usuario@exemplo.test",
    displayName: "Usuário Teste",
    scopes: [
      "openid",
      "email",
      "https://www.googleapis.com/auth/drive.file",
      "https://www.googleapis.com/auth/calendar.events",
    ],
    driveFolder: "Segunda Mente/Lumen-PC",
    weeklyBackupEnabled: true,
    lastBackupAt: "2026-11-15T10:00:00Z",
    lastBackupAtMs: 1763200800000,
    lastRestoreAt: null,
    lastBackupError: null,
    ...overrides,
  };
}

function makeSnapshot(items) {
  return {
    items,
    shortcuts: {
      enabled_date_shortcuts: ["today", "tomorrow", "inThreeDays", "nextMonday"],
      time_shortcuts_minutes: [540, 840, 1200],
      default_alert_minutes: 20,
    },
    windows_notifications_available: true,
    voice_ready: false,
    recording: false,
    quarantined_file: null,
    sync: {
      state: "synced",
      paired: true,
      device_id: "pc-test-1",
      pending_outbox: 0,
      pending_peer: null,
      last_synced_at: null,
      last_error: null,
      quarantined_count: 0,
      poison_count: 0,
      revoked: false,
      conflicts: [],
      unsynced_item_ids: [],
    },
  };
}

test("L7.3: GoogleBridge.createEventFromTask recusa enviar sem confirmed=true e só invoca quando confirmado", async () => {
  responder = async (cmd, args) => {
    if (cmd === "google_calendar_create_from_task") {
      return {
        id: "ev-1",
        summary: "Enviar proposta",
        description: null,
        location: null,
        start: "2026-11-20T14:00:00-03:00",
        end: "2026-11-20T14:30:00-03:00",
        allDay: false,
        htmlLink: null,
        status: "confirmed",
      };
    }
    return args ?? null;
  };

  await assert.rejects(
    GoogleBridge.createEventFromTask({
      taskId: "t-1",
      confirmed: false,
      overrideDate: "2026-11-20",
      overrideTime: "14:00",
    }),
    /confirmação/i,
  );
  assert.equal(sent("google_calendar_create_from_task").length, 0);

  const created = await GoogleBridge.createEventFromTask({
    taskId: "t-1",
    confirmed: true,
    overrideDate: "2026-11-20",
    overrideTime: "14:00",
    durationMinutes: 30,
  });
  assert.equal(created.id, "ev-1");
  const invoked = sent("google_calendar_create_from_task");
  assert.equal(invoked.length, 1);
  assert.equal(invoked[0].input.confirmed, true);
});

test("L7.2 e L7.3 na página Tarefas exigem confirmação dupla para restaurar backup e confirmar evento no Calendar", async () => {
  const sampleItem = {
    id: "t-cal-1",
    text: "Revisar orçamento Q4",
    due: "2026-11-20 15:00",
    due_date: "2026-11-20",
    due_time: "15:00",
    time_zone: "America/Sao_Paulo",
    alert_minutes: 15,
    priority: "high",
    subtasks: [],
    recurrence: null,
    series_id: null,
    done: false,
    created_at: "2026-11-15T09:00:00Z",
    done_at: null,
    snoozed_until: null,
    source: null,
    source_stale: false,
    version: 1,
    deleted_at: null,
  };

  responder = async (cmd) => {
    if (cmd === "tasks_state") return makeSnapshot([sampleItem]);
    if (cmd === "google_status") return connectedGoogleStatus();
    if (cmd === "google_calendar_list_events") {
      return [
        {
          id: "ev-upcoming",
          summary: "Alinhamento semanal",
          description: null,
          location: null,
          start: "2026-11-18T10:00:00-03:00",
          end: "2026-11-18T11:00:00-03:00",
          allDay: false,
          htmlLink: null,
          status: "confirmed",
        },
      ];
    }
    if (cmd === "google_restore_backup") {
      return {
        backupCreatedAt: "2026-11-15T10:00:00Z",
        sha256: "abc",
        tasksRestored: true,
        notesRestored: 2,
        personasRestored: true,
        settingsRestored: true,
        safetySnapshot: "BackupRestoreSafety/pre-restore.json",
      };
    }
    if (cmd === "google_calendar_create_from_task") {
      return {
        id: "ev-created",
        summary: "Revisar orçamento Q4",
        description: null,
        location: null,
        start: "2026-11-20T15:00:00-03:00",
        end: "2026-11-20T15:30:00-03:00",
        allDay: false,
        htmlLink: null,
        status: "confirmed",
      };
    }
    return null;
  };

  const host = initialDom.root();
  pageCtx = {};
  await tasksPage(host, pageCtx);
  await new Promise((r) => setTimeout(r, 20));

  assert.ok(host.textContent.includes("Google Calendar e Backup"));
  assert.ok(host.textContent.includes("Alinhamento semanal"));
  assert.ok(host.textContent.includes("Segunda Mente/Lumen-PC"));

  // Restauração exige dois cliques (Restaurar backup -> Confirmar restauração)
  const restoreBtn = host.find("BUTTON").find((b) => b.textContent === "Restaurar backup");
  assert.ok(restoreBtn);
  restoreBtn.fire("click");
  assert.equal(sent("google_restore_backup").length, 0);

  const confirmRestoreBtn = host.find("BUTTON").find((b) => b.textContent === "Confirmar restauração");
  assert.ok(confirmRestoreBtn);
  confirmRestoreBtn.fire("click");
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(sent("google_restore_backup").length, 1);

  // Abrir inspetor da tarefa e agendar no Calendar exige preview + confirmação
  const editBtn = host.querySelector(".tasks-edit-btn");
  assert.ok(editBtn);
  editBtn.fire("click");
  await new Promise((r) => setTimeout(r, 10));

  const scheduleCalBtn = host.find("BUTTON").find((b) => b.textContent === "Agendar no Google Calendar");
  assert.ok(scheduleCalBtn);
  scheduleCalBtn.fire("click");
  assert.equal(sent("google_calendar_create_from_task").length, 0);
  assert.ok(host.textContent.includes("Sem convidados"));

  const confirmCalBtn = host.find("BUTTON").find((b) => b.textContent === "Confirmar evento no Calendar");
  assert.ok(confirmCalBtn);
  confirmCalBtn.fire("click");
  await new Promise((r) => setTimeout(r, 20));

  const createdCalls = sent("google_calendar_create_from_task");
  assert.equal(createdCalls.length, 1);
  assert.equal(createdCalls[0].input.taskId, "t-cal-1");
  assert.equal(createdCalls[0].input.confirmed, true);
});
