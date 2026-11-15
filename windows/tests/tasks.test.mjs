// Tarefas (L4): buckets, atalhos de agenda, busca sem acento, parser de proposta no chat
// e card editável com regra "IA propõe, usuário confirma".

import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, emit, internals, sent } from "./tauri.mjs";
import {
  bucketFor,
  dateForShortcut,
  formatDueLabel,
  formatTimeMinutes,
  groupTasks,
  searchTodos,
  splitTodoProposal,
  timeChipLabel,
} from "../src/core/tasks.ts";
import { chatTodoProposalCard } from "../src/views/tasks.ts";

Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "Windows NT" } });
globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__ ??= { unregisterListener() {} };

function setupDom() {
  const dom = installFakeDom();
  const baseCreate = document.createElement;
  document.createElement = (tag) => {
    const el = baseCreate(tag);
    if (tag === "canvas") el.getContext = () => null;
    return el;
  };
  return dom;
}

const initialDom = setupDom();
const { tasksPage } = await import("../src/hub/pages/tasks.ts");
const { State } = await import("../src/core/state.ts");
const { Island } = await import("../src/island/island.ts");

const flush = () => new Promise((r) => setTimeout(r, 0));
const defaultInvoke = internals.invoke;
let customInvoke = null;
let pageCtx = null;

function makeSync(overrides = {}) {
  return {
    state: "local_only",
    paired: false,
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
    ...overrides,
  };
}

function makeSnapshot(overrides = {}) {
  return {
    items: [fakeTodo()],
    shortcuts: {
      enabled_date_shortcuts: ["today", "tomorrow", "inThreeDays", "nextMonday"],
      time_shortcuts_minutes: [540, 840, 1200],
      default_alert_minutes: 20,
    },
    windows_notifications_available: true,
    voice_ready: false,
    recording: false,
    quarantined_file: null,
    sync: makeSync(),
    ...overrides,
  };
}

async function openTasksPage(snapshot, extra = {}) {
  const container = initialDom.root();
  const ctx = {};
  pageCtx = ctx;
  customInvoke = async (cmd, args) => {
    if (cmd in extra) return extra[cmd](args);
    if (cmd === "tasks_state") return snapshot;
    return undefined;
  };
  await tasksPage(container, ctx);
  return container;
}

function fakeTodo(overrides = {}) {
  return {
    id: "t-1",
    text: "Revisão do relatório mensal",
    due: "",
    due_date: "2026-11-14",
    due_time: "14:00",
    time_zone: "America/Sao_Paulo",
    alert_minutes: 20,
    priority: "normal",
    subtasks: [],
    recurrence: null,
    series_id: null,
    done: false,
    created_at: "2026-11-14T10:00:00Z",
    done_at: null,
    snoozed_until: null,
    source: null,
    source_stale: false,
    version: 1,
    deleted_at: null,
    ...overrides,
  };
}

beforeEach(() => {
  setupDom();
  calls.length = 0;
  customInvoke = null;
  pageCtx = null;
  internals.invoke = async (cmd, args) => {
    if (customInvoke) {
      const handled = await customInvoke(cmd, args);
      if (handled !== undefined) {
        calls.push([cmd, args]);
        return handled;
      }
    }
    return defaultInvoke(cmd, args);
  };
});

afterEach(() => {
  pageCtx?.onLeave?.();
  pageCtx = null;
  customInvoke = null;
  internals.invoke = defaultInvoke;
});

test("agrupa Hoje, Próximas, Atrasadas, Sem data e Concluídas ignorando tombstones", () => {
  const items = [
    fakeTodo({ id: "atrasada", due_date: "2026-11-12" }),
    fakeTodo({ id: "hoje", due_date: "2026-11-14" }),
    fakeTodo({ id: "proxima", due_date: "2026-11-18" }),
    fakeTodo({ id: "semdata", due_date: null, due_time: null }),
    fakeTodo({ id: "feita", due_date: "2026-11-14", done: true, done_at: "2026-11-14T11:00:00Z" }),
    fakeTodo({ id: "apagada", deleted_at: "2026-11-14T12:00:00Z" }),
  ];
  assert.equal(bucketFor(items[0], "2026-11-14"), "overdue");
  assert.equal(bucketFor(items[1], "2026-11-14"), "today");
  assert.equal(bucketFor(items[2], "2026-11-14"), "upcoming");
  assert.equal(bucketFor(items[3], "2026-11-14"), "unscheduled");
  assert.equal(bucketFor(items[4], "2026-11-14"), "done");

  const grouped = groupTasks(items, "2026-11-14");
  assert.deepEqual(grouped.overdue.map((i) => i.id), ["atrasada"]);
  assert.deepEqual(grouped.today.map((i) => i.id), ["hoje"]);
  assert.deepEqual(grouped.upcoming.map((i) => i.id), ["proxima"]);
  assert.deepEqual(grouped.unscheduled.map((i) => i.id), ["semdata"]);
  assert.deepEqual(grouped.done.map((i) => i.id), ["feita"]);
});

