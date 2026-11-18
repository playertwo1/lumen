// Fluxo de confirmação na ilha para ferramentas internas do Mochi (M1.3):
// hooks.ts + state.ts + island.ts + views.ts (buildApproval) + i18n.

import { after, afterEach, beforeEach, mock, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, emit, internals, sent } from "./tauri.mjs";

Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "Windows NT" },
});
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

setupDom();

const { DEFAULT_SETTINGS, State } = await import("../src/core/state.ts");
const { registerHookHandlers } = await import("../src/island/hooks.ts");
const { Island } = await import("../src/island/island.ts");
const { setLanguage } = await import("../src/i18n/i18n.ts");

const realPerfNow = performance.now.bind(performance);
const realRaf = globalThis.requestAnimationFrame;

let nowMs = 1_000;
const root = document.createElement("div");
globalThis.requestAnimationFrame = () => 1;
internals.metadata = { currentWindow: { label: "island" }, currentWebview: { label: "island" } };
State.settings = {
  ...DEFAULT_SETTINGS,
  mochiReactsToPc: false,
  closedIsland: "hidden",
};
const island = new Island(root);
registerHookHandlers(island);
globalThis.requestAnimationFrame = realRaf;

const hook = (payload) => emit("hook", payload);
const seconds = (n) => mock.timers.tick(n * 1000);
const advanceClock = (ms) => {
  nowMs += ms;
};

function mochiPayload(requestId, overrides = {}) {
  return {
    hook_event_name: "PermissionRequest",
    request_id: requestId,
    session_id: "mochi",
    coucou_kind: "mochi_tool",
    tool_name: "med_probe",
    mochi_summary: "Salvar rascunho local do resumo.",
    mochi_risk: "medium",
    mochi_allow_always: true,
    tool_input: { command: "Salvar rascunho local do resumo." },
    ...overrides,
  };
}

function approvalButtons() {
  island.syncDom();
  const view = [...root.walk()].find((el) => el.classList.contains("view") && el.classList.contains("on"));
  assert.ok(view, "deve existir uma view ativa");
  const actions = view.querySelector(".actions");
  assert.ok(actions, "deve existir linha de botões .actions");
  return actions.find("button");
}

function approvalCardParts() {
  island.syncDom();
  const card = [...root.walk()].find(
    (el) => el.classList.contains("view") && el.classList.contains("on"),
  )?.querySelector(".card.wash");
  assert.ok(card, "cartão âmbar deve estar visível");
  return {
    card,
    who: card.querySelector(".who-row"),
    code: card.querySelector(".code"),
    fold: card.querySelector(".icon-btn.fold"),
    buttons: card.querySelector(".actions").find("button"),
  };
}

beforeEach(() => {
  setLanguage("en");
  nowMs = 1_000;
  performance.now = () => nowMs;
  globalThis.requestAnimationFrame = () => 1;
  mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  internals.metadata = { currentWindow: { label: "island" }, currentWebview: { label: "island" } };
  State.tasks = [];
  State.focusId = null;
  State.mode = "hidden";
  State.view = "overview";
  State.paused = false;
  State.isPinned = false;
  State.pendingApproval = null;
  State.approvalQueue = [];
  State.focusBeforeApproval = null;
  State.settings = {
    ...DEFAULT_SETTINGS,
    activeIntegrations: ["integration_claude", ...DEFAULT_SETTINGS.activeIntegrations],
    mochiReactsToPc: false,
    closedIsland: "hidden",
  };
  State.loadIntegrationTasks();
  State.focusId = "integration_claude";
  island.fsm.pinned = false;
  island.fsm.cancelTimers();
  island.fsm.state = "hidden";
});

afterEach(async () => {
  mock.timers.runAll();
  mock.timers.reset();
  await new Promise((resolve) => setImmediate(resolve));
  performance.now = realPerfNow;
  globalThis.requestAnimationFrame = realRaf;
  setLanguage("en");
});

