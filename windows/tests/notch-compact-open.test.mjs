// Testes do Notch Aberto (5ª aba icônica de Tarefas, captura rápida e pílulas)
// e do Notch Compacto (Modo 2A adaptativo + Centro Contextual dinâmico).

import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { internals } from "./tauri.mjs";
import { compactCenter, effectiveMailUnread, focusTargetLabel } from "../src/core/compact.ts";
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
const { buildHeader, buildViews } = await import("../src/views/views.ts");
const { buildTaskCaptureView } = await import("../src/views/tasks.ts");
const { buildMailCard } = await import("../src/views/mail.ts");
const { Island } = await import("../src/island/island.ts");
const { Spotify } = await import("../src/core/spotify.ts");

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
  State.settings.openOnHover = false;
  State.settings.activeIntegrations = [];
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

test("header do Notch aberto tem 7 abas somente icônicas incluindo tarefas, Anotador e E-mail", () => {
  const actionsDone = [];
  const hubPages = [];
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
    openSettingsWindow: (page) => {
      hubPages.push(page ?? "general");
    },
    blip: () => {},
    chooseOutfit: () => {},
    previewOutfit: () => {},
    keyboard: () => {},
    holdOpen: () => {},
    launched: () => {},
  });

  const tabs = header.el.find(".tab");
  assert.equal(tabs.length, 7);
  for (const tab of tabs) {
    assert.equal(tab.textContent, "");
    assert.equal(tab.find("svg").length, 1);
  }

  const tasksTab = tabs[4];
  assert.equal(tasksTab.getAttribute("title"), "Nova tarefa");
  tasksTab.fire("click");
  assert.deepEqual(actionsDone, ["taskCapture"]);

  const notesTab = tabs[5];
  assert.equal(notesTab.getAttribute("title"), "Abrir Anotador");
  notesTab.fire("click");

  const mailTab = tabs[6];
  assert.equal(mailTab.getAttribute("title"), "Abrir E-mail (Visão geral)");
  mailTab.fire("click");
  assert.deepEqual(hubPages, ["notes", "mailOverview"]);

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

test("faixa do WhatsApp exige integração ativa e conectada e abre o cartão pelo foco", ({ mock }) => {
  const root = initialDom.root();
  const prevRaf = globalThis.requestAnimationFrame;
  let island;
  // Evita laço infinito síncrono de frames em fake DOM
  globalThis.requestAnimationFrame = () => 1;
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    State.settings.openOnHover = true;
    island = new Island(root);
    island.fsm.forcePetit();
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
      link: { state: "needsQr" },
      conversations: [{ id: "c1", name: "Maria", unread: 1, messages: [] }],
      unread: 1,
      sending: false,
      sendError: null,
      down: false,
    };

    State.settings.activeIntegrations = ["integration_whatsapp"];
    island.syncDom();
    assert.equal(centerBtn.classList.contains("on"), false, "link desconectado");

    WhatsApp.view.link = { state: "connected" };
    WhatsApp.view.on = false;
    island.syncDom();
    assert.equal(centerBtn.classList.contains("on"), false, "integração desligada");

    WhatsApp.view.on = true;
    const whatsappTask = State.tasks.pop();
    island.syncDom();
    assert.equal(centerBtn.classList.contains("on"), false, "pílula ausente");
    State.tasks.push(whatsappTask);

    State.settings.activeIntegrations = [];
    island.syncDom();
    assert.equal(centerBtn.classList.contains("on"), false, "pílula residual após desativar");
    State.settings.activeIntegrations = ["integration_whatsapp"];

    WhatsApp.view.unread = 2;
    WhatsApp.view.conversations = [];
    island.syncDom();
    assert.equal(centerBtn.classList.contains("on"), true);
    assert.match(centerBtn.textContent, /WhatsApp · 2 novas/);
    assert.equal(island.targetSize().w, COMPACT_W);

    island.onCursor(window.innerWidth / 2, 1);
    assert.equal(island.wasInIsland, true);
    mock.timers.tick(500);
    assert.equal(State.mode, "compact", "a abertura automática não deve tirar a faixa antes do clique");

    // Clique no Centro Contextual abre o Notch expandido focado no WhatsApp
    centerBtn.fire("click");
    assert.equal(State.mode, "expanded");
    assert.equal(State.view, "overview");
    assert.equal(State.focusId, "integration_whatsapp");
    island.syncDom();
    assert.equal(root.find(".wa-card").length, 1);
  } finally {
    island?.fsm.cancelTimers();
    globalThis.requestAnimationFrame = prevRaf;
  }
});