test("atalhos de data e horário (L4.2) calculam Hoje, Amanhã, +3 dias, Próxima segunda e 9h/14h/20h", () => {
  // Sábado 2026-11-14 local -> próxima segunda é 2026-11-16
  const saturday = new Date(2026, 10, 14, 10, 0, 0);
  assert.equal(dateForShortcut("today", saturday), "2026-11-14");
  assert.equal(dateForShortcut("tomorrow", saturday), "2026-11-15");
  assert.equal(dateForShortcut("inThreeDays", saturday), "2026-11-17");
  assert.equal(dateForShortcut("nextMonday", saturday), "2026-11-16");

  // Segunda 2026-11-16 local -> próxima segunda pula +7 dias (2026-11-23)
  const monday = new Date(2026, 10, 16, 8, 0, 0);
  assert.equal(dateForShortcut("nextMonday", monday), "2026-11-23");

  assert.equal(formatTimeMinutes(540), "09:00");
  assert.equal(formatTimeMinutes(840), "14:00");
  assert.equal(formatTimeMinutes(1200), "20:00");
  assert.equal(timeChipLabel(540), "9h");
  assert.equal(timeChipLabel(840), "14h");
  assert.equal(timeChipLabel(1200), "20h");
});

test("busca sem acento e case-insensitive inclui título e subtarefas", () => {
  const items = [
    fakeTodo({
      id: "consulta",
      text: "Marcar revisão médica anual",
      subtasks: [{ id: "s1", text: "Levar exame de sangue", done: false }],
    }),
    fakeTodo({ id: "outra", text: "Comprar café" }),
  ];
  assert.deepEqual(searchTodos(items, "revisao medica").map((i) => i.id), ["consulta"]);
  assert.deepEqual(searchTodos(items, "EXAME SANGUE").map((i) => i.id), ["consulta"]);
  assert.deepEqual(searchTodos(items, "cafe").map((i) => i.id), ["outra"]);
});

test("splitTodoProposal extrai proposta do chat e descarta datas ambíguas sem adivinhar", () => {
  const md = [
    "Posso registrar isso para você.",
    "```lumen-todo",
    JSON.stringify({
      id: "p-amb",
      title: "Enviar contrato assinado",
      date_state: "ambiguous",
      due_date: "2026-11-20",
      date_phrase: "sexta",
      time_state: "ambiguous",
      due_time: "09:00",
      time_phrase: "de manhã",
      priority: "high",
    }),
    "```",
  ].join("\n");

  const { text, proposal, isAmbiguous } = splitTodoProposal(md);
  assert.equal(text, "Posso registrar isso para você.");
  assert.ok(proposal);
  assert.equal(isAmbiguous, true);
  // Segurança: datas/horas marcadas como ambiguous nunca passam como preenchidas.
  assert.equal(proposal.due_date, null);
  assert.equal(proposal.due_time, null);
  assert.equal(proposal.date_phrase, "sexta");
  assert.equal(proposal.time_phrase, "de manhã");
});

test("chatTodoProposalCard nunca grava sem clique explícito em Confirmar", async () => {
  customInvoke = (cmd, args) => {
    if (cmd !== "tasks_confirm_proposal") return undefined;
    const { input } = args;
    return fakeTodo({
      id: "saved-99",
      text: input.text,
      due_date: input.due_date,
      due_time: input.due_time,
      alert_minutes: input.alert_minutes,
      priority: input.priority,
    });
  };

  const card = chatTodoProposalCard({
    id: "prop-chat-1",
    title: "Pagar guia",
    date_state: "ambiguous",
    due_date: null,
    date_phrase: "sexta",
    time_state: "unspecified",
    due_time: null,
    time_phrase: null,
    priority: "normal",
  });

  // Montar o cartão não pode disparar gravação automática.
  assert.equal(calls.length, 0);
  const banner = card.find(".task-ambiguous-banner")[0];
  assert.ok(banner.textContent.includes("sexta"));

  // Usuário escolhe "Amanhã" e "14h" e clica em Confirmar.
  const chips = card.find(".task-chip");
  const amanha = chips.find((b) => b.textContent === "Amanhã");
  const h14 = chips.find((b) => b.textContent === "14h");
  assert.ok(amanha && h14);
  amanha.fire("click");
  h14.fire("click");

  const confirmBtn = card.find("BUTTON").find((b) => b.textContent === "Confirmar");
  assert.ok(confirmBtn);
  confirmBtn.fire("click");
  await new Promise((r) => setTimeout(r, 0));

  const confirmCalls = sent("tasks_confirm_proposal");
  assert.equal(confirmCalls.length, 1);
  assert.equal(confirmCalls[0].input.due_date, dateForShortcut("tomorrow"));
  assert.equal(confirmCalls[0].input.due_time, "14:00");
  assert.equal(formatDueLabel({ due_date: "2099-01-02", due_time: "09:00", alert_minutes: 20 }), "02/01/2099 · 09:00 · aviso 20m");
});

