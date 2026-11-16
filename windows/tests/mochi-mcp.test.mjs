// Testes de UI e bridge da subfase M1.5 (Configuração local de servidores MCP nas CLIs).

import { afterEach, beforeEach, mock, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals, sent } from "./tauri.mjs";

Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "Windows NT", languages: ["en-US"] },
});

installFakeDom();
document.documentElement = { dir: "ltr", lang: "en", dataset: {} };

const { MochiBridge } = await import("../src/core/bridge.ts");
const { setLanguage } = await import("../src/i18n/i18n.ts");
const { mcpSection } = await import("../src/settings/mcp.ts");
const { statusDot } = await import("../src/settings/parts.ts");

const originalInvoke = internals.invoke;
const settle = () => new Promise((resolve) => setImmediate(resolve));

let mcpList = [];
let previewError = null;
let applyError = null;

function defaultList() {
  return [
    {
      id: "claude",
      name: "Claude Code",
      path: "C:\\Users\\tester\\.claude.json",
      cliFound: true,
      registered: false,
      commandMatches: false,
    },
    {
      id: "codex",
      name: "Codex",
      path: "C:\\Users\\tester\\.codex\\config.toml",
      cliFound: true,
      registered: true,
      commandMatches: false,
    },
    {
      id: "gemini",
      name: "Gemini CLI",
      path: "C:\\Users\\tester\\.gemini\\settings.json",
      cliFound: false,
      registered: false,
      commandMatches: false,
    },
    {
      id: "antigravity",
      name: "Antigravity",
      path: "C:\\Users\\tester\\.gemini\\antigravity\\mcp_config.json",
      cliFound: true,
      registered: true,
      commandMatches: true,
    },
  ];
}

