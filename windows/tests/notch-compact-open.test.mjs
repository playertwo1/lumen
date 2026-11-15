// Testes do Notch Aberto (5ª aba icônica de Tarefas, captura rápida e pílulas)
// e do Notch Compacto (Modo 2A adaptativo + Centro Contextual dinâmico).

import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { internals } from "./tauri.mjs";
import { compactCenter } from "../src/core/compact.ts";
import { COMPACT_W, NOTCH_H, botPosition, compactWidth } from "../src/core/layout.ts";

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
  globalThis.window ??= globalThis;
  globalThis.window.addEventListener ??= () => {};
  globalThis.window.removeEventListener ??= () => {};
  globalThis.window.dispatchEvent ??= () => true;
  globalThis.window.innerWidth ??= 720;
  globalThis.window.devicePixelRatio ??= 1;
  globalThis.document.addEventListener ??= () => {};
  globalThis.document.removeEventListener ??= () => {};
  return dom;
}

const initialDom = setupDom();
const { State } = await import("../src/core/state.ts");
const { WhatsApp } = await import("../src/core/whatsapp.ts");
const { Spotify } = await import("../src/core/spotify.ts");
const { buildHeader } = await import("../src/views/views.ts");
const { buildTaskCaptureView } = await import("../src/views/tasks.ts");
const { Island } = await import("../src/island/island.ts");

const defaultInvoke = internals.invoke;

function makeTodo(overrides = {}) {
  return {
    id: "todo-1",
    text: "Enviar relatório mensal",
    due: "",
    due_date: "2026-11-15",
    due_time: "15:00",
    time_zone: "America/Sao_Paulo",
    alert_minutes: 20,
    priority: "normal",
    subtasks: [],
    recurrence: null,
    series_id: null,
    done: false,
    created_at: "2026-11-15T10:00:00Z",
    done_at: null,
    snoozed_until: null,
    source: null,
    source_stale: false,
    version: 1,
    deleted_at: null,
    ...overrides,
  };
}

function resetRuntimeState() {
  State.tasks = [];
  State.focusId = "integration_claude";
  State.mode = "hidden";
  State.view = "overview";
  State.pendingApproval = null;
  State.integrations = {};
  State.settings.showClock = false;
  State.settings.showPlanInCompactIsland = false;
  State.settings.mochiReactsToPc = false;
  WhatsApp.view = {
    on: false,
    link: { state: "off" },
    conversations: [],
    unread: 0,
    sending: false,
    sendError: null,
    down: false,
  };
  Spotify.state.track = null;
  Spotify.state.playing = false;
}

beforeEach(() => {
  internals.invoke = async () => undefined;
  internals.metadata = { currentWindow: { label: "main" }, currentWebview: { label: "main" } };
  resetRuntimeState();
});

afterEach(() => {
  internals.invoke = defaultInvoke;
  delete internals.metadata;
  resetRuntimeState();
});

test("geometria compacta (2A) usa altura 36px e largura enxuta quando sem centro contextual", () => {
  assert.equal(NOTCH_H, 36);
  assert.equal(botPosition("compact", "overview", NOTCH_H).cy, 18);

  assert.equal(compactWidth(false, 0, false), 92);
  assert.equal(compactWidth(false, 4, false), 136);
  assert.equal(compactWidth(false, 0, true), 140);
  assert.equal(compactWidth(false, 4, true), 196);
  assert.ok(compactWidth(false, 4, true) < COMPACT_W);

  assert.equal(compactWidth(true, 0, false), COMPACT_W);
  assert.equal(compactWidth(true, 4, true), COMPACT_W);
});

