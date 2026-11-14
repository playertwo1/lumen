// Testes sintéticos da L5: MailCraft principal, Outlook secundário, resumo e preparo de rascunho sem envio.

import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals } from "./tauri.mjs";
import { pillDefinition } from "../src/core/pills.ts";
import { MAIL_BLUE, MAIL_ID } from "../src/core/mail.ts";
import { State } from "../src/core/state.ts";
import { buildMailCard } from "../src/views/mail.ts";

installFakeDom();

const defaultInvoke = internals.invoke;
let customInvoke = null;

function findButtons(root, matchText) {
  const found = [];
  const walk = (node) => {
    if (!node) return;
    if (node.tagName === "BUTTON" && (node.textContent ?? "").includes(matchText)) {
      found.push(node);
    }
    for (const child of node.children ?? []) walk(child);
  };
  walk(root);
  return found;
}

function collectInputs(root) {
  const inputs = [];
  const walk = (node) => {
    if (!node) return;
    if (node.tagName === "INPUT" || node.tagName === "TEXTAREA") inputs.push(node);
    for (const child of node.children ?? []) walk(child);
  };
  walk(root);
  return inputs;
}

beforeEach(() => {
  calls.length = 0;
  customInvoke = null;
  internals.invoke = async (cmd, args) => {
    calls.push({ cmd, args });
    if (customInvoke) return customInvoke(cmd, args);
    return undefined;
  };
});

afterEach(() => {
  internals.invoke = defaultInvoke;
  State.integrations[MAIL_ID] = undefined;
});

test("pill integration_mail preserva contrato e suporte no Windows com MailCraft", () => {
  const pill = pillDefinition(MAIL_ID);
  assert.ok(pill);
  assert.equal(pill.id, "integration_mail");
  assert.equal(pill.color, MAIL_BLUE);
  assert.equal(pill.support, "windows");
});

test("card da ilha opera em MailCraft principal, resume inbox e salva rascunho sem enviar", async () => {
  State.integrations[MAIL_ID] = {
    data: {
      running: true,
      windows: true,
      backend: "mailcraft",
      mode: "control",
      unread: 1,
      shown: 12,
      maxShown: 60,
      messages: [
        {
          entryId: "42",
          storeId: "mailcraft",
          account: "eu@exemplo.com",
          senderName: "Ana",
          address: "ana@exemplo.com",
          subject: "Alinhamento do projeto",
          date: Date.now() - 60_000,
          unread: true,
          flagged: false,
          local: "2026-11-14 11:00",
        },
      ],
    },
    error: null,
  };

  customInvoke = async (cmd) => {
    if (cmd === "mail_summarize_inbox") {
      return {
        summary: "- Prioridade: responder Ana sobre alinhamento.",
        count: 1,
        unread: 1,
        backend: "mailcraft",
        warnings: [],
      };
    }
    if (cmd === "mail_prepare_draft") {
      return {
        saved: true,
        provider: "mailcraft",
        composeId: 9,
        openedWindow: true,
        note: "Rascunho salvo no MailCraft para revisão. Nada foi enviado.",
      };
    }
    return undefined;
  };

  let keyboardOn = false;
  const card = buildMailCard({ keyboard: (on) => { keyboardOn = on; } });
  card.sync();
  assert.ok((card.el.textContent ?? "").includes("MailCraft"));

  // Resumir caixa
  const summarizeBtn = findButtons(card.el, "Resumir")[0];
  assert.ok(summarizeBtn);
  summarizeBtn.fire("click");
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(calls.some((c) => c.cmd === "mail_summarize_inbox"));
  assert.ok((card.el.textContent ?? "").includes("responder Ana sobre alinhamento"));

  // Voltar à lista e preparar rascunho sem envio
  findButtons(card.el, "Voltar à lista")[0].fire("click");
  const draftBtn = findButtons(card.el, "Rascunho")[0];
  assert.ok(draftBtn);
  draftBtn.fire("click");
  assert.equal(keyboardOn, true);

  const [toInput, subjInput, bodyInput] = collectInputs(card.el);
  toInput.value = "ana@exemplo.com";
  subjInput.value = "Re: Alinhamento do projeto";
  bodyInput.value = "Combinado para segunda às 10h.";

  const saveDraftBtn = findButtons(card.el, "Salvar rascunho (sem enviar)")[0];
  assert.ok(saveDraftBtn);
  saveDraftBtn.fire("click");
  await new Promise((r) => setTimeout(r, 10));

  const draftCall = calls.find((c) => c.cmd === "mail_prepare_draft");
  assert.ok(draftCall);
  assert.equal(draftCall.args.to, "ana@exemplo.com");
  assert.equal(draftCall.args.subject, "Re: Alinhamento do projeto");
  assert.ok(!calls.some((c) => c.cmd.includes("send") || c.cmd === "mail_plan_start"));
});

test("card exibe rótulo secundário quando backend está em outlook", () => {
  State.integrations[MAIL_ID] = {
    data: {
      running: false,
      windows: true,
      backend: "outlook",
      unread: 0,
      shown: 12,
      maxShown: 60,
      messages: [],
    },
    error: null,
  };
  const card = buildMailCard({ keyboard: () => {} });
  card.sync();
  assert.ok((card.el.textContent ?? "").includes("Outlook (secundário)"));
});