test("L6.3: local_only, sync ausente e saved_on_pc exibem 'Salvo neste PC' e nunca 'Sincronizado'", async () => {
  const localPage = await openTasksPage(makeSnapshot({ sync: makeSync({ state: "local_only", paired: false }) }));
  const localBar = localPage.querySelector(".tasks-sync-bar");
  assert.ok(localBar.textContent.includes("Salvo neste PC · celular não pareado"));
  assert.equal(localPage.querySelector(".tasks-sync-now-btn"), null);
  assert.equal(localPage.textContent.includes("Sincronizado"), false);
  for (const pill of localPage.find(".tasks-item-sync")) {
    assert.equal(pill.textContent, "Salvo neste PC");
  }

  // Tolerância a snapshots sem o campo sync
  const legacySnap = makeSnapshot();
  delete legacySnap.sync;
  const legacyPage = await openTasksPage(legacySnap);
  assert.ok(legacyPage.querySelector(".tasks-sync-bar").textContent.includes("Salvo neste PC · celular não pareado"));
  assert.equal(legacyPage.textContent.includes("Sincronizado"), false);

  // saved_on_pc com alterações pendentes
  const pendingPage = await openTasksPage(
    makeSnapshot({
      sync: makeSync({
        state: "saved_on_pc",
        paired: true,
        pending_outbox: 2,
        unsynced_item_ids: ["t-1"],
      }),
    }),
  );
  const pendingBar = pendingPage.querySelector(".tasks-sync-bar");
  assert.ok(pendingBar.textContent.includes("Salvo neste PC · 2 alterações pendentes para enviar"));
  assert.ok(pendingPage.querySelector(".tasks-sync-now-btn"));
  assert.equal(pendingPage.textContent.includes("Sincronizado"), false);
});

test("L6.3: waiting_phone mantém item em 'Salvo neste PC' até 'Sincronizar agora' concluir em 'synced'", async () => {
  const items = [
    fakeTodo({ id: "t-recente", text: "Item recém-enviado" }),
    fakeTodo({ id: "t-antigo", text: "Item anterior" }),
  ];
  const syncedSnap = makeSnapshot({
    items,
    sync: makeSync({
      state: "synced",
      paired: true,
      pending_outbox: 0,
      pending_peer: 0,
      last_synced_at: "2026-11-15T02:00:00Z",
      unsynced_item_ids: [],
    }),
  });

  const page = await openTasksPage(
    makeSnapshot({
      items,
      sync: makeSync({
        state: "waiting_phone",
        paired: true,
        pending_outbox: 0,
        pending_peer: 1,
        unsynced_item_ids: ["t-recente"],
      }),
    }),
    { tasks_sync_now: () => syncedSnap },
  );

  const bar = page.querySelector(".tasks-sync-bar");
  assert.ok(bar.textContent.includes("Salvo neste PC · aguardando o celular abrir a caixa"));
  assert.equal(page.textContent.includes("Sincronizado"), false);
  for (const pill of page.find(".tasks-item-sync")) {
    assert.equal(pill.textContent, "Salvo neste PC");
  }

  const syncBtn = page.querySelector(".tasks-sync-now-btn");
  assert.ok(syncBtn);
  assert.equal(syncBtn.textContent, "Sincronizar agora");
  syncBtn.fire("click");
  await flush();

  assert.equal(sent("tasks_sync_now").length, 1);
  assert.ok(bar.textContent.includes("Sincronizado"));
  assert.equal(bar.textContent.includes("Salvo neste PC"), false);
  for (const pill of page.find(".tasks-item-sync")) {
    assert.equal(pill.textContent, "Sincronizado");
  }

  // Mesmo em state === "synced", item ainda listado em unsynced_item_ids continua "Salvo neste PC"
  const partialPage = await openTasksPage(
    makeSnapshot({
      items,
      sync: makeSync({
        state: "synced",
        paired: true,
        unsynced_item_ids: ["t-recente"],
      }),
    }),
  );
  const pills = partialPage.find(".tasks-item").map((card) => ({
    id: card.getAttribute("data-task-id"),
    syncText: card.querySelector(".tasks-item-sync").textContent,
  }));
  assert.deepEqual(pills, [
    { id: "t-recente", syncText: "Salvo neste PC" },
    { id: "t-antigo", syncText: "Sincronizado" },
  ]);
});