test("compactCenter retorna null quando ocioso e respeita prioridades de contexto", () => {
  const now = new Date(2026, 10, 15, 14, 0, 0);
  const idle = {
    now,
    pendingApproval: null,
    notice: null,
    whatsapp: null,
    mailUnread: 0,
    todos: [],
    music: null,
  };
  assert.equal(compactCenter(idle), null);

  // Tarefa > 2h no futuro, concluída ou adiada não polui o centro
  assert.equal(
    compactCenter({
      ...idle,
      todos: [
        makeTodo({ id: "t-later", due_time: "17:00" }),
        makeTodo({ id: "t-done", due_time: "14:30", done: true }),
        makeTodo({ id: "t-snooze", due_time: "14:10", snoozed_until: new Date(now.getTime() + 600_000).toISOString() }),
      ],
    }),
    null,
  );

  // Tarefa dentro da janela (<= 2h) aparece
  const soon = compactCenter({
    ...idle,
    todos: [makeTodo({ id: "t-soon", due_time: "15:15", text: "Reunião de produto" })],
  });
  assert.equal(soon?.kind, "task");
  assert.equal(soon?.text, "15:15 · Reunião de produto");

  // Tarefa atrasada hoje ganha flag urgent e prefixo Atrasada
  const overdue = compactCenter({
    ...idle,
    todos: [makeTodo({ id: "t-late", due_time: "13:30", text: "Enviar nota" })],
  });
  assert.equal(overdue?.kind, "task");
  assert.equal(overdue?.urgent, true);
  assert.equal(overdue?.text, "Atrasada · Enviar nota");

  // Mail > WhatsApp > Aviso > Aprovação respeitam hierarquia
  const withMail = compactCenter({
    ...idle,
    mailUnread: 3,
    todos: [makeTodo({ due_time: "15:00" })],
  });
  assert.equal(withMail?.kind, "mail");
  assert.equal(withMail?.text, "Mail · 3 não lidos");

  const withWa = compactCenter({
    ...idle,
    mailUnread: 3,
    whatsapp: { unread: 1, conversations: [{ name: "Ana", unread: 1 }] },
  });
  assert.equal(withWa?.kind, "whatsapp");
  assert.equal(withWa?.text, "Nova msg · Ana");

  const withNotice = compactCenter({
    ...idle,
    notice: { text: "Nota salva", color: "#34D399", expiresAt: now.getTime() + 5000 },
    whatsapp: { unread: 2, conversations: [] },
  });
  assert.equal(withNotice?.kind, "notice");
  assert.equal(withNotice?.text, "Nota salva");

  const expiredNotice = compactCenter({
    ...idle,
    notice: { text: "Antiga", color: "#34D399", expiresAt: now.getTime() - 1 },
    music: { title: "Midnight City", artist: "M83" },
  });
  assert.equal(expiredNotice?.kind, "music");
  assert.equal(expiredNotice?.note, true);

  const withApproval = compactCenter({
    ...idle,
    pendingApproval: { pillName: "Claude Code", questions: null },
    notice: { text: "Nota salva", color: "#34D399", expiresAt: now.getTime() + 5000 },
  });
  assert.equal(withApproval?.kind, "approval");
  assert.equal(withApproval?.urgent, true);
  assert.equal(withApproval?.text, "Aprovação · Claude Code");
});

test("header do Notch aberto tem 5 abas somente icônicas incluindo captura de tarefas", () => {
  const actionsDone = [];
  const header = buildHeader({
    setView: (v) => {
      actionsDone.push(v);
      State.view = v;
    },
    cancelDrop: () => {},
    draftDroppedFile: async () => {},
    collapse: () => {},
    foldApproval: () => {},
    setFocus: () => {},
    openTerminal: () => {},
    openTarget: () => {},
    openUrl: () => {},
    decide: () => {},
    answer: () => {},
    answerInTerminal: () => {},
    toggleSound: () => {},
    setVolume: () => {},
    setAutoClose: () => {},
    openSettingsWindow: () => {},
    blip: () => {},
    chooseOutfit: () => {},
    previewOutfit: () => {},
    keyboard: () => {},
    holdOpen: () => {},
    launched: () => {},
  });

  const tabs = header.el.find(".tab");
  assert.equal(tabs.length, 5);
  for (const tab of tabs) {
    assert.equal(tab.textContent, "");
    assert.equal(tab.find("svg").length, 1);
  }

  const tasksTab = tabs[4];
  assert.equal(tasksTab.getAttribute("title"), "Nova tarefa");
  tasksTab.fire("click");
  assert.deepEqual(actionsDone, ["taskCapture"]);

  header.sync();
  assert.equal(tasksTab.classList.contains("on"), true);

  State.view = "taskConfirm";
  header.sync();
  assert.equal(tasksTab.classList.contains("on"), true);

  State.view = "overview";
  header.sync();
  assert.equal(tasksTab.classList.contains("on"), false);
  assert.equal(tabs[0].classList.contains("on"), true);
});

