// Testes da M1.6 para src/views/chat.ts: estados do Mochi na ilha,
// pausa rápida sincronizada, botão cancelar, ocultação de <mochi_tool_call>
// durante streaming e erro visível sem perda de contexto.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, emit, internals, sent } from "./tauri.mjs";

installFakeDom();
const { buildPrompt, stripMochiToolMarkup } = await import("../src/views/chat.ts");
const { DEFAULT_SETTINGS, State } = await import("../src/core/state.ts");
const { setLanguage } = await import("../src/i18n/i18n.ts");

let answers;
const plainInvoke = internals.invoke;
internals.invoke = async (cmd, args) => {
  const result = await plainInvoke(cmd, args);
  if (cmd in answers) return typeof answers[cmd] === "function" ? answers[cmd](args) : answers[cmd];
  return result;
};

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

let view;
beforeEach(() => {
  calls.length = 0;
  answers = {};
  setLanguage("pt-BR");
  State.settings = { ...DEFAULT_SETTINGS, chatModels: {} };
  State.chatHistory = [];
  State.stateOverride = null;
  State.mochiRunState = null;
  State.paused = false;
  State.noteMessage = null;
  State.view = "prompt";
  State.droppedFile = null;
  State.windowContext = null;
  view = buildPrompt(() => {});
  view.sync();
});

const $ = (cls) => view.el.querySelector(cls);

test("mochi-run-state atualiza todas as 5 transições no chat e o stateOverride da ilha", () => {
  assert.equal($(".mochi-run-status").style.display, "none");

  emit("mochi-run-state", { state: "thinking", tool: null, detail: null });
  assert.equal(State.stateOverride, "thinking");
  assert.equal($(".mochi-run-status").dataset.phase, "thinking");
  assert.equal($(".mochi-run-status").textContent, "Pensando…");

  emit("mochi-run-state", { state: "awaiting_click", tool: "demo_medium", detail: null });
  assert.equal(State.stateOverride, "approval");
  assert.equal($(".mochi-run-status").dataset.phase, "awaiting_click");
  assert.match($(".mochi-run-status").textContent, /Aguardando clique… \(demo_medium\)/);

  emit("mochi-run-state", { state: "executing", tool: "ping", detail: null });
  assert.equal(State.stateOverride, "working");
  assert.equal($(".mochi-run-status").dataset.phase, "executing");
  assert.match($(".mochi-run-status").textContent, /Executando… \(ping\)/);

  emit("mochi-run-state", { state: "done", tool: "ping", detail: null });
  assert.equal(State.stateOverride, null);
  assert.equal($(".mochi-run-status").dataset.phase, "done");
  assert.match($(".mochi-run-status").textContent, /Feito \(ping\)/);

  State.chatHistory = [{ id: 1, role: "user", content: "faça um ping" }];
  emit("mochi-run-state", { state: "failed", tool: "ping", detail: "O assistente está em pausa." });
  assert.equal(State.stateOverride, null);
  assert.equal($(".mochi-run-status").dataset.phase, "failed");
  assert.match($(".mochi-run-status").textContent, /Falhou \(ping\)/);
  assert.equal($(".mochi-run-error").style.display, "block");
  assert.equal($(".mochi-run-error").textContent, "O assistente está em pausa.");
  assert.equal(State.chatHistory.length, 1);
});

test("pausa rápida no chat envia set_paused e sincroniza com evento mochi-paused", async () => {
  const pauseBtn = $(".mochi-pause-btn");
  assert.equal(pauseBtn.textContent, "Pausar");
  assert.ok(!pauseBtn.classList.contains("on"));

  pauseBtn.fire("click");
  await flush();
  assert.equal(State.paused, true);
  assert.equal(pauseBtn.textContent, "Em pausa");
  assert.ok(pauseBtn.classList.contains("on"));
  assert.deepEqual(sent("set_paused"), [{ paused: true }]);

  emit("mochi-paused", false);
  assert.equal(State.paused, false);
  assert.equal(pauseBtn.textContent, "Pausar");
  assert.ok(!pauseBtn.classList.contains("on"));

  emit("mochi-paused", true);
  assert.equal(State.paused, true);
  assert.equal(pauseBtn.textContent, "Em pausa");
});

test("botão Cancelar aparece apenas durante execução e chama mochi_cancel_execution", async () => {
  const cancelBtn = $(".mochi-cancel-btn");
  assert.equal(cancelBtn.style.display, "none");

  let rejectSend;
  answers.chat_send = () => new Promise((_, reject) => (rejectSend = reject));
  $(".chat-input").value = "faça um ping";
  $(".send-btn").fire("click");
  await flush();
  view.sync();

  assert.equal(cancelBtn.style.display, "inline-flex");
  assert.equal(cancelBtn.disabled, false);

  cancelBtn.fire("click");
  await flush();
  assert.equal(sent("mochi_cancel_execution").length, 1);
  assert.equal(cancelBtn.disabled, true);

  rejectSend(new Error("Execução cancelada pelo usuário."));
  await flush();
  await flush();
  view.sync();

  assert.equal(cancelBtn.style.display, "none");
  assert.equal(State.mochiRunState?.state, "failed");
  assert.equal(State.noteMessage, "Execução cancelada pelo usuário.");
  assert.equal($(".mochi-run-error").textContent, "Execução cancelada pelo usuário.");
});

test("oculta tags <mochi_tool_call> parciais e completas durante streaming e resposta final", async () => {
  assert.equal(
    stripMochiToolMarkup("Olá <mochi_tool_call>{\"tool\":\"ping\"}"),
    "Olá",
  );
  assert.equal(
    stripMochiToolMarkup("Início <mochi_tool_call>{\"tool\":\"pi"),
    "Início",
  );

  let resolveSend;
  answers.chat_send = () => new Promise((resolve) => (resolveSend = resolve));
  $(".chat-input").value = "faça um ping";
  $(".send-btn").fire("click");
  await flush();
  view.sync();

  // Delta apenas com bloco parcial de tool call: mantém dots e não cria bolha vazia.
  emit("chat-delta", "<mochi_tool_call>{\"tool\":\"pi");
  assert.ok($(".typing"));
  assert.equal(view.el.find(".reply").length, 0);

  // Delta já com texto limpo após tool result: exibe apenas a resposta natural.
  emit("chat-delta", "Ping concluído: **lumen-mochi-m1**");
  assert.equal($(".typing"), null);
  assert.equal(view.el.find(".reply").length, 1);
  assert.equal(view.el.find(".reply")[0].textContent, "Ping concluído: lumen-mochi-m1");

  resolveSend({
    text: "Ping concluído: **lumen-mochi-m1** <mochi_tool_call>{\"tool\":\"ping\"}</mochi_tool_call>",
  });
  await flush();
  await flush();
  view.sync();

  const finalReplies = view.el.find(".reply");
  assert.equal(finalReplies.length, 1);
  assert.equal(finalReplies[0].textContent, "Ping concluído: lumen-mochi-m1");
  assert.ok(!finalReplies[0].textContent.includes("mochi_tool_call"));
});