after(() => {
  delete internals.metadata;
});

test("receives mochi_tool without altering focus or creating a fake pill, and declines when paused", () => {
  State.setFocus("integration_n8n");
  const beforeIds = State.tasks.map((t) => t.id);

  hook(mochiPayload("m-1"));

  assert.equal(State.focusId, "integration_n8n");
  assert.deepEqual(State.tasks.map((t) => t.id), beforeIds);
  assert.equal(State.tasks.some((t) => t.id === "mochi" || t.id === "agent_mochi"), false);
  assert.equal(State.tasks.find((t) => t.id === "integration_claude")?.state, "idle");
  assert.equal(State.pendingApproval?.requestId, "m-1");
  assert.equal(State.pendingApproval?.coucouKind, "mochi_tool");
  assert.equal(State.pendingApproval?.mochiRisk, "medium");
  assert.equal(State.pendingApproval?.allowAlways, true);
  assert.deepEqual(sent("approval_ack"), [{ requestId: "m-1" }]);
  assert.deepEqual(sent("permission_toast_show"), [{
    requestId: "m-1",
    sessionId: "mochi",
    agentId: "mochi",
    wasHidden: true,
  }]);

  // Enquanto pausado, declina de imediato sem abrir cartão.
  State.pendingApproval = null;
  State.approvalQueue = [];
  State.paused = true;
  calls.length = 0;
  hook(mochiPayload("m-paused"));
  assert.equal(State.pendingApproval, null);
  assert.deepEqual(sent("approval_decline"), [{ requestId: "m-paused" }]);
  assert.deepEqual(sent("approval_ack"), []);
});

test("renders tool, summary, risk and Always allow only for medium risk (including pt-BR)", () => {
  setLanguage("pt-BR");
  hook(mochiPayload("m-med", {
    tool_name: "notes_edit",
    mochi_summary: "Atualizar nota de reunião.",
    mochi_risk: "medium",
    mochi_allow_always: true,
  }));

  const med = approvalCardParts();
  assert.match(med.who.textContent, /Lumen/);
  assert.match(med.who.textContent, /notes_edit/);
  assert.match(med.who.textContent, /Risco: Médio/);
  assert.equal(med.code.textContent, "Atualizar nota de reunião.");
  assert.deepEqual(
    med.buttons.map((b) => b.find("span")[0].textContent),
    ["Negar", "Permitir", "Sempre permitir"],
  );

  // Risco alto: nunca mostra "Sempre permitir", mesmo se payload vier adulterado com mochi_allow_always=true.
  State.pendingApproval = null;
  hook(mochiPayload("m-high", {
    tool_name: "delete_all",
    mochi_summary: "Apagar arquivo definitivo.",
    mochi_risk: "high",
    mochi_allow_always: true,
  }));

  assert.equal(State.pendingApproval?.allowAlways, false);
  const high = approvalCardParts();
  assert.match(high.who.textContent, /delete_all/);
  assert.match(high.who.textContent, /Risco: Alto/);
  assert.equal(high.code.textContent, "Apagar arquivo definitivo.");
  assert.deepEqual(
    high.buttons.map((b) => b.find("span")[0].textContent),
    ["Negar", "Permitir"],
  );

  // Risco baixo (caso seja exibido no cartão): também não exibe "Sempre permitir".
  State.pendingApproval = null;
  hook(mochiPayload("m-low", {
    tool_name: "ping",
    mochi_summary: "Executar ping local.",
    mochi_risk: "low",
    mochi_allow_always: false,
  }));
  const low = approvalCardParts();
  assert.match(low.who.textContent, /Risco: Baixo/);
  assert.equal(low.buttons.length, 2);
});

