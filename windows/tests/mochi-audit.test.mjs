// Auditoria local redigida e limpeza em Settings → General (M1.4):
// MochiBridge.auditList / auditClear, mochiAuditRows() e i18n em pt-BR/en.

import { afterEach, beforeEach, mock, test } from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals, sent } from "./tauri.mjs";

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.endsWith(".css")) {
      return { url: new URL(specifier, context.parentURL).href, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url.endsWith(".css")) return { format: "module", source: "", shortCircuit: true };
    if (url.endsWith("/hub/hero.ts")) {
      const { outputText } = ts.transpileModule(readFileSync(fileURLToPath(url), "utf8"), {
        compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
      });
      return { format: "module", source: outputText, shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});

Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "Windows NT", languages: ["en-US"] },
});

installFakeDom();
document.getElementById = () => null;
document.visibilityState = "hidden";
document.documentElement = { dir: "ltr", lang: "en", dataset: {} };
document.addEventListener = () => {};
window.addEventListener = () => {};

const { MochiBridge } = await import("../src/core/bridge.ts");
const { setLanguage } = await import("../src/i18n/i18n.ts");
const { mochiAuditRows, generalSection } = await import("../src/settings/main.ts");

const originalInvoke = internals.invoke;
const settle = () => new Promise((resolve) => setImmediate(resolve));

let auditEntries = [];
let failAuditList = false;

beforeEach(() => {
  setLanguage("en");
  mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  auditEntries = [
    {
      timestampMs: 1_794_668_400_000,
      isoTime: "2026-11-16T09:00:00Z",
      toolId: "ping",
      risk: "low",
      decision: "autoLow",
      result: "success",
      summary: "ping local",
    },
    {
      timestampMs: 1_794_668_300_000,
      isoTime: "2026-11-16T08:58:20Z",
      toolId: "med_probe",
      risk: "medium",
      decision: "denied",
      result: "blocked",
      summary: "Permissão negada para executar a ferramenta.",
    },
  ];
  failAuditList = false;

  internals.invoke = async (cmd, args) => {
    if (cmd === "mochi_audit_list") {
      calls.push([cmd, args]);
      if (failAuditList) throw new Error("audit read failed");
      return typeof args?.limit === "number" ? auditEntries.slice(0, args.limit) : auditEntries;
    }
    if (cmd === "mochi_audit_clear") {
      calls.push([cmd, args]);
      auditEntries = [];
      return null;
    }
    return originalInvoke(cmd, args);
  };
});

afterEach(() => {
  internals.invoke = originalInvoke;
  mock.timers.runAll();
  mock.timers.reset();
  setLanguage("en");
});

test("MochiBridge.auditList and auditClear forward expected Tauri commands", async () => {
  const all = await MochiBridge.auditList();
  assert.equal(all?.length, 2);
  const limited = await MochiBridge.auditList(1);
  assert.equal(limited?.length, 1);

  assert.deepEqual(sent("mochi_audit_list"), [{ limit: null }, { limit: 1 }]);

  await MochiBridge.auditClear();
  assert.equal(sent("mochi_audit_clear").length, 1);
  assert.equal((await MochiBridge.auditList())?.length, 0);
});

test("mochiAuditRows renders local count, clear button calls mochi_audit_clear and shows confirmation", async () => {
  const container = document.createElement("div");
  container.append(...mochiAuditRows());
  await settle();

  const textBefore = container.textContent;
  assert.match(textBefore, /Tool log/);
  assert.match(textBefore, /2 local entries/);
  assert.match(textBefore, /never raw arguments, secrets or personal paths/);

  const clearBtn = container.find("button").find((b) => b.classList.contains("danger"));
  assert.ok(clearBtn, "deve existir botão danger para limpar o registro");
  assert.equal(clearBtn.textContent, "Clear tool log");

  await clearBtn.fire("click");
  await settle();

  assert.equal(sent("mochi_audit_clear").length, 1);
  assert.equal(clearBtn.disabled, true);
  assert.match(container.textContent, /0 local entries/);
  assert.match(container.textContent, /Tool log cleared\./);

  mock.timers.tick(2400);
  assert.equal(clearBtn.disabled, false);
  assert.doesNotMatch(container.textContent, /Tool log cleared\./);
});

test("generalSection places mochiAuditRows right below recapRows and supports pt-BR + resilient fallback", async () => {
  failAuditList = true;
  setLanguage("pt-BR");

  const section = generalSection();
  await settle();

  const buttons = section.find("button");
  const clearHistoryIdx = buttons.findIndex((b) => b.textContent === "Limpar histórico");
  const clearToolLogIdx = buttons.findIndex((b) => b.textContent === "Limpar registro");
  assert.ok(clearHistoryIdx >= 0, "recapRows deve estar em generalSection");
  assert.ok(clearToolLogIdx > clearHistoryIdx, "mochiAuditRows deve ficar logo abaixo de recapRows");
  assert.match(section.textContent, /Registro de ferramentas/);
  assert.match(section.textContent, /0 entrada local|0 entradas locais/);
  assert.match(section.textContent, /nunca argumentos brutos, segredos nem caminhos pessoais/);

  const clearBtn = buttons[clearToolLogIdx];
  await clearBtn.fire("click");
  await settle();

  assert.equal(sent("mochi_audit_clear").length, 1);
  assert.match(section.textContent, /Registro limpo\./);
});
