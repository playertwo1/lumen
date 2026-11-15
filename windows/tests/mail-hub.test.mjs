// Testes sintéticos das páginas de MailCraft e Triagem no Hub (Etapa 3).

import { test } from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import ts from "typescript";
import { installFakeDom } from "./fakedom.mjs";
import { internals } from "./tauri.mjs";

const { root } = installFakeDom();
registerHooks({
  load(url, context, nextLoad) {
    const loaded = nextLoad(url, context);
    if (!url.endsWith("/src/hub/hero.ts")) return loaded;
    return {
      ...loaded,
      format: "module",
      source: ts.transpileModule(String(loaded.source), {
        compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
      }).outputText,
    };
  },
});

document.addEventListener = () => {};
window.addEventListener ??= () => {};
document.documentElement = { lang: "pt-BR" };
document.visibilityState = "hidden";
globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

const { mailOverviewPage, mailRulesPage, mailBannedPage, mailVIPPage } = await import("../src/hub/pages/mail.ts");
const flush = () => new Promise((resolve) => setTimeout(resolve, 10));
const originalInvoke = internals.invoke;

function makeCtx(navigated = []) {
  return {
    hero: () => root(),
    status: { models: { parakeet: { state: "ready" } } },
    navigate: (id) => navigated.push(id),
  };
}

test("mailOverviewPage preserva âncora mail.autosort, testa conexão, resume caixa e salva rascunho sem enviar", async (t) => {
  t.after(() => {
    internals.invoke = originalInvoke;
  });
  const called = [];
  internals.invoke = async (cmd, args) => {
    called.push({ cmd, args });
    if (cmd === "mail_triage") {
      return {
        settings: { autoSort: false, autoSortSince: null, banned: [], vip: [], rules: [] },
        journal: [],
        sent: [],
        today: { sorted: 7, junk: 2, important: 1 },
        windows: true,
        outlookOpen: true,
      };
    }
    if (cmd === "mail_config_get") {
      return { backend: "mailcraft", mode: "control", host: "127.0.0.1", port: 7991, mcpCommand: "" };
    }
    if (cmd === "mail_config_set") return args.config;
    if (cmd === "mail_test_connection") {
      return { connected: true, mode: "control", endpoint: "127.0.0.1:7991", accounts: 1, mailboxState: "sincronizada" };
    }
    if (cmd === "mail_summarize_inbox") {
      return { count: 4, unread: 1, backend: "mailcraft", summary: "• Retornar contrato.", warnings: [] };
    }
    if (cmd === "mail_prepare_draft") {
      return { saved: true, provider: "mailcraft", composeId: 3, openedWindow: true, note: "Rascunho salvo sem envio." };
    }
    return undefined;
  };

  const page = root();
  await mailOverviewPage(page, makeCtx());

  const autosortAnchor = page.find("div").find((el) => el.dataset?.anchor === "mail.autosort");
  assert.ok(autosortAnchor, "âncora mail.autosort deve existir");
  assert.match(page.textContent, /Nunca envia sozinho/);

  page.find("button").find((b) => b.textContent === "Testar conexão").fire("click");
  await flush();
  assert.match(page.textContent, /Conectado/);
  assert.match(page.textContent, /127\.0\.0\.1:7991/);

  page.find("button").find((b) => b.textContent === "Resumir caixa agora").fire("click");
  await flush();
  assert.match(page.textContent, /4 mensagens · 1 não lidas · mailcraft/);
  assert.match(page.textContent, /Retornar contrato\./);

  const inputs = page.find("input");
  const toInput = inputs.find((i) => i.getAttribute("aria-label") === "Destinatário do rascunho");
  const subjInput = inputs.find((i) => i.getAttribute("aria-label") === "Assunto do rascunho");
  const bodyInput = page.find("textarea")[0];
  toInput.value = "contato@exemplo.com";
  subjInput.value = "Alinhamento";
  bodyInput.value = "Texto sintético.";

  page.find("button").find((b) => b.textContent === "Salvar rascunho (sem enviar)").fire("click");
  await flush();
  assert.match(page.textContent, /Rascunho salvo sem envio\./);
  assert.ok(!called.some((c) => c.cmd.includes("send") || c.cmd === "mail_plan_start"));
});

test("mailRulesPage, mailBannedPage e mailVIPPage preservam âncoras e manipulam regras e chips com segurança", async (t) => {
  t.after(() => {
    internals.invoke = originalInvoke;
  });
  let savedSettings = null;
  const view = {
    settings: {
      autoSort: false,
      autoSortSince: null,
      banned: ["spam"],
      vip: ["ana@exemplo.com"],
      rules: [{ id: "r1", name: "Regra inicial", enabled: true, conditions: [{ field: "sender", value: "news" }], action: { kind: "junk" } }],
    },
    journal: [],
    sent: [],
    today: { sorted: 0, junk: 0, important: 0 },
    windows: true,
    outlookOpen: true,
  };
  internals.invoke = async (cmd, args) => {
    if (cmd === "mail_triage") return structuredClone(view);
    if (cmd === "mail_triage_set") {
      savedSettings = args.settings;
      return null;
    }
    return undefined;
  };

  const rulesPage = root();
  await mailRulesPage(rulesPage, makeCtx());
  assert.ok(rulesPage.find("div").find((el) => el.dataset?.anchor === "mail.rules"));
  assert.match(rulesPage.textContent, /Regras configuradas \(1\)/);

  const ruleInput = rulesPage.find("input")[0];
  ruleInput.value = "fatura";
  rulesPage.find("button").find((b) => b.textContent === "Add" || b.textContent === "Adicionar").fire("click");
  assert.equal(savedSettings.rules.length, 2);
  assert.match(rulesPage.textContent, /Regras configuradas \(2\)/);

  const bannedPage = root();
  await mailBannedPage(bannedPage, makeCtx());
  assert.ok(bannedPage.find("div").find((el) => el.dataset?.anchor === "mail.banned"));
  assert.match(bannedPage.textContent, /1 item/);
  const removeBanned = bannedPage.find(".hub-chip")[0].find("button")[0];
  assert.equal(removeBanned.getAttribute("type"), "button");
  removeBanned.fire("click");
  assert.deepEqual(savedSettings.banned, []);
  assert.match(bannedPage.textContent, /0 itens/);

  const vipPage = root();
  await mailVIPPage(vipPage, makeCtx());
  assert.ok(vipPage.find("div").find((el) => el.dataset?.anchor === "mail.vip"));
  const vipInput = vipPage.find("input")[0];
  vipInput.value = "@cliente.exemplo";
  vipPage.find("button").find((b) => b.textContent === "Add" || b.textContent === "Adicionar").fire("click");
  assert.deepEqual(savedSettings.vip, ["ana@exemplo.com", "@cliente.exemplo"]);
});
