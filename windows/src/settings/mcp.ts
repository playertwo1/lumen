// Settings -> Agents: registro seguro do servidor MCP local do Lumen nas
// CLIs externas (Claude Code, Codex, Gemini CLI, Antigravity). Sempre mostra
// o diff exato e o caminho do backup antes de qualquer gravação por clique.

import { MochiBridge, type AgentHookPlan, type McpAgentStatus } from "../core/bridge";
import { clear, h } from "../views/dom";
import { renderDiff, statusDot } from "./parts";
import { t } from "../i18n/i18n";

const TEXT = {
  get title() { return t("Local MCP tools (Lumen)"); },
  get intro() {
    return t("Expose Lumen tools to your local CLIs over an authenticated loopback MCP bridge. Preview the exact diff and backup path before anything is written.");
  },
  get none() { return t("Coucou could not check local MCP registrations."); },
  get registered() { return t("Registered"); },
  get outdated() { return t("Registered with another path"); },
  get cliFound() { return t("CLI found — ready to register"); },
  get cliNotFound() { return t("CLI not detected — config will be prepared anyway"); },
  get install() { return t("Install…"); },
  get update() { return t("Update…"); },
  get reinstall() { return t("Reinstall…"); },
  get remove() { return t("Remove…"); },
  get relayMissing() { return t("The relay isn't installed yet. Restart Coucou."); },
  previewInstall: (name: string) => t("This is exactly what changes for {name}. Nothing else is touched.", { name }),
  get previewRemove() { return t("This removes Coucou's entries only. Everything else stays."); },
  backup: (to: string) => (to ? t("Backup → {path}", { path: to }) : t("No existing file — nothing to back up.")),
  get confirmInstall() { return t("Back up and write"); },
  get confirmRemove() { return t("Back up and remove"); },
  get cancel() { return t("Cancel"); },
  get back() { return t("Back"); },
  done: (backups: string) => (
    backups
      ? t("Done. Previous version saved as {paths}.", { paths: backups.split("\n").join(", ") })
      : t("Done.")
  ),
  failed: (err: unknown) => t("Could not write: {error}", { error: errorText(err) }),
};

function errorText(err: unknown): string {
  return String(err).replace(/^Error:\s*/, "");
}

export function mcpSection(list: McpAgentStatus[] | null, hookReady = true): HTMLElement {
  const blocks = h("div", { style: "display:flex;flex-direction:column;gap:14px" });
  if (!list || list.length === 0) {
    blocks.append(h("div", { class: "hint", text: TEXT.none }));
  }
  for (const status of list ?? []) blocks.append(mcpAgentBlock(status, hookReady));
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: TEXT.title })),
    h("div", { class: "hint", text: TEXT.intro }),
    blocks,
  );
}

function mcpAgentBlock(initial: McpAgentStatus, hookReady: boolean): HTMLElement {
  let status = initial;
  const head = h("div", { class: "row" });
  const body = h("div", { style: "display:flex;flex-direction:column;gap:10px" });
  const block = h("div", { style: "display:flex;flex-direction:column;gap:8px" }, head, body);

  async function refresh() {
    const fresh = (await MochiBridge.mcpStatus().catch(() => null))?.find((a) => a.id === status.id);
    if (fresh) status = fresh;
    draw();
  }

  function statusHint(): string {
    if (status.registered) {
      return status.commandMatches ? TEXT.registered : TEXT.outdated;
    }
    return status.cliFound ? TEXT.cliFound : TEXT.cliNotFound;
  }

  function primaryLabel(): string {
    if (!status.registered) return TEXT.install;
    return status.commandMatches ? TEXT.reinstall : TEXT.update;
  }

  function draw() {
    clear(head);
    clear(body);
    const install = h("button", {
      class: "primary",
      text: primaryLabel(),
      onclick: () => void showPreview(true),
    });
    if (!hookReady) {
      install.disabled = true;
      install.title = TEXT.relayMissing;
    }
    head.append(
      h(
        "div",
        { style: "display:flex;align-items:center;gap:8px;min-width:132px" },
        statusDot(status.registered && status.commandMatches),
        h("span", { style: "font-size:12.5px", text: status.name }),
      ),
      h("span", { class: "hint", text: statusHint() }),
      h("span", { class: "spacer" }),
      install,
    );
    if (status.registered) {
      head.append(
        h("button", {
          class: "danger",
          text: TEXT.remove,
          onclick: () => void showPreview(false),
        }),
      );
    }
    body.append(h("span", { class: "path", text: status.path }));
  }

  async function showPreview(install: boolean) {
    let plan: AgentHookPlan;
    try {
      plan = await MochiBridge.mcpPreview(status.id, install);
    } catch (err) {
      clear(body);
      body.append(
        h("div", { class: "notice err", text: errorText(err) }),
        h("div", { class: "row" }, h("button", { text: TEXT.back, onclick: draw })),
      );
      return;
    }

    clear(body);
    body.append(
      h("div", { class: "hint", text: install ? TEXT.previewInstall(status.name) : TEXT.previewRemove }),
      renderDiff(plan.diff),
      h("div", { class: "row" }, h("span", { class: "path", text: TEXT.backup(plan.backup) })),
    );

    const confirm = h("button", {
      class: install ? "primary" : "danger",
      text: install ? TEXT.confirmInstall : TEXT.confirmRemove,
    });
    confirm.addEventListener("click", async () => {
      confirm.disabled = true;
      try {
        const backups = await MochiBridge.mcpApply(status.id, install, plan.fingerprint);
        clear(body);
        body.append(h("div", { class: "notice ok", text: TEXT.done(backups) }));
        globalThis.setTimeout(() => void refresh(), 2600);
      } catch (err) {
        confirm.disabled = false;
        body.append(h("div", { class: "notice err", text: TEXT.failed(err) }));
      }
    });
    body.append(h("div", { class: "row" }, confirm, h("button", { text: TEXT.cancel, onclick: draw })));
  }

  draw();
  return block;
}