beforeEach(() => {
  setLanguage("en");
  mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  mcpList = defaultList();
  previewError = null;
  applyError = null;

  internals.invoke = async (cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === "mochi_mcp_status") {
      return mcpList;
    }
    if (cmd === "mochi_mcp_preview") {
      if (previewError) throw new Error(previewError);
      const target = mcpList.find((item) => item.id === args.agent);
      return {
        diff: args.install
          ? `+++ ${target?.path ?? args.agent}\n+[mcp_servers.lumen]\n+command = "C:\\\\hook.exe"`
          : `--- ${target?.path ?? args.agent}\n-[mcp_servers.lumen]`,
        backup: target?.registered ? `${target.path}.bak-20261116-100000` : "",
        path: target?.path ?? "",
        fingerprint: `fp-${args.agent}-${args.install ? "in" : "rm"}`,
      };
    }
    if (cmd === "mochi_mcp_apply") {
      if (applyError) throw new Error(applyError);
      const target = mcpList.find((item) => item.id === args.agent);
      if (target) {
        target.registered = args.install;
        target.commandMatches = args.install;
      }
      return target ? `${target.path}.bak-20261116-100000` : "";
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

test("MochiBridge MCP commands forward exact Tauri arguments", async () => {
  const status = await MochiBridge.mcpStatus();
  assert.equal(status?.length, 4);

  const plan = await MochiBridge.mcpPreview("codex", true);
  assert.equal(plan.fingerprint, "fp-codex-in");

  const backups = await MochiBridge.mcpApply("codex", true, plan.fingerprint);
  assert.match(backups, /\.codex\\config\.toml\.bak-/);

  assert.deepEqual(sent("mochi_mcp_status"), [{}]);
  assert.deepEqual(sent("mochi_mcp_preview"), [{ agent: "codex", install: true }]);
  assert.deepEqual(sent("mochi_mcp_apply"), [
    { agent: "codex", install: true, fingerprint: "fp-codex-in" },
  ]);
});

test("mcpSection renders status dots, labels and buttons for all states", () => {
  const section = mcpSection(mcpList, true);
  assert.match(section.textContent, /Local MCP tools \(Lumen\)/);

  const buttons = section.find("button").map((b) => b.textContent);
  assert.ok(buttons.includes("Install…"));
  assert.ok(buttons.includes("Update…"));
  assert.ok(buttons.includes("Reinstall…"));
  assert.equal(buttons.filter((b) => b === "Remove…").length, 2);

  assert.match(section.textContent, /CLI found — ready to register/);
  assert.match(section.textContent, /Registered with another path/);
  assert.match(section.textContent, /CLI not detected — config will be prepared anyway/);
  assert.match(section.textContent, /Registered/);

  const dots = section.find(".dot");
  assert.equal(dots.length, 4);
  const offClass = statusDot(false).className;
  const onClass = statusDot(true).className;
  assert.equal(dots[0].className, offClass); // claude: not registered
  assert.equal(dots[1].className, offClass); // codex: outdated path
  assert.equal(dots[2].className, offClass); // gemini: not registered
  assert.equal(dots[3].className, onClass); // antigravity: matches
});

test("install requires diff preview and explicit click before calling mochi_mcp_apply", async () => {
  const section = mcpSection(mcpList, true);
  const installBtn = section.find("button").find((b) => b.textContent === "Install…");
  assert.ok(installBtn);

  await installBtn.fire("click");
  await settle();

  // Mostrou o diff e caminho do backup, mas ainda não gravou nada.
  assert.equal(sent("mochi_mcp_preview").length, 1);
  assert.equal(sent("mochi_mcp_apply").length, 0);
  assert.match(section.textContent, /No existing file — nothing to back up\./);
  assert.ok(section.find(".diff").length >= 1);

  const cancelBtn = section.find("button").find((b) => b.textContent === "Cancel");
  assert.ok(cancelBtn);
  cancelBtn.fire("click");
  await settle();
  assert.equal(sent("mochi_mcp_apply").length, 0);

  // Segundo fluxo: confirma explicitamente com o fingerprint exibido.
  const installAgain = section.find("button").find((b) => b.textContent === "Install…");
  await installAgain.fire("click");
  await settle();

  const confirmBtn = section.find("button").find((b) => b.textContent === "Back up and write");
  assert.ok(confirmBtn);
  await confirmBtn.fire("click");
  await settle();

  assert.deepEqual(sent("mochi_mcp_apply"), [
    { agent: "claude", install: true, fingerprint: "fp-claude-in" },
  ]);
  assert.match(section.textContent, /Done\. Previous version saved as/);

  mock.timers.tick(2600);
  await settle();
  assert.equal(sent("mochi_mcp_status").length, 1);
  assert.match(section.textContent, /Reinstall…/);
});

test("remove flow passes install=false and handles preview/apply errors without losing state", async () => {
  const section = mcpSection(mcpList, true);

  // Erro na prévia exibe .notice.err e botão Back.
  previewError = "config.toml isn't valid TOML";
  const updateBtn = section.find("button").find((b) => b.textContent === "Update…");
  await updateBtn.fire("click");
  await settle();
  assert.match(section.textContent, /config\.toml isn't valid TOML/);
  const backBtn = section.find("button").find((b) => b.textContent === "Back");
  assert.ok(backBtn);
  backBtn.fire("click");
  await settle();

  // Erro no apply mantém botão habilitado e mostra erro; depois funciona na remoção do Antigravity.
  previewError = null;
  applyError = "file changed since preview";
  const removeButtons = section.find("button").filter((b) => b.textContent === "Remove…");
  assert.equal(removeButtons.length, 2);
  await removeButtons[1].fire("click");
  await settle();

  assert.match(section.textContent, /Backup → .*mcp_config\.json\.bak-/);
  const confirmRemove = section.find("button").find((b) => b.textContent === "Back up and remove");
  assert.ok(confirmRemove);
  await confirmRemove.fire("click");
  await settle();
  assert.equal(confirmRemove.disabled, false);
  assert.match(section.textContent, /Could not write: file changed since preview/);

  applyError = null;
  await confirmRemove.fire("click");
  await settle();
  assert.deepEqual(sent("mochi_mcp_apply").at(-1), {
    agent: "antigravity",
    install: false,
    fingerprint: "fp-antigravity-rm",
  });
});

test("mcpSection disables install when hook relay is missing and renders fallback + pt-BR", () => {
  setLanguage("pt-BR");
  const emptySection = mcpSection([]);
  assert.match(emptySection.textContent, /Ferramentas MCP locais \(Lumen\)/);
  assert.match(emptySection.textContent, /não conseguiu verificar os registros MCP locais/);

  const notReady = mcpSection(mcpList, false);
  const installBtn = notReady.find("button").find((b) => b.textContent === "Instalar…");
  assert.ok(installBtn);
  assert.equal(installBtn.disabled, true);
});