test("L6.3: estados revoked, error, quarentena/poison e falha de tasks_sync_now", async () => {
  const revokedPage = await openTasksPage(
    makeSnapshot({
      sync: makeSync({ state: "revoked", paired: true, revoked: true }),
    }),
  );
  assert.ok(revokedPage.querySelector(".tasks-sync-bar").textContent.includes("Salvo neste PC · pareamento revogado"));
  assert.equal(revokedPage.textContent.includes("Sincronizado"), false);

  const errorPage = await openTasksPage(
    makeSnapshot({
      sync: makeSync({
        state: "error",
        paired: true,
        last_error: "Tempo esgotado no servidor",
        quarantined_count: 1,
        poison_count: 2,
      }),
    }),
    {
      tasks_sync_now: () => {
        throw new Error("Sem conexão com a caixa");
      },
    },
  );
  const barText = errorPage.querySelector(".tasks-sync-bar").textContent;
  assert.ok(barText.includes("Salvo neste PC · falha na última sincronização (Tempo esgotado no servidor)"));
  assert.ok(barText.includes("1 atualização em quarentena"));
  assert.ok(barText.includes("2 mensagens inválidas ignoradas"));
  assert.equal(errorPage.textContent.includes("Sincronizado"), false);

  const syncBtn = errorPage.querySelector(".tasks-sync-now-btn");
  syncBtn.fire("click");
  await flush();
  const warnBanner = errorPage.querySelector(".tasks-banner.warn");
  assert.equal(warnBanner.hidden, false);
  assert.ok(warnBanner.textContent.includes("Sem conexão com a caixa"));
  assert.equal(errorPage.querySelector(".tasks-sync-now-btn").disabled, false);
});

test("L6.3: resolução inline de conflitos chama tasks_resolve_conflict com conflictId e choice", async () => {
  const item = fakeTodo({ id: "t-conf", text: "Título vencedor temporário" });
  const conflictTitle = {
    id: "conf-1",
    entity_id: "t-conf",
    field: "text",
    local_text: "Texto deste PC",
    remote_text: "Texto do celular",
    winner_side: "remote",
    winner_text: "Texto do celular",
    loser_text: "Texto deste PC",
    local_updated_at: "2026-11-15T01:00:00Z",
    remote_updated_at: "2026-11-15T01:05:00Z",
    conflict_state: "pending_review",
  };
  const conflictSub = {
    id: "conf-2",
    entity_id: "t-conf",
    field: "subtask",
    local_text: "Subtarefa PC",
    remote_text: "Subtarefa celular",
    winner_side: "local",
    winner_text: "Subtarefa PC",
    loser_text: "Subtarefa celular",
    local_updated_at: "2026-11-15T01:06:00Z",
    remote_updated_at: "2026-11-15T01:02:00Z",
    conflict_state: "pending_review",
  };

  let currentSnap = makeSnapshot({
    items: [item],
    sync: makeSync({
      state: "synced",
      paired: true,
      conflicts: [conflictTitle, conflictSub],
    }),
  });

  const page = await openTasksPage(currentSnap, {
    tasks_resolve_conflict: ({ conflictId, choice }) => {
      if (conflictId === "conf-1" && choice === "local") {
        currentSnap = makeSnapshot({
          items: [fakeTodo({ id: "t-conf", text: "Texto deste PC" })],
          sync: makeSync({
            state: "saved_on_pc",
            paired: true,
            pending_outbox: 1,
            conflicts: [conflictSub],
            unsynced_item_ids: ["t-conf"],
          }),
        });
      } else if (conflictId === "conf-2" && choice === "remote") {
        currentSnap = makeSnapshot({
          items: [fakeTodo({ id: "t-conf", text: "Texto deste PC", subtasks: [{ id: "s1", text: "Subtarefa celular", done: false }] })],
          sync: makeSync({
            state: "saved_on_pc",
            paired: true,
            pending_outbox: 1,
            conflicts: [],
            unsynced_item_ids: ["t-conf"],
          }),
        });
      }
      return currentSnap;
    },
  });

  const cards = page.find(".tasks-conflict-card");
  assert.equal(cards.length, 2);
  assert.ok(cards[0].textContent.includes("Conflito de edição (Título)"));
  assert.ok(cards[0].textContent.includes("Neste PC: Texto deste PC"));
  assert.ok(cards[0].textContent.includes("No celular: Texto do celular"));
  assert.ok(cards[1].textContent.includes("Conflito de edição (Subtarefa)"));

  const keepLocal = cards[0].find("BUTTON").find((b) => b.textContent === "Manter texto deste PC");
  assert.ok(keepLocal);
  keepLocal.fire("click");
  await flush();

  assert.deepEqual(sent("tasks_resolve_conflict")[0], { conflictId: "conf-1", choice: "local" });
  assert.equal(page.querySelector(".tasks-item-title").textContent, "Texto deste PC");
  assert.equal(page.find(".tasks-conflict-card").length, 1);

  const useRemote = page.find(".tasks-conflict-card")[0].find("BUTTON").find((b) => b.textContent === "Usar texto do celular");
  assert.ok(useRemote);
  useRemote.fire("click");
  await flush();

  assert.deepEqual(sent("tasks_resolve_conflict")[1], { conflictId: "conf-2", choice: "remote" });
  assert.equal(page.find(".tasks-conflict-card").length, 0);
});