test("Allow, Always allow and Deny send the right decision for the active request", () => {
  // 1) Allow
  hook(mochiPayload("m-allow"));
  const allowBtns = approvalButtons();
  advanceClock(700);
  allowBtns[1].fire("click");
  assert.deepEqual(sent("approval_decision"), [{ requestId: "m-allow", decision: "allow" }]);
  assert.equal(State.pendingApproval, null);

  // 2) Always allow (Medium)
  calls.length = 0;
  hook(mochiPayload("m-always"));
  const alwaysBtns = approvalButtons();
  advanceClock(700);
  alwaysBtns[2].fire("click");
  assert.deepEqual(sent("approval_decision"), [{ requestId: "m-always", decision: "always" }]);
  assert.equal(State.pendingApproval, null);

  // 3) Deny
  calls.length = 0;
  hook(mochiPayload("m-deny", { mochi_risk: "high", mochi_allow_always: false }));
  const denyBtns = approvalButtons();
  assert.equal(denyBtns.length, 2);
  advanceClock(700);
  denyBtns[0].fire("click");
  assert.deepEqual(sent("approval_decision"), [{ requestId: "m-deny", decision: "deny" }]);
  assert.equal(State.pendingApproval, null);
});

test("fold keeps request pending in compact mode while collapse/escape preserve it and decline denies", () => {
  hook(mochiPayload("m-fold"));
  const { fold } = approvalCardParts();
  assert.equal(State.mode, "expanded");
  assert.equal(State.view, "approval");

  fold.fire("click");
  assert.equal(State.mode, "compact");
  assert.equal(State.pendingApproval?.requestId, "m-fold");
  assert.equal(State.isPinned, true);
  assert.deepEqual(sent("approval_decision"), []);
  assert.deepEqual(sent("approval_decline"), []);

  island.syncDom();
  const compactCenter = [...root.walk()].find((el) => el.attributes.get("id") === "compact-center");
  assert.ok(compactCenter);
  assert.match(compactCenter.textContent, /Aprovação · Lumen/);

  // Clicar no centro compacto reabre o cartão pendente.
  compactCenter.fire("click");
  assert.equal(State.mode, "expanded");
  assert.equal(State.view, "approval");
  assert.equal(State.pendingApproval?.requestId, "m-fold");

  // collapse() também só recolhe sem decidir.
  island.collapse();
  assert.equal(State.mode, "compact");
  assert.equal(State.pendingApproval?.requestId, "m-fold");

  // Reabre e testa declinar / cancelar via approval-expired.
  compactCenter.fire("click");
  emit("approval-expired", "m-fold");
  assert.equal(State.pendingApproval, null);
  assert.equal(State.isPinned, false);
  assert.deepEqual(sent("approval_decision"), []);
});

test("timeout drops pending mochi_tool request after 110s without allowing", () => {
  hook(mochiPayload("m-timeout"));
  assert.equal(State.pendingApproval?.requestId, "m-timeout");

  seconds(109);
  assert.equal(State.pendingApproval?.requestId, "m-timeout");

  seconds(1);
  assert.equal(State.pendingApproval, null);
  assert.equal(State.isPinned, false);
  assert.deepEqual(sent("approval_decision"), []);
});

