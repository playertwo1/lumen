// Settings → Agents and Settings → Active pills, for Orca (src-tauri/src/orca.rs).
// Read-only, except "Open terminal", which only moves focus in Orca. Orca closed
// or missing shows its state and nothing else: never an error.

import { Bridge, type OrcaState, type OrcaTerminal } from "../core/bridge";
import { h } from "../views/dom";
import { statusDot } from "./parts";

function errorText(err: unknown): string {
  return String(err).replace(/^Error:\s*/, "");
}

/** Settings → Agents: Orca's install and whether its app is open. */
export function orcaAgentSection(state: OrcaState | null): HTMLElement {
  const label = !state?.installed ? "Não encontrado" : state.running ? "Aberto" : "Instalado";
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: "Orca" })),
    h("div", {
      class: "hint",
      text: "Ambiente de execução: o Lumen mostra os terminais do Orca em Abas ativas. O Lumen não altera a configuração do Orca.",
    }),
    h(
      "div",
      { style: "display:flex;align-items:center;gap:8px" },
      statusDot(!!state?.installed),
      h("span", { style: "font-size:12.5px", text: "Orca" }),
      h("span", { class: "hint", text: label }),
    ),
  );
}

/** Settings → Active pills: Orca's terminals as tabs, each with "Open terminal". None when Orca is closed. */
export function orcaTabsSections(state: OrcaState | null): HTMLElement[] {
  if (!state?.running) return [];
  const note = h("div", { class: "hint" });
  const rows = state.terminals.length
    ? state.terminals.map((terminal) => orcaTabRow(terminal, note))
    : [h("div", { class: "hint", text: "O Orca está aberto e não tem terminais." })];
  return [
    h(
      "section",
      {},
      h("h2", {}, h("span", { text: "Orca" })),
      h("div", { class: "hint", text: "Terminais abertos no Orca. Abrir um terminal só traz a aba dele para a frente." }),
      note,
      h("div", { class: "pill-group" }, ...rows),
    ),
  ];
}

function orcaTabRow(terminal: OrcaTerminal, note: HTMLElement): HTMLElement {
  const open = h("button", { text: "Abrir terminal" }) as HTMLButtonElement;
  open.addEventListener("click", async () => {
    open.disabled = true;
    note.className = "hint";
    note.textContent = "";
    try {
      await Bridge.orcaFocusTerminal(terminal.handle);
    } catch (err) {
      note.className = "notice err";
      note.textContent = errorText(err);
    } finally {
      open.disabled = false;
    }
  });
  return h(
    "div",
    { class: "pill-row" },
    h("span", { class: "name", text: terminal.title }),
    h("span", { class: "state", text: terminal.connected ? "Conectado" : "Desconectado" }),
    open,
  );
}