test("L6.3: task-dismiss dispensa lembrete ativo na Ilha apenas para o taskId correspondente e recarrega página", async () => {
  const oldMode = State.mode;
  const oldView = State.view;
  try {
    State.mode = "hidden";
    State.view = "overview";
    const island = Object.create(Island.prototype);
    island.engine = { triggerEmote() {} };
    island.ensureRunning = () => {};
    island.fsm = {
      pinned: false,
      forceHome: () => island.expand("taskReminder"),
      forcePetit: () => island.setMode("compact"),
    };
    island.setMode = (mode) => { State.mode = mode; };
    island.animateGeometry = () => {};
    island.expand = (view) => {
      State.view = view;
      State.mode = "expanded";
    };
    island.alert = (view) => {
      State.view = view;
      State.mode = "expanded";
    };

    island.taskReminder({
      id: "t-dismiss-1",
      text: "Tomar remédio",
      due_date: "2026-11-15",
      due_time: "09:00",
      priority: "high",
      alert_minutes: 0,
    });
    assert.equal(State.mode, "expanded");
    assert.equal(State.view, "taskReminder");

    // Outro ID não dispensa o card ativo
    island.taskDismiss("t-outro");
    assert.equal(State.mode, "expanded");
    assert.equal(State.view, "taskReminder");

    // ID correspondente recolhe a Ilha e sai da view taskReminder
    island.taskDismiss("t-dismiss-1");
    assert.equal(State.mode, "compact");
    assert.notEqual(State.view, "taskReminder");

    // Na página Tarefas, o evento task-dismiss dispara recarga de tasks_state
    let stateCalls = 0;
    await openTasksPage(makeSnapshot(), {
      tasks_state: () => {
        stateCalls++;
        return makeSnapshot();
      },
    });
    assert.equal(stateCalls, 1);
    emit("task-dismiss", "t-dismiss-1");
    await flush();
    assert.equal(stateCalls, 2);
  } finally {
    State.mode = oldMode;
    State.view = oldView;
  }
});

test("L6.4: taskSyncState aciona gesto P3 (saved vs synced) na Bússola e fallback seguro", () => {
  const island = Object.create(Island.prototype);
  let syncStateCalled = null;
  let ensured = 0;
  island.ensureRunning = () => { ensured++; };
  island.engine = {
    triggerTaskSync: (state) => { syncStateCalled = state; },
    triggerEmote: () => {},
  };

  island.taskSyncState("saved");
  assert.equal(syncStateCalled, "saved");
  assert.equal(ensured, 1);

  island.taskSyncState("synced");
  assert.equal(syncStateCalled, "synced");
  assert.equal(ensured, 2);

  // Fallback quando triggerTaskSync não existe na engine
  let emoteCalled = null;
  island.engine = {
    triggerEmote: (emote) => { emoteCalled = emote; },
  };
  island.taskSyncState("synced");
  assert.equal(emoteCalled, "happy");
  assert.equal(ensured, 3);

  island.taskSyncState("saved");
  assert.equal(emoteCalled, "wink");
  assert.equal(ensured, 4);
});