test("view taskCapture mostra cabeçalho rápido, botão Criar proposta e aviso de confirmação", () => {
  const host = buildTaskCaptureView({
    setView: () => {},
    cancelDrop: () => {},
    draftDroppedFile: async () => {},
    collapse: () => {},
    foldApproval: () => {},
    setFocus: () => {},
    openTerminal: () => {},
    openTarget: () => {},
    openUrl: () => {},
    decide: () => {},
    answer: () => {},
    answerInTerminal: () => {},
    toggleSound: () => {},
    setVolume: () => {},
    setAutoClose: () => {},
    openSettingsWindow: () => {},
    blip: () => {},
    chooseOutfit: () => {},
    previewOutfit: () => {},
    keyboard: () => {},
    holdOpen: () => {},
    launched: () => {},
  });

  const titles = host.el.find(".title").map((e) => e.textContent);
  assert.equal(titles.includes("Nova tarefa rápida"), true);
  assert.equal(host.el.find(".task-propose-btn")[0]?.textContent, "Criar proposta");
  assert.equal(
    host.el.find(".task-capture-hint")[0]?.textContent,
    "Datas ambíguas sempre pedem confirmação",
  );
});

test("Island no modo compacto alterna entre 2A enxuto e Centro Contextual quando chega mensagem", () => {
  const root = initialDom.root();
  const prevRaf = globalThis.requestAnimationFrame;
  // Evita laço infinito síncrono de frames em fake DOM
  globalThis.requestAnimationFrame = () => 1;
  try {
    const island = new Island(root);
    State.mode = "compact";
    State.notify();

    // Simula 1 passo de frame para sincronizar o DOM sem aguardar RAF infinito
    island.syncDom();
    const centerBtn = [...root.walk()].find((e) => e.attributes.get("id") === "compact-center");
    assert.ok(centerBtn);
    assert.equal(centerBtn.classList.contains("on"), false);
    assert.equal(island.targetSize().w, 92);

    // Mensagem no WhatsApp ativa o Centro Contextual e expande para COMPACT_W
    State.tasks = [
      {
        id: "integration_claude",
        name: "Claude Code",
        color: "#60A5FA",
        state: "idle",
        steps: [],
        stepIndex: 0,
        source: "hooks",
      },
      {
        id: "integration_whatsapp",
        name: "WhatsApp",
        color: "#22C55E",
        state: "idle",
        steps: [],
        stepIndex: 0,
        source: "whatsapp",
      },
    ];
    WhatsApp.view = {
      on: true,
      link: { state: "open" },
      conversations: [{ id: "c1", name: "Maria", unread: 1, messages: [] }],
      unread: 1,
      sending: false,
      sendError: null,
      down: false,
    };

    island.syncDom();
    assert.equal(centerBtn.classList.contains("on"), true);
    assert.match(centerBtn.textContent, /Nova msg · Maria/);
    assert.equal(island.targetSize().w, COMPACT_W);

    // Clique no Centro Contextual abre o Notch expandido focado no WhatsApp
    centerBtn.fire("click");
    assert.equal(State.mode, "expanded");
    assert.equal(State.view, "overview");
    assert.equal(State.focusId, "integration_whatsapp");
    island.fsm.cancelTimers();
  } finally {
    globalThis.requestAnimationFrame = prevRaf;
  }
});
