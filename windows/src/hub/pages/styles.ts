// Styles: an optional clean-up by Claude (no "um", no repeats, corrections
// applied), as said, natural or polished, per app. Port of the styles part of
// FlowHubWords.swift.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { FlowBridge, type AppStyle, type Style, type SummaryEngine } from "../../core/flow";
import type { HubContext } from "../shell";
import { pageSave } from "../save";
import { card, divider, iconButton, label, row, select, toggle } from "../ui";
import { summaryAvailability } from "./flowsettings";

export function styleOptions(): [Style, string][] {
  return [["verbatim", t("As said")], ["natural", t("Natural")], ["polished", t("Polished")]];
}

/** Who cleans the words: the Notetaker's engines, without "off" (the toggle turns the clean-up off). */
export const cleanupOptions: [SummaryEngine, string][] = [
  ["claude", "Claude (chave ou CLI)"],
  ["chatgpt", "ChatGPT / Codex (OAuth ou CLI)"],
  ["gemini", "Antigravity / Gemini (CLI · OAuth Google)"],
  ["chat", "Mesmo provedor do Chat"],
];

/** Where the words go with each engine. Never the voice. */
export function cleanupDestination(engine: SummaryEngine): string {
  switch (engine) {
    case "chatgpt":
      return "Com ChatGPT / Codex, o texto (nunca sua voz) vai para o ChatGPT pela sua conta conectada por OAuth, com fallback para a CLI do Codex. Desligada, tudo fica neste computador.";
    case "gemini":
      return "Com Antigravity / Gemini, o texto (nunca sua voz) vai para a CLI oficial (agy ou gemini) com seu login Google, ou para sua chave Google AI se houver. Desligada, tudo fica neste computador.";
    case "chat":
      return "Com o mesmo provedor do Chat, o texto (nunca sua voz) vai para o provedor e o modelo ativos em Ajustes → Chat, que podem ser um servidor local. Desligada, tudo fica neste computador.";
    default:
      return t("With the clean-up on, the text (never your voice) goes to Anthropic with your Claude key. Off, everything stays on this computer.");
  }
}

export async function stylesPage(page: HTMLElement, ctx: HubContext) {
  const status = await ctx.refresh();
  let styles: AppStyle[] = (await FlowBridge.styles()) ?? [];
  page.append(ctx.hero("styles", t("Styles"), "A IA escolhida limpa o que você fala, no estilo do app em que você escreve."));
  const write = pageSave(page);
  const save = async (next: AppStyle[]) => {
    const result = await write(() => FlowBridge.setStyles(next));
    if (result === "pending") return;
    if (result === "saved") styles = next;
    // Restore the actual selection as well when the native store rejects it.
    drawApps();
  };

  const perApp = h("div", {});
  const drawApps = () => {
    clear(perApp);
    perApp.classList.toggle("dimmed", !ctx.status.prefs.cleanup);
    styles.forEach((s, i) => {
      if (i > 0) perApp.append(divider());
      perApp.append(row(s.name, null,
        select(styleOptions(), s.style, (style) => {
          void save(styles.map((x) => (x.id === s.id ? { ...x, style } : x)));
        }),
        iconButton("x", t("Delete"), () => {
          void save(styles.filter((x) => x.id !== s.id));
        }),
      ));
    });
  };

  const engine: SummaryEngine = cleanupOptions.some(([id]) => id === status.prefs.cleanupEngine) ? status.prefs.cleanupEngine : "claude";
  const keyNote = h("div", { class: "hub-warn" });
  const cleanupNote = h("div", { class: "hub-note" });
  // What is missing for the chosen engine, and where the text goes.
  const drawEngine = (next: SummaryEngine) => {
    if (next === "claude") {
      keyNote.textContent = t("The clean-up uses your Claude key. Add it in Notch → Chat.");
      keyNote.style.display = status.hasClaudeKey ? "none" : "";
    } else {
      const info = summaryAvailability(next, status.summaryBackends);
      keyNote.textContent = info.text;
      keyNote.style.display = info.ready || !info.text ? "none" : "";
    }
    cleanupNote.textContent = cleanupDestination(next);
  };
  drawEngine(engine);
  const defaultRow = row(t("Other apps"), null, select(styleOptions(), status.prefs.defaultStyle, (style) => void ctx.setPrefs((p) => (p.defaultStyle = style))));
  const stylesCard = card(
    row("Limpar o texto ditado", t("Removes the “um”, the repeats and the corrections you made while speaking, and fixes the punctuation."),
      toggle(status.prefs.cleanup, (on) => {
        void ctx.setPrefs((p) => (p.cleanup = on));
        perApp.classList.toggle("dimmed", !on);
      })),
    row("Quem faz a limpeza", null, select(cleanupOptions, engine, (next) => {
      drawEngine(next);
      void ctx.setPrefs((p) => (p.cleanupEngine = next));
    })),
    keyNote,
    divider(),
    defaultRow,
    cleanupNote,
  );
  stylesCard.dataset.anchor = "flow.styles";
  page.append(stylesCard);

  // "Add an app": the apps with a window right now.
  const add = h("select", { class: "hub-select" }) as HTMLSelectElement;
  const fillAdd = async () => {
    clear(add);
    add.append(h("option", { value: "", text: `+ ${t("Add an app")}` }));
    const running = ((await FlowBridge.runningApps()) ?? []).filter((a) => !styles.some((s) => s.id === a.id));
    if (running.length === 0) add.append(h("option", { value: "-", text: t("No other open app"), disabled: true }));
    for (const a of running) add.append(h("option", { value: `${a.id}\n${a.name}`, text: a.name }));
    add.value = "";
  };
  add.addEventListener("focus", () => void fillAdd());
  add.addEventListener("change", async () => {
    const [id, name] = add.value.split("\n");
    if (!id || id === "-") return;
    await save([...styles, { id, name: name ?? id, style: ctx.status.prefs.defaultStyle }]);
    void fillAdd();
  });
  void fillAdd();
  page.append(h("div", { class: "hub-row" }, label(t("Style per app")), h("span", { class: "spacer", style: "flex:1" }), add));
  page.append(card(perApp));
  drawApps();
}