test("two queued mochi requests require separate clicks guarded by CLICK_GUARD_MS and cap at MAX_PENDING", () => {
  State.setFocus("integration_github");

  hook(mochiPayload("q-1", {
    tool_name: "tool_one",
    mochi_summary: "Primeira ação média.",
    mochi_risk: "medium",
    mochi_allow_always: true,
  }));
  hook(mochiPayload("q-2", {
    tool_name: "tool_two",
    mochi_summary: "Segunda ação crítica.",
    mochi_risk: "high",
    mochi_allow_always: false,
  }));

  assert.equal(State.pendingApproval?.requestId, "q-1");
  assert.deepEqual(State.approvalQueue.map((x) => x.requestId), ["q-2"]);

  const btns1 = approvalButtons();
  assert.equal(btns1.length, 3);

  // Clique dentro do guarda (< 600 ms) é ignorado.
  advanceClock(300);
  btns1[2].fire("click");
  assert.deepEqual(sent("approval_decision"), []);
  assert.equal(State.pendingApproval?.requestId, "q-1");

  // Passados 600 ms, o 1º clique decide somente q-1 e promove q-2.
  advanceClock(350);
  btns1[2].fire("click");
  assert.deepEqual(sent("approval_decision"), [{ requestId: "q-1", decision: "always" }]);
  assert.equal(State.pendingApproval?.requestId, "q-2");
  assert.deepEqual(State.approvalQueue, []);
  assert.equal(State.focusId, "integration_github");

  // Mesmo um clique imediato antes de syncDom() é bloqueado pelo guarda de troca de requestId.
  btns1[1].fire("click");
  assert.deepEqual(sent("approval_decision"), [{ requestId: "q-1", decision: "always" }]);
  assert.equal(State.pendingApproval?.requestId, "q-2");

  // Após syncDom(), q-2 mostra risco alto sem "Always allow" e ainda bloqueia até 600 ms.
  const btns2 = approvalButtons();
  assert.equal(btns2.length, 2);
  advanceClock(400);
  btns2[0].fire("click");
  assert.equal(State.pendingApproval?.requestId, "q-2");

  // Após cumprir os 600 ms do 2º cartão, o novo clique decide q-2.
  advanceClock(250);
  btns2[0].fire("click");
  assert.deepEqual(sent("approval_decision"), [
    { requestId: "q-1", decision: "always" },
    { requestId: "q-2", decision: "deny" },
  ]);
  assert.equal(State.pendingApproval, null);
  assert.equal(State.focusId, "integration_github");

  // Verifica teto MAX_PENDING = 5 na fila.
  calls.length = 0;
  for (const id of ["c1", "c2", "c3", "c4", "c5", "c6"]) {
    hook(mochiPayload(id));
  }
  assert.equal(State.pendingApproval?.requestId, "c1");
  assert.equal(State.approvalQueue.length, 4);
  assert.deepEqual(sent("approval_decline"), [{ requestId: "c6" }]);
});

test("mixed external and mochi queue preserves focus restoration and Reply in terminal stays external-only", () => {
  State.setFocus("integration_n8n");

  // Pedido externo muda foco para Antigravity, seguido de pedido Mochi na fila.
  hook({
    hook_event_name: "PermissionRequest",
    request_id: "agy-1",
    session_id: "s-agy",
    coucou_agent: "antigravity",
    coucou_kind: "agy_pre_tool",
    tool_name: "run_command",
    tool_input: { command: "npm test" },
  });
  assert.equal(State.focusId, "agent_antigravity");

  hook(mochiPayload("m-after-agy", {
    tool_name: "ping_med",
    mochi_summary: "Confirmar ação após Antigravity.",
  }));
  assert.equal(State.pendingApproval?.requestId, "agy-1");
  assert.equal(State.approvalQueue[0]?.requestId, "m-after-agy");

  // "Reply in terminal" no agy-1 envia approval_decline, restaura foco e promove m-after-agy.
  const agyBtns = approvalButtons();
  assert.equal(agyBtns.length, 3); // Deny, Allow, Reply in terminal
  advanceClock(700);
  agyBtns[2].fire("click");
  assert.deepEqual(sent("approval_decline"), [{ requestId: "agy-1" }]);
  assert.equal(State.focusId, "integration_n8n");
  assert.equal(State.pendingApproval?.requestId, "m-after-agy");

  // Em seguida, negar m-after-agy envia approval_decision("deny") sem alterar foco.
  calls.length = 0;
  const btns = approvalButtons();
  assert.equal(btns.length, 3); // Deny, Allow, Always allow
  advanceClock(700);
  btns[0].fire("click");
  assert.deepEqual(sent("approval_decision"), [{ requestId: "m-after-agy", decision: "deny" }]);
  assert.equal(State.pendingApproval, null);
  assert.equal(State.focusId, "integration_n8n");
});