test("effectiveMailUnread silencia os 357 avisos ao limpar e só volta a notificar com novos e-mails", () => {
  assert.equal(effectiveMailUnread(357, null), 357);
  assert.equal(effectiveMailUnread(357, 357), 0);
  assert.equal(effectiveMailUnread(350, 357), 0);
  assert.equal(effectiveMailUnread(359, 357), 2);
  assert.equal(effectiveMailUnread(0, 357), 0);
});

test("botão Limpar no card do Mail remove o aviso de 357 não lidos tanto na lista quanto no detalhe e no notch compacto", () => {
  State.mailUnreadDismissedAt = null;
  State.integrations.integration_mail = {
    configured: true,
    loaded: true,
    error: null,
    data: {
      running: true,
      windows: true,
      backend: "mailcraft",
      unread: 357,
      messages: [
        {
          storeId: "mailcraft",
          entryId: "m1",
          senderName: "AliExpress",
          address: "promo@example.com",
          subject: "Você viu, agora economize",
          date: new Date().toISOString(),
          unread: true,
          flagged: false,
        },
      ],
      shown: 12,
      maxShown: 60,
    },
  };

  const card = buildMailCard({ keyboard: () => {} });
  card.sync();

  assert.equal(card.el.find(".mail-unread")[0]?.textContent, "357");
  const clearBtnList = card.el.find(".mail-clear-btn")[0];
  assert.ok(clearBtnList);

  // Entra no detalhe (como no print do usuário)
  card.el.find(".mail-row")[0].fire("click");
  const clearBtnDetail = card.el.querySelector(".mail-clear-btn.detail");
  assert.ok(clearBtnDetail);

  clearBtnDetail.fire("click");
  assert.equal(State.mailUnreadDismissedAt, 357);

  card.sync();
  assert.equal(card.el.querySelector(".mail-clear-btn.detail"), null);
  assert.equal(card.el.find(".mail-unread")[0]?.style.display, "none");
  assert.equal(
    compactCenter({
      now: new Date(),
      pendingApproval: null,
      notice: null,
      whatsapp: null,
      mailUnread: effectiveMailUnread(357, State.mailUnreadDismissedAt),
      todos: [],
      music: null,
    }),
    null,
  );
  State.mailUnreadDismissedAt = null;
});

test("seta ↗ Abrir tem tooltip contextual e aciona openTarget para Mail, Codex, Claude e Antigravity", () => {
  assert.equal(focusTargetLabel("integration_mail", "Mail", "mailcraft"), "Abrir MailCraft");
  assert.equal(focusTargetLabel("integration_mail", "Mail", "outlook"), "Abrir Outlook");
  assert.equal(focusTargetLabel("agent_codex", "Codex"), "Abrir Codex");
  assert.equal(focusTargetLabel("agent_antigravity", "Antigravity"), "Abrir Antigravity");
  assert.equal(focusTargetLabel("integration_claude", "Claude Code"), "Abrir Claude Code");
  assert.equal(focusTargetLabel("agent_claude-desktop", "Claude Desktop"), "Abrir Claude Desktop");

  let targetClicks = 0;
  State.tasks = [
    { id: "agent_codex", name: "Codex", color: "#2DD4BF", state: "idle", steps: [], stepIndex: 0, source: "agent", isIntegration: true },
    { id: "agent_antigravity", name: "Antigravity", color: "#E879F9", state: "idle", steps: [], stepIndex: 0, source: "agent", isIntegration: true },
  ];
  State.focusId = "agent_codex";
  State.view = "overview";

  const overview = buildViews({
    setView: () => {},
    cancelDrop: () => {},
    draftDroppedFile: async () => {},
    collapse: () => {},
    foldApproval: () => {},
    setFocus: () => {},
    openTerminal: () => {},
    openTarget: () => { targetClicks += 1; },
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
  }, () => {}).get("overview");
  assert.ok(overview);

  overview.sync();
  const jump = overview.el.querySelector(".icon-btn.jump");
  assert.ok(jump);
  assert.equal(jump.getAttribute("title"), "Abrir Codex");
  jump.fire("click");
  assert.equal(targetClicks, 1);

  State.focusId = "agent_antigravity";
  overview.sync();
  assert.equal(jump.getAttribute("title"), "Abrir Antigravity");
});
