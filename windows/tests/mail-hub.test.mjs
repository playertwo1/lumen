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

test("Visão geral abre o MailCraft com o canal de controle e mostra sucesso ou erro perto do botão", async (t) => {
  t.after(() => {
    internals.invoke = originalInvoke;
  });
  let openError = null;
  const called = [];
  internals.invoke = async (cmd, args) => {
    called.push({ cmd, args });
    if (cmd === "mail_triage") {
      return {
        settings: { autoSort: false, autoSortSince: null, banned: [], vip: [], rules: [] },
        journal: [],
        sent: [],
        today: { sorted: 0, junk: 0, important: 0 },
        windows: true,
        outlookOpen: false,
      };
    }
    if (cmd === "mail_config_get") {
      return { backend: "mailcraft", mode: "control", host: "127.0.0.1", port: 7991, mcpCommand: "" };
    }
    if (cmd === "mail_open_mailcraft") {
      if (openError) throw new Error(openError);
      return "MailCraft aberto com o canal de controle na porta 7991.";
    }
    return undefined;
  };

  const page = root();
  await mailOverviewPage(page, makeCtx());
  const statusLine = () => page.find("div").find((el) => el.getAttribute("role") === "status" && el.textContent.includes("MailCraft"));

  page.find("button").find((b) => b.textContent === "Abrir MailCraft").fire("click");
  await flush();
  assert.ok(called.some((c) => c.cmd === "mail_open_mailcraft"));
  assert.match(statusLine().textContent, /canal de controle na porta 7991/);

  openError = "O MailCraft está aberto sem o canal de controle.";
  page.find("button").find((b) => b.textContent === "Abrir MailCraft").fire("click");
  await flush();
  assert.match(statusLine().textContent, /aberto sem o canal de controle/);
  assert.match(statusLine().className, /err/);
});

test("Triagem do MailCraft substitui o cartão do Outlook, agrupa e recomenda sem apagar", async (t) => {
  t.after(() => {
    internals.invoke = originalInvoke;
  });
  const called = [];
  const msg = (id, sender, subject) => ({
    entryId: id, storeId: "mailcraft", account: "", senderName: sender, address: `${id}@exemplo.com`,
    subject, date: 0, unread: true, flagged: false, local: "",
  });
  internals.invoke = async (cmd, args) => {
    called.push({ cmd, args });
    if (cmd === "mail_triage") {
      return {
        settings: { autoSort: false, autoSortSince: null, banned: [], vip: [], rules: [] },
        journal: [], sent: [], today: { sorted: 0, junk: 0, important: 0 },
        windows: true, outlookOpen: false, backend: "mailcraft", classified: [],
      };
    }
    if (cmd === "mail_config_get") {
      return { backend: "mailcraft", mode: "control", host: "127.0.0.1", port: 7991, mcpCommand: "" };
    }
    if (cmd === "mail_classify") {
      return {
        note: null,
        items: [
          { message: msg("1", "Energia", "Conta de luz de novembro"), category: "conta", recommend: "manter", reason: "Fatura com vencimento" },
          { message: msg("2", "Streaming", "Sua assinatura será renovada"), category: "assinatura", recommend: "ler", reason: "Renovação" },
          { message: msg("3", "Loja", "50% off"), category: "promocao", recommend: "apagar", reason: "Oferta" },
        ],
      };
    }
    return undefined;
  };

  const page = root();
  await mailOverviewPage(page, makeCtx());
  assert.doesNotMatch(page.textContent, /Outlook clássico \(triagem\)/);
  assert.match(page.textContent, /Triagem do MailCraft/);

  page.find("button").find((b) => b.textContent === "Fazer triagem").fire("click");
  await flush();
  assert.match(page.textContent, /Contas a pagar/);
  assert.match(page.textContent, /Renovações de assinatura/);
  assert.match(page.textContent, /3 mensagens classificadas/);
  // Com resultado, a explicação vira uma linha; "Saiba mais" mostra o texto inteiro.
  assert.match(page.textContent, /Só recomenda: nada sai da caixa sem o seu clique/);
  assert.doesNotMatch(page.textContent, /Seus VIPs, palavras proibidas/);
  page.find("button").find((b) => b.textContent === "Saiba mais").fire("click");
  assert.match(page.textContent, /Seus VIPs, palavras proibidas/);
  page.find("button").find((b) => b.textContent === "Menos").fire("click");
  assert.match(page.textContent, /2prioritários/);
  // Contas e renovações não oferecem "Importante": já estão no topo.
  assert.equal(page.find("button").filter((b) => b.textContent === "Importante").length, 1);

  // Os números filtram: "recomendo apagar" mostra só a promoção; "Mostrar tudo" volta.
  const tile = (caption) => page.find("button").find((b) => b.textContent.endsWith(caption));
  tile("recomendo apagar").fire("click");
  await flush();
  assert.equal(tile("recomendo apagar").getAttribute("aria-pressed"), "true");
  assert.match(page.textContent, /Só recomendo apagar: 1 de 3/);
  assert.doesNotMatch(page.textContent, /Contas a pagar/);
  page.find("button").find((b) => b.textContent === "Mostrar tudo").fire("click");
  await flush();
  assert.match(page.textContent, /Contas a pagar/);
  assert.equal(tile("recomendo apagar").getAttribute("aria-pressed"), "false");

  page.find("button").find((b) => b.textContent === "Importante").fire("click");
  await flush();
  assert.ok(called.some((c) => c.cmd === "mail_act" && c.args.kind === "flag" && c.args.message.entryId === "3"));
  assert.ok(!called.some((c) => /send|delete/.test(c.cmd)));

  // Apagar pede confirmação: o primeiro clique não faz nada, o segundo move para Excluídos.
  const apagar = () => page.find("button").filter((b) => /^Apagar/.test(b.textContent))[0];
  const deleteCalls = () => called.filter((c) => c.cmd === "mail_act" && c.args.kind === "delete");
  const before = page.find("button").filter((b) => /^Apagar/.test(b.textContent)).length;
  apagar().fire("click");
  await flush();
  assert.equal(deleteCalls().length, 0);
  assert.match(apagar().textContent, /Apagar\?/);
  apagar().fire("click");
  await flush();
  assert.equal(deleteCalls().length, 1);
  assert.equal(page.find("button").filter((b) => /^Apagar/.test(b.textContent)).length, before - 1);
  assert.ok(!called.some((c) => c.args?.kind === "deletePermanently"));

  // "Desfazer" devolve a mensagem para a Entrada e para a lista.
  assert.match(page.textContent, /· movida para Excluídos/);
  const deletedId = deleteCalls()[0].args.message.entryId;
  page.find("button").find((b) => b.textContent === "Desfazer").fire("click");
  await flush();
  assert.ok(called.some((c) => c.cmd === "mail_act" && c.args.kind === "undelete" && c.args.message.entryId === deletedId));
  assert.match(page.textContent, /De volta à Entrada/);
  assert.equal(page.find("button").filter((b) => /^Apagar/.test(b.textContent)).length, before);

  // "Abrir" mostra a mensagem no MailCraft, sem mexer na caixa.
  page.find("button").find((b) => b.textContent === "Abrir").fire("click");
  await flush();
  assert.ok(called.some((c) => c.cmd === "mail_open" && c.args.reply === false));

  // "Apagar os N": só com o filtro "recomendo apagar", pede confirmação e move só os recomendados.
  assert.ok(!page.find("button").some((b) => /^Apagar os/.test(b.textContent)));
  tile("recomendo apagar").fire("click");
  await flush();
  const bulk = () => page.find("button").find((b) => /^Apagar (os )?1/.test(b.textContent));
  const deletesBefore = deleteCalls().length;
  bulk().fire("click");
  await flush();
  assert.equal(deleteCalls().length, deletesBefore);
  assert.match(bulk().textContent, /Clique de novo/);
  bulk().fire("click");
  await flush();
  await new Promise((r) => setTimeout(r, 400));
  await flush();
  assert.equal(deleteCalls().length, deletesBefore + 1);
  assert.equal(deleteCalls().at(-1).args.message.entryId, "3");
  assert.match(page.textContent, /1 mensagem movida para Excluídos/);
  page.find("button").find((b) => b.textContent === "Desfazer").fire("click");
  await flush();
  assert.equal(called.filter((c) => c.cmd === "mail_act" && c.args.kind === "undelete" && c.args.message.entryId === "3").length, 1);
  assert.match(page.textContent, /1 mensagem de volta à Entrada/);
});

