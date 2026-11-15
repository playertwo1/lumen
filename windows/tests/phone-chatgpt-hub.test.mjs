// Etapa 4 (Celular Android + Login ChatGPT): garante que a reorganização visual
// preserva contratos de status, alternância de switch, relay, serviços, permissões
// e gestão de contas ChatGPT usando FakeDOM e ponte sintética.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals, sent } from "./tauri.mjs";

installFakeDom();
const fakeProto = Object.getPrototypeOf(document.createElement("div"));
if (!fakeProto.replaceWith) {
  fakeProto.replaceWith = function (...nodes) {
    const parent = this.parentNode;
    if (!parent) return;
    const idx = parent.childNodes.indexOf(this);
    if (idx < 0) return;
    for (const n of nodes) n.parentNode = parent;
    parent.childNodes.splice(idx, 1, ...nodes);
    this.parentNode = null;
  };
}
if (!globalThis.window) {
  globalThis.window = {
    setInterval: () => 0,
    clearInterval: () => {},
    setTimeout,
    clearTimeout,
  };
}

const { phoneSection } = await import("../src/settings/phone.ts");
const { chatGptAccountSection } = await import("../src/settings/chatgpt.ts");
const { DEFAULT_SETTINGS } = await import("../src/core/state.ts");
const { h } = await import("../src/views/dom.ts");

let answers;
const plainInvoke = internals.invoke;
internals.invoke = async (cmd, args) => {
  const result = await plainInvoke(cmd, args);
  if (cmd in answers) return typeof answers[cmd] === "function" ? answers[cmd](args) : answers[cmd];
  return result;
};

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function makeToggle(on, onChange) {
  let state = on;
  const btn = h("button", { class: on ? "switch on" : "switch", "aria-pressed": String(on) });
  btn.addEventListener("click", () => {
    state = !state;
    btn.classList.toggle("on", state);
    btn.setAttribute("aria-pressed", String(state));
    onChange(state);
  });
  return btn;
}

beforeEach(() => {
  calls.length = 0;
  answers = {};
});

test("phoneSection renders live status pill, balanced workbench and full-width services block when running", async () => {
  answers.phone_link_status = {
    enabled: true,
    running: true,
    port: 8443,
    host: "192.168.1.42",
    name: "Lumen-Desktop",
    clients: 1,
    error: null,
    relayState: "linked",
  };
  answers.phone_relay_status = { shown: true, enabled: true, url: "wss://relay.exemplo.workers.dev", hasAccess: true, state: "linked" };
  answers.phone_details_status = { enabled: true };
  answers.phone_answers_status = { enabled: true };
  answers.phone_diffs_status = { enabled: false };
  answers.phone_usage_status = { enabled: true };
  answers.phone_recap_status = { enabled: false };
  answers.phone_services_status = { services: ["integration_github"] };
  answers.phone_chat_status = { enabled: true, models: [] };

  const root = phoneSection(false, makeToggle, { ...DEFAULT_SETTINGS, chatProvider: "chatgpt" });
  await flush();
  await flush();

  const livePill = root.querySelector(".phone-live-pill");
  assert.ok(livePill);
  assert.equal(livePill.textContent, "Conectado · 192.168.1.42:8443");
  assert.ok(livePill.classList.contains("ok"));

  const masterSwitch = root.querySelector(".phone-master-switch").querySelector(".switch");
  assert.ok(masterSwitch.classList.contains("on"));

  assert.ok(root.querySelector(".phone-workbench"));
  assert.ok(root.querySelector(".phone-extras-grid"));
  assert.ok(root.querySelector(".phone-relay-state"));
  const checkedChips = root.find(".phone-service-chip").filter((chip) => chip.classList.contains("checked"));
  assert.equal(checkedChips.length, 1);
  assert.equal(checkedChips[0].textContent, "GitHub");
});

test("phoneSection toggles service chip and updates Bridge.phoneServicesSet", async () => {
  answers.phone_link_status = {
    enabled: true, running: true, port: 8443, host: "192.168.1.42", name: "Desktop", clients: 0, error: null, relayState: "off",
  };
  answers.phone_relay_status = { shown: false, enabled: false, url: "", hasAccess: false, state: "off" };
  answers.phone_details_status = { enabled: false };
  answers.phone_answers_status = { enabled: false };
  answers.phone_diffs_status = { enabled: false };
  answers.phone_usage_status = { enabled: false };
  answers.phone_recap_status = { enabled: false };
  answers.phone_services_status = { services: [] };
  answers.phone_chat_status = { enabled: false, models: [] };
  answers.phone_services_set = ({ services }) => ({ services });

  const root = phoneSection(true, makeToggle, { ...DEFAULT_SETTINGS });
  await flush();
  await flush();

  const waitingPill = root.querySelector(".phone-live-pill");
  assert.equal(waitingPill.textContent, "Aguardando · 192.168.1.42:8443");
  assert.equal(root.querySelector(".phone-relay-state"), null);

  const firstChip = root.querySelector(".phone-service-chip");
  const checkbox = firstChip.find("input")[0];
  checkbox.checked = true;
  checkbox.fire("change");
  await flush();

  assert.equal(sent("phone_services_set").length, 1);
  assert.deepEqual(sent("phone_services_set")[0].services, ["integration_stripe"]);
  assert.ok(firstChip.classList.contains("checked"));
});

test("chatGptAccountSection handles disconnected, active switching, activation and sign-out", async () => {
  const settings = { ...DEFAULT_SETTINGS, chatProvider: "anthropic" };
  let savedCount = 0;
  let updatedCount = 0;

  const initialStatus = {
    activeProfileId: "p-1",
    accounts: [
      { profileId: "p-1", label: "Conta Pessoal (Plus)", connected: true, planEnabled: true, active: true },
      { profileId: "p-2", label: "Conta Secundária", connected: true, planEnabled: true, active: false },
    ],
  };

  answers.chatgpt_status = initialStatus;
  answers.chatgpt_activate = ({ profileId }) => ({
    activeProfileId: profileId,
    accounts: initialStatus.accounts.map((a) => ({ ...a, active: a.profileId === profileId })),
  });
  answers.chatgpt_sign_out = { localCleared: true, remoteRevoked: true };

  const el = chatGptAccountSection(initialStatus, null, {
    settings,
    save: async () => { savedCount += 1; },
    onUpdated: () => { updatedCount += 1; },
  });

  const rows = el.find(".chatgpt-account-row");
  assert.equal(rows.length, 2);

  const useInChat = rows[0].find("button").find((b) => b.textContent === "Usar no chat");
  assert.ok(useInChat);
  assert.ok(useInChat.classList.contains("primary"));
  useInChat.fire("click");
  await flush();

  assert.equal(settings.chatProvider, "chatgpt");
  assert.equal(savedCount, 1);
  assert.equal(updatedCount, 1);
  const inUseBtn = el.find("button").find((b) => b.textContent === "Em uso");
  assert.ok(inUseBtn);
  assert.ok(inUseBtn.disabled);

  const activateSecondary = el.find("button").find((b) => b.textContent === "Usar esta conta");
  assert.ok(activateSecondary);
  activateSecondary.fire("click");
  await flush();
  assert.equal(sent("chatgpt_activate").length, 1);
  assert.equal(sent("chatgpt_activate")[0].profileId, "p-2");

  const signOutBtn = el.find("button").find((b) => b.textContent === "Sair");
  signOutBtn.fire("click");
  await flush();
  assert.equal(sent("chatgpt_sign_out").length, 1);
  assert.equal(el.querySelector(".notice.ok").textContent, "Sessão encerrada e revogada.");
});