test("Triagem: sem o canal de controle, o erro oferece reiniciar o MailCraft com controle", async (t) => {
  t.after(() => {
    internals.invoke = originalInvoke;
  });
  const called = [];
  const readOnly = "MailCraft conectado só para leitura local (sem `--control 7991`): para salvar rascunhos ou alterar mensagens, inicie o MailCraft com `--control 7991`.";
  internals.invoke = async (cmd, args) => {
    called.push({ cmd, args });
    if (cmd === "mail_triage") {
      return {
        settings: { autoSort: false, autoSortSince: null, banned: [], vip: [], rules: [] },
        journal: [], sent: [], today: { sorted: 0, junk: 0, important: 0 },
        windows: true, outlookOpen: false, backend: "mailcraft", classified: [],
      };
    }
    if (cmd === "mail_config_get") return { backend: "mailcraft", mode: "control", host: "127.0.0.1", port: 7991, mcpCommand: "" };
    if (cmd === "mail_classify") {
      return {
        note: null,
        items: [{ message: { entryId: "9", storeId: "mailcraft", account: "", senderName: "Loja", address: "9@x.com", subject: "Oferta", date: 0, unread: true, flagged: false, local: "" }, category: "promocao", recommend: "apagar", reason: "Oferta" }],
      };
    }
    if (cmd === "mail_act") throw new Error(readOnly);
    if (cmd === "mail_restart_mailcraft") return "MailCraft aberto com o canal de controle na porta 7991.";
    return undefined;
  };

  const page = root();
  await mailOverviewPage(page, makeCtx());
  page.find("button").find((b) => b.textContent === "Fazer triagem").fire("click");
  await flush();
  page.find("button").find((b) => b.textContent === "Importante").fire("click");
  await flush();
  assert.match(page.textContent, /só para leitura local/);
  const fix = page.find("button").find((b) => b.textContent === "Reiniciar MailCraft com controle");
  assert.ok(fix, "o botão de reiniciar aparece");
  assert.ok(!called.some((c) => c.cmd === "mail_restart_mailcraft"), "só reinicia depois do clique");
  fix.fire("click");
  await flush();
  assert.ok(called.some((c) => c.cmd === "mail_restart_mailcraft"));
  assert.match(page.textContent, /canal de controle na porta 7991/);
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
