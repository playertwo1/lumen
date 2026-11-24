// E-mail pessoal (MailCraft principal + Outlook clássico secundário):
// leitura, resumo da caixa e preparo de rascunhos sem envio automático, além
// de regras, palavras proibidas, VIPs e diário de triagem.

import { h, clear } from "../../views/dom";
import { parseMarkdown, parseInline, type Inline } from "../../views/markdown";
import { t, LUMEN_WINDOWS } from "../../i18n/i18n";
import {
  MAIL_BLUE,
  MAIL_FIELDS,
  MailBridge,
  fieldTitle,
  journalLine,
  nameOf,
  reasonLine,
  ruleLine,
  ruleName,
  type ClassifiedMail,
  type MailAction,
  type MailBackend,
  type MailCategory,
  type MailCraftConfig,
  type MailCraftMode,
  type MailCraftStatus,
  type MailField,
  type MailSettings,
  type TriageView,
} from "../../core/mail";
import type { HubContext, PageId } from "../shell";
import { emptyState } from "../hero";
import { button, card, divider, icon, iconButton, label, row, select, toggle } from "../ui";

const EMPTY: TriageView = {
  settings: { autoSort: false, autoSortSince: null, banned: [], vip: [], rules: [] },
  journal: [],
  sent: [],
  today: { sorted: 0, junk: 0, important: 0 },
  windows: true,
  outlookOpen: false,
};

const pageAccent = () => (LUMEN_WINDOWS ? "#E99A3D" : MAIL_BLUE);
const lang = () => document.documentElement.lang || (LUMEN_WINDOWS ? "pt-BR" : undefined);

async function load(): Promise<TriageView> {
  return (await MailBridge.triage()) ?? EMPTY;
}

function save(settings: MailSettings) {
  void MailBridge.setTriage(settings);
}

function input(placeholder: string, onEnter?: () => void, ariaLabel?: string): HTMLInputElement {
  const el = h("input", {
    class: "hub-input",
    placeholder,
    "aria-label": ariaLabel ?? placeholder,
  }) as HTMLInputElement;
  if (onEnter) el.addEventListener("keydown", (e) => e.key === "Enter" && onEnter());
  return el;
}

function time(ms: number, withDay = false): string {
  try {
    return new Intl.DateTimeFormat(
      lang(),
      withDay
        ? { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }
        : { hour: "2-digit", minute: "2-digit" },
    ).format(new Date(ms));
  } catch {
    return new Date(ms).toLocaleTimeString();
  }
}

/** Aviso discreto quando a triagem automática do Outlook clássico está desligada. */
function autoSortHint(view: TriageView, redraw: () => void): HTMLElement | null {
  // No MailCraft estas listas valem sempre, na triagem da Visão geral.
  if (view.settings.autoSort || view.backend !== "outlook") return null;
  return h(
    "div",
    { class: "mail-autosort-banner", role: "status" },
    h("span", { text: t("Automatic sorting is off: these settings do nothing until it is on.") }),
    button(
      t("Turn on"),
      () => {
        save({ ...view.settings, autoSort: true });
        window.setTimeout(redraw, 300);
      },
      "primary",
      true,
    ),
  );
}

function appendSummaryInline(into: HTMLElement, nodes: Inline[]) {
  for (const node of nodes) {
    switch (node.kind) {
      case "text":
        into.append(document.createTextNode(node.text));
        break;
      case "code":
        into.append(h("code", { class: "mail-summary-inline-code", text: node.text }));
        break;
      case "strong": {
        const el = h("strong");
        appendSummaryInline(el, node.children);
        into.append(el);
        break;
      }
      case "em": {
        const el = h("em");
        appendSummaryInline(el, node.children);
        into.append(el);
        break;
      }
      case "link":
        // E-mails externos nunca viram links clicáveis dentro do resumo do Lumen.
        appendSummaryInline(into, node.children);
        break;
    }
  }
}

/** Renderiza o Markdown do resumo em blocos ordenados (títulos, parágrafos e listas) usando nós DOM seguros. */
export function renderSummaryBody(target: HTMLElement, summary: string, warnings: string[] = []) {
  clear(target);
  const normalized = summary
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => line.replace(/^(\s*)[•–—]\s+/, "$1- "))
    .join("\n");

  const blocks = parseMarkdown(normalized);
  let activeList: HTMLUListElement | HTMLOListElement | null = null;
  let activeOrdered = false;

  const flushList = () => {
    if (activeList) {
      target.append(activeList);
      activeList = null;
    }
  };

  for (const block of blocks) {
    if (block.kind === "item") {
      const isOrdered = block.prefix !== "•";
      if (!activeList || activeOrdered !== isOrdered) {
        flushList();
        activeOrdered = isOrdered;
        activeList = (
          isOrdered
            ? h("ol", { class: "mail-summary-list is-ordered" })
            : h("ul", { class: "mail-summary-list" })
        ) as HTMLUListElement | HTMLOListElement;
      }
      const li = h("li");
      if (block.indent > 0) li.style.marginInlineStart = `${Math.min(block.indent, 3) * 14}px`;
      appendSummaryInline(li, parseInline(block.text));
      activeList.append(li);
      continue;
    }

    flushList();
    switch (block.kind) {
      case "heading": {
        const tag = block.level <= 2 ? "h3" : "h4";
        const heading = h(tag, { class: block.level <= 2 ? "mail-summary-heading" : "mail-summary-heading is-sub" });
        appendSummaryInline(heading, parseInline(block.text));
        target.append(heading);
        break;
      }
      case "paragraph": {
        const p = h("p", { class: "mail-summary-prose" });
        appendSummaryInline(p, parseInline(block.text));
        target.append(p);
        break;
      }
      case "code":
        target.append(h("pre", { class: "mail-summary-code" }, h("code", { text: block.code })));
        break;
      case "quote": {
        const q = h("blockquote", { class: "mail-summary-quote" });
        appendSummaryInline(q, parseInline(block.text));
        target.append(q);
        break;
      }
      case "rule":
        target.append(h("hr", { class: "mail-summary-rule" }));
        break;
    }
  }
  flushList();

  for (const w of warnings) {
    target.append(h("p", { class: "mail-summary-warning", text: w }));
  }
}

/** Atualiza a barra estruturada de leitura da conexão MailCraft. */
function setConnectionReadout(
  badge: HTMLElement,
  readout: HTMLElement,
  tone: "idle" | "busy" | "ok" | "err",
  badgeText: string,
  message: string,
  endpoint?: string,
) {
  badge.className = tone === "ok" || tone === "err" ? `mail-conn-pill ${tone}` : "mail-conn-pill";
  badge.textContent = badgeText;
  readout.className = tone === "ok" || tone === "err" ? `mail-status-readout ${tone}` : "mail-status-readout";
  clear(readout);
  readout.append(h("span", { class: "mail-status-dot", "aria-hidden": "true" }));
  const body = h("span", { class: "mail-status-copy" });
  if (endpoint) {
    body.append(h("code", { class: "mail-mono-code", text: endpoint }), h("span", { text: ` — ${message}` }));
  } else {
    body.append(h("span", { text: message }));
  }
  readout.append(body);
}

/** Atalho de navegação lateral discreto para as páginas secundárias de triagem. */
function triageAside(
  ctx: HubContext,
  current: "mailRules" | "mailBanned" | "mailVIP",
  view: TriageView,
): HTMLElement {
  const stepLink = (id: PageId, order: string, title: string, desc: string) => {
    const isCurrent = id === current;
    const cls = isCurrent ? "mail-precedence-item current" : "mail-precedence-item";
    const btn = h(
      "button",
      {
        type: "button",
        class: cls,
        onclick: () => ctx.navigate(id),
        ...(isCurrent ? { "aria-current": "page" } : {}),
      },
      h("span", { class: "mail-precedence-step", text: order }),
      h(
        "span",
        { class: "mail-precedence-text" },
        h("span", { class: "mail-precedence-title", text: title }),
        h("span", { class: "mail-precedence-desc", text: desc }),
      ),
    );
    return btn;
  };

  const activeRules = view.settings.rules.filter((r) => r.enabled).length;
  const onMailCraft = view.backend !== "outlook";
  const statusLabel = onMailCraft
    ? "Usadas na triagem do MailCraft"
    : view.settings.autoSort ? "Triagem automática ligada" : "Triagem automática desligada";
  const statusTone = onMailCraft || view.settings.autoSort ? "mail-aside-state ok" : "mail-aside-state";

  const guide = card(
    h(
      "div",
      { class: "mail-section-head" },
      h("h2", { class: "mail-section-title", text: "Ordem de prioridade" }),
    ),
    h("div", {
      class: "mail-section-sub",
      text: "Na triagem, o Lumen avalia cada e-mail nesta sequência, sem apagar mensagens:",
    }),
    h(
      "div",
      { class: "mail-precedence-list" },
      stepLink("mailVIP", "1º", "Remetentes VIP", `${view.settings.vip.length} ${view.settings.vip.length === 1 ? "item" : "itens"} · sempre importantes`),
      stepLink("mailBanned", "2º", "Palavras proibidas", `${view.settings.banned.length} ${view.settings.banned.length === 1 ? "item" : "itens"} · vão para o lixo`),
      stepLink("mailRules", "3º", "Regras personalizadas", `${activeRules}/${view.settings.rules.length} ativas · primeira que casar vence`),
    ),
    divider(),
    h(
      "div",
      { class: "mail-aside-foot" },
      h("span", { class: statusTone, text: statusLabel }),
      button("Abrir visão geral", () => ctx.navigate("mailOverview"), "", true),
    ),
  );
  guide.classList.add("mail-card");
  guide.classList.add("mail-aside-card");
  return guide;
}

// ── Triagem do MailCraft ─────────────────────────────────────────────────────

const CATEGORY_ORDER: [MailCategory, string][] = [
  ["conta", "Contas a pagar"],
  ["assinatura", "Renovações de assinatura"],
  ["importante", "Importantes"],
  ["compra", "Compras e entregas"],
  ["notificacao", "Notificações automáticas"],
  ["outros", "Outros"],
  ["promocao", "Promoções"],
  ["spam", "Spam"],
];

const LOW_SIGNAL_CATEGORIES = new Set<MailCategory>(["promocao", "spam"]);

/** Os números do topo filtram a lista: "Todas" mostra tudo, os outros focam um recorte. */
type TriageFilter = "importantes" | "apagar" | "promo";
const TRIAGE_FILTERS: { id: TriageFilter; caption: string; tone: "accent" | "warn" | "neutral"; has: (i: ClassifiedMail) => boolean }[] = [
  { id: "importantes", caption: "prioritários", tone: "accent", has: (i) => ["conta", "assinatura", "importante"].includes(i.category) },
  { id: "apagar", caption: "recomendo apagar", tone: "warn", has: (i) => i.recommend === "apagar" },
  { id: "promo", caption: "promoções e spam", tone: "neutral", has: (i) => i.category === "promocao" || i.category === "spam" },
];

/** Some com a linha: desliza e fecha a altura, para a lista não pular. */
function leaveRow(row: HTMLElement, done: () => void) {
  if (typeof row.animate !== "function") return done();
  const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const h0 = row.getBoundingClientRect().height;
  const anim = reduced
    ? row.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 120, easing: "ease-out" })
    : row.animate(
        [
          { opacity: 1, transform: "translateX(0)", height: `${h0}px`, offset: 0 },
          { opacity: 0, transform: "translateX(14px)", height: `${h0}px`, offset: 0.55 },
          { opacity: 0, transform: "translateX(14px)", height: "0px", paddingTop: "0px", paddingBottom: "0px" },
        ],
        { duration: 280, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
      );
  anim.onfinish = done;
  anim.oncancel = done;
}

/** Arma confirmação em dois cliques e desarma por Escape ou perda de foco (sem cronômetro). */
function bindTwoClickConfirm(
  btn: HTMLButtonElement,
  idleText: string,
  isBusy: () => boolean,
) {
  const disarm = () => {
    if (isBusy() || !btn.classList.contains("is-armed")) return;
    btn.classList.remove("is-armed");
    btn.textContent = idleText;
  };
  btn.addEventListener("blur", () => disarm());
  btn.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && btn.classList.contains("is-armed")) {
      e.stopPropagation();
      disarm();
    }
  });
}

type TriageCardOptions = {
  onReply?: (item: ClassifiedMail) => void;
};

/** Lê a caixa do MailCraft, separa contas, renovações, importantes, compras, notificações, promoções e spam e recomenda o que apagar. */
function mailCraftTriageCard(initial: ClassifiedMail[], opts: TriageCardOptions = {}): HTMLElement {
  const stats = h("div", { class: "mail-stats mail-triage-filters", role: "group", "aria-label": "Filtrar a triagem" });
  const filterNote = h("div", { class: "mail-filter-note is-empty", role: "status", "aria-live": "polite" });
  const list = h("div", { class: "mail-triage-results" });
  const status = h("div", { class: "mail-status-line is-empty", role: "status", "aria-live": "polite" });
  let items = initial.slice();
  let filter: TriageFilter | null = null;
  const collapsedGroups = new Set<MailCategory>(["promocao", "spam"]);
  let rows = new Map<ClassifiedMail, HTMLElement>();
  let closeOpenMenu: (() => void) | null = null;

  // A explicação inteira antes da primeira triagem; depois, uma linha e "Saiba mais".
  const INTRO_FULL =
    "O Lumen analisa toda a caixa de entrada (até 200 mensagens), separa contas, renovações, importantes, compras, notificações, promoções e spam, e recomenda o que apagar. Nada sai sem o seu clique (vai para Excluídos ou Spam) e nada é enviado.";
  const INTRO_SHORT = "Só recomenda: nada sai da caixa sem o seu clique, e tudo vai para Excluídos ou Spam.";
  const introText = h("span", { text: INTRO_FULL });
  const introMore = h("button", { class: "mail-filter-clear mail-intro-more", type: "button", text: "Saiba mais" });
  const intro = h("div", { class: "mail-section-sub mail-triage-intro" }, introText, introMore);
  let introOpen = false;
  const syncIntro = () => {
    const short = items.length > 0 && !introOpen;
    introText.textContent = short ? INTRO_SHORT : INTRO_FULL;
    introMore.textContent = introOpen ? "Menos" : "Saiba mais";
    introMore.classList.toggle("is-empty", items.length === 0);
    introMore.setAttribute("aria-expanded", String(introOpen));
  };
  introMore.addEventListener("click", () => {
    introOpen = !introOpen;
    syncIntro();
  });
  let bulkBusy = false;

  /** O MailCraft está aberto sem `--control`: oferece reiniciá-lo com o canal (um clique do usuário). */
  const showError = (err: unknown) => {
    const text = String(err).replace(/^Error:\s*/, "");
    status.className = "mail-status-line err";
    status.textContent = text;
    if (!text.includes("--control")) return;
    const fix = button("Reiniciar MailCraft com controle", () => {
      fix.disabled = true;
      fix.textContent = "Reiniciando…";
      void MailBridge.restartMailCraft()
        .then((msg) => {
          status.className = "mail-status-line ok";
          status.textContent = `${msg} Clique em Fazer triagem de novo.`;
        })
        .catch(showError);
    }, "primary", true);
    status.append(" ", fix);
  };

  /** `confirm`: o primeiro clique arma a confirmação, o segundo executa. `removeRow`: some com a linha. */
  const actionButton = (
    text: string,
    run: () => Promise<void>,
    optsBtn: { confirm?: string; removeRow?: () => void; kind?: "" | "danger"; label?: string } = {},
  ) => {
    let busy = false;
    const btn = button(text, () => {
      if (busy) return;
      if (optsBtn.confirm && !btn.classList.contains("is-armed")) {
        btn.textContent = optsBtn.confirm;
        btn.classList.add("is-armed");
        return;
      }
      busy = true;
      btn.classList.remove("is-armed");
      btn.disabled = true;
      btn.setAttribute("aria-busy", "true");
      void run()
        .then(() => (optsBtn.removeRow ? optsBtn.removeRow() : btn.remove()))
        .catch((err) => {
          busy = false;
          btn.disabled = false;
          btn.removeAttribute("aria-busy");
          btn.textContent = text;
          showError(err);
        });
    }, optsBtn.kind ?? "", true);
    if (optsBtn.confirm) bindTwoClickConfirm(btn, text, () => busy);
    if (optsBtn.label) btn.setAttribute("aria-label", optsBtn.label);
    return btn;
  };

  /** Depois de Apagar/Spam, no lugar da linha: "Desfazer" devolve a mensagem para a Entrada e para a lista. */
  const undoStrip = (item: ClassifiedMail, at: number, where: "Excluídos" | "Spam") => {
    const undo = h("button", { class: "mail-filter-clear mail-undo", type: "button", text: "Desfazer" }) as HTMLButtonElement;
    undo.setAttribute("aria-label", `Desfazer: voltar para a Entrada: ${item.message.subject}`);
    undo.addEventListener("click", () => {
      undo.disabled = true;
      undo.textContent = "Desfazendo…";
      void MailBridge.actChecked(item.message, where === "Spam" ? "unjunk" : "undelete")
        .then(() => {
          items.splice(Math.min(at, items.length), 0, item);
          drawStats();
          drawList();
          status.className = "mail-status-line ok";
          status.textContent = "De volta à Entrada.";
        })
        .catch((err) => {
          undo.disabled = false;
          undo.textContent = "Desfazer";
          showError(err);
        });
    });
    return h(
      "div",
      { class: "mail-undo-row", role: "status" },
      h("span", { class: "mail-undo-text", text: `${item.message.senderName || item.message.address} · movida para ${where}` }),
      undo,
    );
  };

  const setFilter = (next: TriageFilter | null) => {
    filter = next;
    drawStats();
    drawList();
  };

  /** Uma mensagem por vez, na ordem da lista; permite cancelar antes da próxima e mantém o desfazer do que já foi movido. */
  const bulkDelete = async (targets: ClassifiedMail[], btn: HTMLButtonElement) => {
    closeOpenMenu?.();
    bulkBusy = true;
    let cancelled = false;
    btn.disabled = true;
    btn.setAttribute("aria-busy", "true");
    btn.classList.remove("is-armed");
    const cancelBtn = button("Cancelar", () => {
      cancelled = true;
      cancelBtn.disabled = true;
      cancelBtn.textContent = "Cancelando…";
    }, "", true);
    if (typeof btn.after === "function") btn.after(cancelBtn);
    else btn.parentElement?.append(cancelBtn);

    const done: { item: ClassifiedMail; at: number }[] = [];
    let failure: unknown = null;
    for (const item of targets) {
      if (cancelled) break;
      btn.textContent = `Apagando ${done.length + 1} de ${targets.length}…`;
      try {
        await MailBridge.actChecked(item.message, "delete");
        done.push({ item, at: items.indexOf(item) });
        const row = rows.get(item);
        if (row) leaveRow(row, () => row.remove());
      } catch (err) {
        failure = err;
        break;
      }
    }
    cancelBtn.remove();
    if (done.length > 0 && rows.size > 0) {
      await new Promise((r) => window.setTimeout(r, 300));
    }
    items = items.filter((i) => !done.some((d) => d.item === i));
    bulkBusy = false;
    drawStats();
    drawList();
    if (done.length > 0) {
      const strip = bulkUndoStrip(done);
      list.prepend(strip);
      (strip.querySelector(".mail-undo") as HTMLButtonElement | null)?.focus();
    }
    if (cancelled) {
      status.className = "mail-status-line";
      status.textContent = `Limpeza interrompida após ${done.length} de ${targets.length}.`;
    } else if (failure) {
      showError(failure);
    }
  };

  /** "Apagar os N" / "Limpar grupo (N)": um clique arma, o segundo move o conjunto para Excluídos. */
  const makeBulkButton = (
    targets: ClassifiedMail[],
    idleText: string,
    armedText: string,
    ariaLabel: string,
    extraClass: string,
  ) => {
    const btn = button(idleText, () => {
      if (bulkBusy) return;
      if (!btn.classList.contains("is-armed")) {
        btn.classList.add("is-armed");
        btn.textContent = armedText;
        return;
      }
      void bulkDelete(targets, btn);
    }, "danger", true);
    btn.classList.add(extraClass);
    btn.setAttribute("aria-label", ariaLabel);
    bindTwoClickConfirm(btn, idleText, () => bulkBusy);
    return btn;
  };

  /** Desfaz o lote: devolve para a Entrada, uma por vez, e as linhas voltam ao lugar. */
  const bulkUndoStrip = (done: { item: ClassifiedMail; at: number }[]) => {
    const undo = h("button", { class: "mail-filter-clear mail-undo", type: "button", text: "Desfazer" });
    undo.setAttribute("aria-label", `Desfazer: voltar as ${done.length} mensagens para a Entrada`);
    undo.addEventListener("click", async () => {
      undo.disabled = true;
      const back: typeof done = [];
      for (const d of done) {
        undo.textContent = `Voltando ${back.length + 1} de ${done.length}…`;
        try {
          await MailBridge.actChecked(d.item.message, "undelete");
          back.push(d);
        } catch (err) {
          showError(err);
          break;
        }
      }
      for (const d of [...back].sort((a, b) => a.at - b.at)) items.splice(Math.min(d.at, items.length), 0, d.item);
      drawStats();
      drawList();
      if (back.length === done.length) {
        status.className = "mail-status-line ok";
        status.textContent = `${back.length} ${back.length === 1 ? "mensagem" : "mensagens"} de volta à Entrada.`;
      }
    });
    const n = done.length;
    return h(
      "div",
      { class: "mail-undo-row", role: "status" },
      h("span", { class: "mail-undo-text", text: `${n} ${n === 1 ? "mensagem movida" : "mensagens movidas"} para Excluídos` }),
      undo,
    );
  };

  const drawStats = () => {
    syncIntro();
    clear(stats);
    clear(filterNote);
    if (items.length === 0) {
      filterNote.classList.add("is-empty");
      return;
    }

    const allOn = filter === null;
    const allTile = h(
      "button",
      {
        class: `mail-stat-tile mail-filter-tile${allOn ? " is-on" : ""}`,
        type: "button",
        "aria-pressed": String(allOn),
        "aria-label": `${items.length} todas as mensagens`,
      },
      h("span", { class: "mail-stat", text: String(items.length) }),
      h(
        "span",
        { class: "mail-stat-label" },
        h("span", { class: "mail-stat-dot", "aria-hidden": "true" }),
        h("span", { text: "todas" }),
      ),
    );
    allTile.addEventListener("click", () => setFilter(null));
    stats.append(allTile);

    for (const f of TRIAGE_FILTERS) {
      const on = filter === f.id;
      const count = items.filter(f.has).length;
      const tile = h(
        "button",
        {
          class: `mail-stat-tile mail-filter-tile${on ? " is-on" : ""}`,
          type: "button",
          "aria-pressed": String(on),
          "aria-label": `${count} ${f.caption}`,
        },
        h("span", { class: "mail-stat", text: String(count) }),
        h(
          "span",
          { class: "mail-stat-label" },
          h("span", { class: `mail-stat-dot ${f.tone}`, "aria-hidden": "true" }),
          h("span", { text: f.caption }),
        ),
      );
      tile.addEventListener("click", () => setFilter(on ? null : f.id));
      stats.append(tile);
    }

    const active = TRIAGE_FILTERS.find((f) => f.id === filter);
    filterNote.classList.toggle("is-empty", !active);
    if (active) {
      const clearBtn = h("button", { class: "mail-filter-clear", type: "button", text: "Mostrar tudo" });
      clearBtn.addEventListener("click", () => setFilter(null));
      const targets = items.filter(active.has);
      filterNote.append(h("span", { text: `Só ${active.caption}: ${targets.length} de ${items.length}` }), clearBtn);
      if (active.id === "apagar" && targets.length > 0) {
        filterNote.append(
          makeBulkButton(
            targets,
            `Apagar os ${targets.length}`,
            `Apagar ${targets.length}? Clique de novo`,
            `Apagar as ${targets.length} mensagens recomendadas (vão para Excluídos)`,
            "mail-bulk-delete",
          ),
        );
      }
    }
  };

  const drawList = () => {
    closeOpenMenu?.();
    closeOpenMenu = null;
    clear(list);
    rows = new Map();
    if (items.length === 0) {
      list.append(h("div", { class: "mail-section-sub", text: "Clique em Fazer triagem para o Lumen analisar as mensagens da caixa de entrada." }));
      return;
    }
    const active = TRIAGE_FILTERS.find((f) => f.id === filter);
    const shown = active ? items.filter(active.has) : items;
    if (shown.length === 0) {
      list.append(h("div", { class: "mail-section-sub mail-triage-empty", text: "Nada nesta seleção agora." }));
      return;
    }

    let rowIdx = 0;
    for (const [cat, title] of CATEGORY_ORDER) {
      const group = shown.filter((i) => i.category === cat);
      if (group.length === 0) continue;

      const isLowSignal = LOW_SIGNAL_CATEGORIES.has(cat);
      const isCollapsed = filter === null && isLowSignal && collapsedGroups.has(cat);
      const countEl = h("span", { class: "mail-chips-count", text: String(group.length) });
      const groupToggle = h(
        "button",
        {
          type: "button",
          class: "mail-triage-group-toggle",
          "aria-expanded": String(!isCollapsed),
          "aria-label": `${title}, ${group.length} ${group.length === 1 ? "mensagem" : "mensagens"}`,
        },
        icon("chevronRight"),
        h("h3", { class: "mail-section-title", text: title }),
        countEl,
      );
      groupToggle.addEventListener("click", () => {
        if (filter !== null) return;
        if (collapsedGroups.has(cat)) collapsedGroups.delete(cat);
        else collapsedGroups.add(cat);
        drawList();
      });
      if (filter !== null) {
        groupToggle.classList.add("is-static");
      }

      const groupHead = h("div", { class: "mail-section-head mail-triage-group" }, groupToggle);
      let clearGroupBtn: HTMLButtonElement | null = null;
      if (isLowSignal) {
        clearGroupBtn = makeBulkButton(
          group,
          `Limpar grupo (${group.length})`,
          `Apagar ${group.length}? Clique de novo`,
          `Mover todas as ${group.length} mensagens de ${title} para Excluídos`,
          "mail-group-clear",
        );
        groupHead.append(clearGroupBtn);
      }

      let left = group.length;
      list.append(groupHead);

      if (isCollapsed) continue;

      for (const item of group) {
        const m = item.message;
        const row = h("div", { class: "hub-list-row mail-journal mail-triage-row" });
        rows.set(item, row);

        // A mensagem saiu da caixa (Apagar/Spam): some da lista, atualiza contador e exibe Desfazer com foco seguro.
        const gone = (where: "Excluídos" | "Spam") => () =>
          leaveRow(row, () => {
            const at = items.indexOf(item);
            items = items.filter((i) => i !== item);
            drawStats();
            const strip = undoStrip(item, at, where);
            row.replaceWith(strip);
            (strip.querySelector(".mail-undo") as HTMLButtonElement | null)?.focus();
            left -= 1;
            countEl.textContent = String(left);
            if (clearGroupBtn) {
              if (left === 0) clearGroupBtn.remove();
              else if (!clearGroupBtn.classList.contains("is-armed")) clearGroupBtn.textContent = `Limpar grupo (${left})`;
            }
            if (left === 0 && !filter) groupHead.classList.add("is-emptied");
          });

        const deletes = item.recommend === "apagar";
        const mkOpen = () => {
          const open = button("Abrir", () => {
            open.disabled = true;
            void MailBridge.openChecked(m)
              .catch(showError)
              .finally(() => (open.disabled = false));
          }, "", true);
          open.setAttribute("aria-label", `Abrir no MailCraft: ${m.subject}`);
          open.title = "Abrir no MailCraft";
          return open;
        };
        const mkDelete = (labelText: "Apagar" | "Excluir" = "Apagar") =>
          actionButton(labelText, () => MailBridge.actChecked(m, "delete"), {
            confirm: `${labelText}?`,
            removeRow: gone("Excluídos"),
            kind: "danger",
            label: `${labelText} (vai para Excluídos): ${m.subject}`,
          });

        const rowButtons = deletes ? [mkDelete("Apagar")] : [mkOpen(), mkDelete("Excluir")];

        // Menu secundário acessível ("⋯"): ações complementares sem poluir a linha em repouso.
        const menuBtn = h(
          "button",
          {
            type: "button",
            class: "hub-btn small mail-row-menu-btn",
            "aria-haspopup": "menu",
            "aria-expanded": "false",
            "aria-label": `Mais ações para ${m.subject}`,
            title: "Mais ações",
          },
          icon("dots"),
        ) as HTMLButtonElement;
        const menu = h("div", { class: "mail-row-menu", role: "menu" });
        const menuWrap = h("div", { class: "mail-row-menu-wrap" }, menuBtn);
        let onDocDown: ((ev: Event) => void) | null = null;
        let onMenuKey: ((ev: KeyboardEvent) => void) | null = null;

        const closeMenu = (restoreFocus = false) => {
          const attached = menu.isConnected ?? Boolean(menu.parentNode);
          if (!attached) return;
          menu.remove();
          row.classList.remove("is-menu-open");
          menuBtn.setAttribute("aria-expanded", "false");
          if (onDocDown) document.removeEventListener?.("pointerdown", onDocDown, true);
          if (onMenuKey) document.removeEventListener?.("keydown", onMenuKey, true);
          onDocDown = null;
          onMenuKey = null;
          if (closeOpenMenu === closeMenu) closeOpenMenu = null;
          if (restoreFocus) menuBtn.focus();
        };

        const addMenuItem = (
          labelText: string,
          run: () => void | Promise<void>,
          tone: "normal" | "danger" = "normal",
        ) => {
          const itemBtn = h("button", {
            type: "button",
            role: "menuitem",
            class: tone === "danger" ? "mail-row-menu-item danger" : "mail-row-menu-item",
            text: labelText,
          }) as HTMLButtonElement;
          itemBtn.addEventListener("click", () => {
            closeMenu(false);
            void run();
          });
          menu.append(itemBtn);
        };

        const populateMenu = () => {
          clear(menu);
          if (opts.onReply) {
            addMenuItem("Responder em rascunho", () => opts.onReply?.(item));
          }
          if (deletes) {
            addMenuItem("Abrir no MailCraft", () => MailBridge.openChecked(m).catch(showError));
          }
          if (m.unread) {
            addMenuItem("Marcar como lida", () =>
              MailBridge.actChecked(m, "read")
                .then(() => {
                  m.unread = false;
                  status.className = "mail-status-line ok";
                  status.textContent = "Marcada como lida.";
                })
                .catch(showError),
            );
          }
          if (!m.flagged) {
            addMenuItem("Marcar como importante", () =>
              MailBridge.actChecked(m, "flag")
                .then(() => {
                  m.flagged = true;
                  status.className = "mail-status-line ok";
                  status.textContent = "Marcada como importante.";
                })
                .catch(showError),
            );
          }
          if (["promocao", "spam", "outros", "notificacao"].includes(cat)) {
            addMenuItem("Mover para Spam", () =>
              MailBridge.actChecked(m, "junk")
                .then(() => gone("Spam")())
                .catch(showError),
            );
          }
        };

        menuBtn.addEventListener("click", (e) => {
          e.stopPropagation();
          if (menu.isConnected ?? Boolean(menu.parentNode)) {
            closeMenu(false);
            return;
          }
          closeOpenMenu?.();
          populateMenu();
          menuWrap.append(menu);
          row.classList.add("is-menu-open");
          menuBtn.setAttribute("aria-expanded", "true");
          closeOpenMenu = () => closeMenu(false);
          onDocDown = (ev: Event) => {
            if (!menuWrap.contains(ev.target as Node)) closeMenu(false);
          };
          onMenuKey = (ev: KeyboardEvent) => {
            if (ev.key === "Escape") {
              ev.preventDefault();
              closeMenu(true);
            }
          };
          document.addEventListener("pointerdown", onDocDown, true);
          document.addEventListener("keydown", onMenuKey, true);
          (menu.querySelector(".mail-row-menu-item") as HTMLButtonElement | null)?.focus();
        });

        const actions = h("div", { class: "mail-triage-actions" }, ...rowButtons, menuWrap);
        const badge = item.recommend === "ler" ? h("span", { class: "mail-rule-outcome accent", text: "Ler" }) : null;
        row.style.setProperty("--i", String(Math.min(rowIdx++, 8)));
        row.append(
          h(
            "div",
            { class: "grow mail-triage-text" },
            h("div", { class: "mail-triage-head" }, h("b", { class: "mail-triage-sender", text: m.senderName || m.address, title: m.address }), badge),
            h("div", { class: "mail-triage-subject", text: m.subject, title: m.subject }),
            h("div", { class: "mail-dim small mail-triage-reason", text: item.reason, title: item.reason }),
          ),
          actions,
        );
        list.append(row);
      }
    }
  };

  let running = false;
  const runBtn = button("Fazer triagem", () => {
    if (running) return;
    running = true;
    runBtn.disabled = true;
    runBtn.setAttribute("aria-busy", "true");
    runBtn.textContent = "Analisando caixa…";
    status.className = "mail-status-line";
    status.textContent = "Lendo e classificando todas as mensagens da caixa de entrada…";
    void MailBridge.classify(200)
      .then((res) => {
        items = res.items.slice();
        setFilter(null);
        const capNote =
          res.totalReported && res.totalReported > res.items.length
            ? ` (${res.items.length} de ${res.totalReported} na caixa — limite de 200)`
            : "";
        status.className = res.note ? "mail-status-line err" : "mail-status-line ok";
        status.textContent = res.note ? `${res.note}${capNote}` : `${res.items.length} mensagens classificadas.${capNote}`;
      })
      .catch(showError)
      .finally(() => {
        running = false;
        runBtn.disabled = false;
        runBtn.removeAttribute("aria-busy");
        runBtn.textContent = "Fazer triagem";
      });
  }, "primary");

  setFilter(null);
  const triageCard = card(
    h(
      "div",
      { class: "mail-section-head" },
      h(
        "div",
        { class: "mail-section-title-col" },
        h("h2", { class: "mail-section-title", text: "Triagem do MailCraft" }),
        intro,
      ),
      h("div", { class: "mail-section-actions" }, runBtn),
    ),
    status,
    stats,
    filterNote,
    list,
  );
  triageCard.classList.add("mail-card");
  triageCard.dataset.anchor = "mail.autosort";
  return triageCard;
}

// ── Visão geral (MailCraft + Triagem secundária) ─────────────────────────────

export async function mailOverviewPage(page: HTMLElement, ctx: HubContext) {
  page.classList.add("mail-page");
  const [view, cfgRaw] = await Promise.all([load(), MailBridge.configGet()]);
  let cfg: MailCraftConfig = cfgRaw ?? {
    backend: "mailcraft",
    mode: "control",
    host: "127.0.0.1",
    port: 7991,
    mcpCommand: "",
  };
  page.append(
    ctx.hero(
      "mailOverview",
      "E-mail pessoal",
      "MailCraft como provedor principal (triagem completa, resumo e rascunhos); Outlook clássico como opção secundária.",
      [],
      true,
    ),
  );
  const redraw = () => ctx.navigate("mailOverview");

  const persistCfg = async (next: MailCraftConfig) => {
    cfg = (await MailBridge.configSet(next)) ?? next;
  };

  // ── Barra compacta no topo: Conexão e provedor ─────────────────────────────
  const connBadge = h("span", { class: "mail-conn-pill" });
  const statusReadout = h("div", { class: "mail-status-readout", role: "status", "aria-live": "polite" });
  setConnectionReadout(
    connBadge,
    statusReadout,
    "idle",
    "Não verificado",
    "Validando a conexão local com o MailCraft.",
  );

  const portInput = input("7991", undefined, "Porta local de controle do MailCraft");
  portInput.classList.add("mail-port-input");
  portInput.type = "number";
  portInput.min = "1";
  portInput.max = "65535";
  portInput.value = String(cfg.port);

  const mcpInput = input("Ex.: mailcraft-cli mcp --connect 127.0.0.1:7991", undefined, "Comando do servidor MCP");
  mcpInput.value = cfg.mcpCommand;
  const mcpField = h(
    "label",
    { class: cfg.mode === "mcp" ? "mail-mcp-field" : "mail-mcp-field is-hidden" },
    h("span", { class: "mail-field-label", text: "Comando MCP (opcional)" }),
    mcpInput,
  );

  const advEndpointSummary = h("span", {
    class: "mail-advanced-endpoint",
    text: cfg.mode === "mcp" ? "MCP local" : `${cfg.host}:${cfg.port}`,
  });

  const savePortOrMcp = () => {
    const portNum = Math.max(1, Math.min(65535, Number.parseInt(portInput.value, 10) || 7991));
    portInput.value = String(portNum);
    const mcpCmd = mcpInput.value.trim();
    if (portNum === cfg.port && mcpCmd === cfg.mcpCommand) return;
    advEndpointSummary.textContent = cfg.mode === "mcp" ? "MCP local" : `${cfg.host}:${portNum}`;
    void persistCfg({ ...cfg, port: portNum, mcpCommand: mcpCmd });
  };
  portInput.addEventListener("change", savePortOrMcp);
  portInput.addEventListener("blur", savePortOrMcp);
  mcpInput.addEventListener("change", savePortOrMcp);
  mcpInput.addEventListener("blur", savePortOrMcp);

  const backendSel = select<MailBackend>(
    [
      ["mailcraft", "MailCraft (principal)"],
      ["outlook", "Outlook clássico (secundário)"],
    ],
    cfg.backend,
    (b) => {
      void persistCfg({ ...cfg, backend: b }).then(redraw);
    },
  );
  backendSel.classList.add("mail-config-select");
  backendSel.setAttribute("aria-label", "Provedor ativo de e-mail");

  const modeSel = select<MailCraftMode>(
    [
      ["control", "Canal local (recomendado)"],
      ["mcp", "MCP local (avançado)"],
    ],
    cfg.mode,
    (m) => {
      mcpField.classList.toggle("is-hidden", m !== "mcp");
      advEndpointSummary.textContent = m === "mcp" ? "MCP local" : `${cfg.host}:${cfg.port}`;
      void persistCfg({ ...cfg, mode: m });
    },
  );
  modeSel.classList.add("mail-config-select");
  modeSel.setAttribute("aria-label", "Transporte local do MailCraft");

  let testingConn = false;
  const renderConnectionResult = (st: MailCraftStatus | null) => {
    if (!st) {
      setConnectionReadout(
        connBadge,
        statusReadout,
        "idle",
        "Desktop apenas",
        "Teste disponível apenas no aplicativo desktop.",
      );
      return;
    }
    if (st.connected) {
      setConnectionReadout(
        connBadge,
        statusReadout,
        "ok",
        "Conectado",
        `${st.accounts} conta(s), caixa ${st.mailboxState} · leitura e rascunho`,
        st.endpoint,
      );
    } else {
      setConnectionReadout(
        connBadge,
        statusReadout,
        "err",
        "Offline",
        st.error ?? `Sem resposta em ${st.endpoint}.`,
      );
    }
  };

  const runConnTest = (persistFirst: boolean) => {
    if (testingConn) return;
    testingConn = true;
    testBtn.disabled = true;
    testBtn.setAttribute("aria-busy", "true");
    testBtn.textContent = "Verificando…";
    const portNum = Math.max(1, Math.min(65535, Number.parseInt(portInput.value, 10) || 7991));
    const draftCfg: MailCraftConfig = { ...cfg, port: portNum, mcpCommand: mcpInput.value.trim() };
    setConnectionReadout(connBadge, statusReadout, "busy", "Verificando…", "Validando canal local com o MailCraft…");
    const step = persistFirst ? persistCfg(draftCfg).then(() => MailBridge.testConnection(draftCfg)) : MailBridge.testConnection(draftCfg);
    void step
      .then((st) => {
        if (page.isConnected === false) return;
        renderConnectionResult(st);
      })
      .catch((err) => {
        if (page.isConnected === false) return;
        setConnectionReadout(
          connBadge,
          statusReadout,
          "err",
          "Erro",
          String(err).replace(/^Error:\s*/, ""),
        );
      })
      .finally(() => {
        testingConn = false;
        if (page.isConnected === false) return;
        testBtn.disabled = false;
        testBtn.removeAttribute("aria-busy");
        testBtn.textContent = "Testar conexão";
      });
  };

  const testBtn = button("Testar conexão", () => runConnTest(true));

  let openingMailCraft = false;
  const openStatus = h("div", { class: "mail-status-line is-empty", role: "status", "aria-live": "polite" });
  const openBtn = button("Abrir MailCraft", () => {
    if (openingMailCraft) return;
    openingMailCraft = true;
    openBtn.disabled = true;
    openBtn.setAttribute("aria-busy", "true");
    openBtn.textContent = "Abrindo…";
    openStatus.className = "mail-status-line";
    openStatus.textContent = "Abrindo o MailCraft…";
    void MailBridge.openMailCraft()
      .then((msg) => {
        openStatus.className = "mail-status-line ok";
        openStatus.textContent = msg;
        void MailBridge.configGet().then((next) => {
          cfg = next ?? cfg;
        });
      })
      .catch((err) => {
        openStatus.className = "mail-status-line err";
        openStatus.textContent = String(err).replace(/^Error:\s*/, "");
      })
      .finally(() => {
        openingMailCraft = false;
        openBtn.disabled = false;
        openBtn.removeAttribute("aria-busy");
        openBtn.textContent = "Abrir MailCraft";
      });
  });

  const onMailCraft = cfg.backend !== "outlook";
  const activeRules = view.settings.rules.filter((r) => r.enabled).length;
  const quickLink = (id: PageId, text: string) =>
    h("button", { type: "button", class: "mail-quick-link", text, onclick: () => ctx.navigate(id) });

  const advancedDetails = h(
    "details",
    { class: "mail-advanced-details" },
    h(
      "summary",
      { class: "mail-advanced-summary" },
      h("span", { text: "Avançado" }),
      advEndpointSummary,
    ),
    h(
      "div",
      { class: "mail-advanced-body" },
      row("Transporte local", "Canal autenticado sem permissão de envio.", modeSel),
      h(
        "div",
        { class: "mail-port-row" },
        h("label", { class: "mail-port-field" }, h("span", { class: "mail-field-label", text: "Porta local" }), portInput),
        mcpField,
      ),
    ),
  );

  const configCard = card(
    h(
      "div",
      { class: "mail-config-bar" },
      h(
        "div",
        { class: "mail-config-title-group" },
        h("h2", { class: "mail-section-title", text: "Conexão e provedor" }),
        connBadge,
      ),
      h(
        "div",
        { class: "mail-config-controls" },
        backendSel,
        onMailCraft ? testBtn : null,
        onMailCraft ? openBtn : null,
      ),
    ),
    onMailCraft ? statusReadout : null,
    onMailCraft ? openStatus : null,
    onMailCraft ? advancedDetails : null,
    h(
      "div",
      { class: "mail-quick-links" },
      quickLink("mailVIP", `Remetentes VIP (${view.settings.vip.length})`),
      quickLink("mailBanned", `Palavras proibidas (${view.settings.banned.length})`),
      quickLink("mailRules", `Regras (${activeRules})`),
    ),
  );
  configCard.classList.add("mail-card", "mail-config-top");
  page.append(configCard);

  // ── Resumo rápido da caixa ─────────────────────────────────────────────────
  const summaryWrap = h("div", { class: "mail-summary-output is-idle" });
  const summaryMeta = h("div", {
    class: "mail-summary-meta",
    role: "status",
    "aria-live": "polite",
    text: "Ainda não gerado nesta sessão",
  });
  const summaryBody = h(
    "div",
    { class: "mail-summary-body" },
    h("p", {
      class: "mail-summary-empty-hint",
      text: "Clique em Resumir caixa agora para ver prioridades, pendências e avisos de toda a caixa de entrada sem enviar ou apagar nada.",
    }),
  );
  summaryWrap.append(summaryMeta, summaryBody);

  let summarizing = false;
  const summarizeBtn = button("Resumir caixa agora", () => {
    if (summarizing) return;
    summarizing = true;
    summarizeBtn.disabled = true;
    summarizeBtn.setAttribute("aria-busy", "true");
    summarizeBtn.textContent = "Resumindo…";
    summaryWrap.classList.remove("is-idle", "is-revealed");
    summaryMeta.className = "mail-summary-meta";
    summaryMeta.textContent = "Lendo todas as mensagens da caixa…";
    renderSummaryBody(summaryBody, "Preparando resumo com o provedor ativo.");
    void MailBridge.summarizeInbox(200)
      .then((res) => {
        summaryMeta.className = "mail-summary-meta";
        summaryMeta.textContent = `${res.count} mensagens · ${res.unread} não lidas · ${time(Date.now())} · ${res.backend}`;
        renderSummaryBody(summaryBody, res.summary, res.warnings);
        summaryWrap.classList.add("is-revealed");
      })
      .catch((err) => {
        summaryMeta.className = "mail-summary-meta err";
        summaryMeta.textContent = "Falha ao resumir caixa";
        renderSummaryBody(summaryBody, String(err).replace(/^Error:\s*/, ""));
      })
      .finally(() => {
        summarizing = false;
        summarizeBtn.disabled = false;
        summarizeBtn.removeAttribute("aria-busy");
        summarizeBtn.textContent = "Resumir caixa agora";
      });
  });
  summarizeBtn.classList.add("mail-summary-trigger");

  const inboxCard = card(
    h(
      "div",
      { class: "mail-section-head" },
      h(
        "div",
        { class: "mail-section-title-col" },
        h("h2", { class: "mail-section-title", text: "Resumo rápido da caixa" }),
        h("div", {
          class: "mail-section-sub",
          text: "Leitura sob demanda do mesmo conjunto analisado na triagem. O Lumen nunca envia e-mails daqui.",
        }),
      ),
      h("div", { class: "mail-section-actions" }, summarizeBtn),
    ),
    summaryWrap,
  );
  inboxCard.classList.add("mail-card");

  // ── Preparar rascunho (com resposta encadeada e validação clara) ───────────
  const draftTo = input("contato@exemplo.com", undefined, "Destinatário do rascunho");
  const draftSubject = input("Assunto do rascunho", undefined, "Assunto do rascunho");
  const draftBody = h("textarea", {
    class: "hub-input mail-draft-body",
    placeholder: "Escreva a mensagem — o Lumen salva apenas como rascunho para sua revisão.",
    "aria-label": "Corpo do rascunho",
    rows: "4",
  }) as HTMLTextAreaElement;
  const draftStatus = h("div", { class: "mail-status-line is-empty", role: "status", "aria-live": "polite" });

  let replyTo: { id: string; subject: string } | null = null;
  const replyChipText = h("span", { class: "mail-reply-chip-text" });
  const replyCancelBtn = h("button", { type: "button", class: "mail-filter-clear", text: "Cancelar" });
  const replyChip = h("div", { class: "mail-reply-chip is-hidden" }, replyChipText, replyCancelBtn);
  const setReplyContext = (next: { id: string; subject: string } | null) => {
    replyTo = next;
    replyChip.classList.toggle("is-hidden", !next);
    replyChipText.textContent = next ? `Em resposta a: ${next.subject}` : "";
  };
  replyCancelBtn.addEventListener("click", () => setReplyContext(null));

  const clearInvalid = (el: HTMLElement) => {
    el.classList.remove("is-invalid");
    el.removeAttribute("aria-invalid");
  };
  for (const el of [draftTo, draftSubject, draftBody]) {
    el.addEventListener("input", () => clearInvalid(el));
  }

  let savingDraft = false;
  const resetDraft = () => {
    draftTo.value = "";
    draftSubject.value = "";
    draftBody.value = "";
    setReplyContext(null);
    clearInvalid(draftTo);
    clearInvalid(draftSubject);
    clearInvalid(draftBody);
    draftStatus.className = "mail-status-line is-empty";
    draftStatus.textContent = "";
  };

  const clearDraftBtn = button("Limpar", resetDraft, "", true);

  const submitDraft = () => {
    if (savingDraft) return;
    const to = (draftTo.value ?? "").trim();
    const subject = (draftSubject.value ?? "").trim();
    const body = draftBody.value ?? "";
    const missing: [HTMLElement, string][] = [];
    if (!to) missing.push([draftTo, "o destinatário"]);
    if (!subject) missing.push([draftSubject, "o assunto"]);
    if (!body.trim()) missing.push([draftBody, "a mensagem"]);

    if (missing.length > 0) {
      for (const [el] of missing) {
        el.classList.add("is-invalid");
        el.setAttribute("aria-invalid", "true");
      }
      draftStatus.className = "mail-status-line err";
      draftStatus.setAttribute("role", "alert");
      draftStatus.textContent = `Preencha ${missing.map(([, name]) => name).join(", ")} antes de salvar o rascunho.`;
      missing[0][0].focus();
      return;
    }

    savingDraft = true;
    draftBtn.disabled = true;
    draftBtn.setAttribute("aria-busy", "true");
    draftBtn.textContent = "Salvando rascunho…";
    draftStatus.className = "mail-status-line";
    draftStatus.setAttribute("role", "status");
    draftStatus.textContent = "Gravando rascunho para revisão…";
    void MailBridge.prepareDraft({
      replyToId: replyTo?.id ?? null,
      to,
      subject,
      body,
    })
      .then((res) => {
        draftStatus.className = "mail-status-line ok";
        draftStatus.textContent = res.note;
      })
      .catch((err) => {
        draftStatus.className = "mail-status-line err";
        draftStatus.setAttribute("role", "alert");
        draftStatus.textContent = String(err).replace(/^Error:\s*/, "");
      })
      .finally(() => {
        savingDraft = false;
        draftBtn.disabled = false;
        draftBtn.removeAttribute("aria-busy");
        draftBtn.textContent = "Salvar rascunho (sem enviar)";
      });
  };
  const draftBtn = button("Salvar rascunho (sem enviar)", submitDraft);
  draftBody.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      submitDraft();
    }
  });

  const guardBadge = h(
    "span",
    { class: "mail-guard-badge" },
    icon("lock"),
    h("span", { text: "Nunca envia sozinho" }),
  );

  const draftCard = card(
    h(
      "div",
      { class: "mail-section-head" },
      h(
        "div",
        { class: "mail-section-title-col" },
        h("h2", { class: "mail-section-title", text: "Preparar rascunho" }),
        h("div", {
          class: "mail-section-sub",
          text: "Abre ou grava o rascunho no MailCraft para revisão humana antes de enviar.",
        }),
      ),
      guardBadge,
    ),
    replyChip,
    h(
      "div",
      { class: "mail-draft-meta" },
      h("label", { class: "mail-field" }, h("span", { class: "mail-field-label", text: "Para" }), draftTo),
      h("label", { class: "mail-field" }, h("span", { class: "mail-field-label", text: "Assunto" }), draftSubject),
    ),
    h("label", { class: "mail-field" }, h("span", { class: "mail-field-label", text: "Mensagem" }), draftBody),
    h(
      "div",
      { class: "mail-draft-foot" },
      h("span", { class: "mail-foot-note", text: "Nenhum envio ocorre pelo Lumen · Ctrl+Enter salva rascunho" }),
      h("div", { class: "mail-draft-foot-actions" }, clearDraftBtn, draftBtn),
    ),
    draftStatus,
  );
  draftCard.classList.add("mail-card");

  const startReplyFromTriage = (item: ClassifiedMail) => {
    const m = item.message;
    setReplyContext({ id: m.entryId, subject: m.subject });
    draftTo.value = m.address || m.senderName;
    draftSubject.value = /^re:/i.test(m.subject.trim()) ? m.subject : `Re: ${m.subject}`;
    clearInvalid(draftTo);
    clearInvalid(draftSubject);
    draftCard.scrollIntoView({ behavior: "smooth", block: "nearest" });
    draftBody.focus();
  };

  // ── Card secundário do Outlook clássico ────────────────────────────────────
  const statTile = (val: number, caption: string, dotTone: "neutral" | "warn" | "ok") =>
    h(
      "div",
      { class: "mail-stat-tile" },
      h("div", { class: "mail-stat", text: String(val) }),
      h(
        "div",
        { class: "mail-stat-label" },
        h("span", { class: `mail-stat-dot ${dotTone}`, "aria-hidden": "true" }),
        h("span", { text: caption }),
      ),
    );

  const sortCard = card(
    h("h2", { class: "mail-section-title", text: "Outlook clássico (triagem)" }),
    h("div", {
      class: "mail-section-sub",
      text: t("Coucou sorts your Outlook inboxes: VIPs first, then banned words, then your rules. It never deletes a message."),
    }),
    divider(),
    row(
      t("Sort new messages automatically"),
      t("Only messages that arrive from now on, in every Outlook account. The classic Outlook must be open."),
      toggle(view.settings.autoSort, (on) => {
        save({ ...view.settings, autoSort: on });
        view.settings.autoSort = on;
      }),
    ),
  );
  sortCard.classList.add("mail-card");
  if (!view.windows) {
    sortCard.append(divider(), h("div", { class: "mail-section-sub", text: t("Outlook is only reached on Windows.") }));
  } else if (!view.outlookOpen) {
    sortCard.append(divider(), h("div", { class: "mail-section-sub", text: t("Outlook is closed: nothing is sorted until it is open.") }));
  }
  sortCard.append(
    h(
      "div",
      { class: "mail-stats" },
      statTile(view.today.sorted, t("sorted today"), "neutral"),
      statTile(view.today.junk, t("sent to Junk"), "warn"),
      statTile(view.today.important, t("marked important"), "ok"),
    ),
  );

  // ── Montagem Master-Detail ─────────────────────────────────────────────────
  const primaryCol = h("div", { class: "mail-primary" });
  const sidebarCol = h("div", { class: onMailCraft ? "mail-sidebar mail-sticky-sidebar" : "mail-sidebar" });
  if (onMailCraft) {
    primaryCol.append(mailCraftTriageCard(view.classified ?? [], { onReply: startReplyFromTriage }));
    sidebarCol.append(inboxCard, draftCard);
    window.setTimeout(() => {
      if (page.isConnected !== false) runConnTest(false);
    }, 0);
  } else {
    sortCard.dataset.anchor = "mail.autosort";
    primaryCol.append(inboxCard, draftCard);
    sidebarCol.append(sortCard);
  }
  page.append(h("div", { class: "mail-workbench" }, primaryCol, sidebarCol));

  if (view.sent.length > 0) {
    const sentHead = h(
      "div",
      { class: "mail-journal-head" },
      label(t("Sent by Coucou")),
      h("span", { class: "mail-chips-count", text: `${view.sent.length}` }),
    );
    page.append(sentHead);
    const sentList = card();
    sentList.classList.add("mail-card");
    sentList.classList.add("mail-activity-card");
    view.sent.forEach((e, i) => {
      if (i > 0) sentList.append(divider());
      sentList.append(
        h(
          "div",
          { class: "hub-list-row mail-journal" },
          h("b", { text: e.to.join(", ") }),
          h("span", { class: "grow mail-dim", text: e.subject }),
          e.attachments.length > 0 ? h("span", { class: "mail-dim", text: `Anexo: ${e.attachments.join(", ")}` }) : h("span"),
          h("span", { class: "mail-ts", text: time(e.date, true) }),
        ),
      );
    });
    page.append(sentList);
  }

  // O diário registra só o que a triagem do Outlook fez; no MailCraft nada é movido automaticamente.
  if (onMailCraft && view.journal.length === 0) return;
  const journalHead = h("div", { class: "mail-journal-head" }, label(t("Journal")));
  if (view.journal.length > 0) {
    journalHead.append(
      button(
        t("Clear"),
        () => {
          void MailBridge.clearJournal().then(redraw);
        },
        "",
        true,
      ),
    );
  }
  page.append(journalHead);
  const journal = card();
  journal.classList.add("mail-card");
  journal.classList.add("mail-activity-card");
  if (view.journal.length === 0) {
    journal.append(
      h(
        "div",
        { class: "mail-empty-inline" },
        emptyState(t("Nothing sorted yet. What Coucou does to your messages shows up here, with an Undo."), "idle", pageAccent()),
      ),
    );
  }
  view.journal.forEach((e, i) => {
    if (i > 0) journal.append(divider());
    const undo = e.undone
      ? h("span", { class: "mail-ts", text: t("Undone") })
      : button(t("Undo"), () => void MailBridge.undo(e.id).then(redraw), "", true);
    journal.append(
      h(
        "div",
        { class: "hub-list-row mail-journal" },
        h(
          "div",
          { class: "grow" },
          h("div", {}, h("b", { text: nameOf(e.sender) }), h("span", { class: "mail-dim", text: `  ${e.subject}` })),
          h("div", { class: "mail-dim small", text: `${journalLine(e.action)} · ${reasonLine(e.reason)}` }),
        ),
        h("span", { class: "mail-ts", text: time(e.date) }),
        undo,
      ),
    );
  });
  page.append(journal);
}

// ── Regras ───────────────────────────────────────────────────────────────────

type Kind = "junk" | "important" | "read" | "move";

export async function mailRulesPage(page: HTMLElement, ctx: HubContext) {
  page.classList.add("mail-page");
  const view = await load();
  const redraw = () => ctx.navigate("mailRules");
  page.append(
    ctx.hero(
      "mailRules",
      t("Rules"),
      t("If a message matches, Coucou does the action. The first rule that matches wins; VIPs and banned words come before."),
      [],
      true,
    ),
  );
  const hint = autoSortHint(view, redraw);
  if (hint) page.append(hint);

  let field: MailField = "sender";
  let kind: Kind = "junk";
  const value = input(t("newsletter, @shop.com, invoice…"), () => add(), t("newsletter, @shop.com, invoice…"));
  value.classList.add("mail-rule-value");
  const mailbox = input(t("Mailbox name"), () => add(), t("Mailbox name"));
  mailbox.classList.add("mail-rule-mailbox");
  mailbox.style.display = "none";

  const action = (): MailAction | null => {
    switch (kind) {
      case "junk":
        return { kind: "junk" };
      case "important":
        return { kind: "important" };
      case "read":
        return { kind: "markRead" };
      case "move":
        return mailbox.value.trim() ? { kind: "move", mailbox: mailbox.value.trim() } : null;
    }
  };
  const add = () => {
    const v = value.value.trim();
    const a = action();
    if (!v) {
      value.focus();
      return;
    }
    if (!a) {
      mailbox.focus();
      return;
    }
    const rules = [
      ...view.settings.rules,
      {
        id: `${Date.now().toString(16)}-${Math.random().toString(16).slice(2, 8)}`,
        name: ruleName(field, v, a),
        enabled: true,
        conditions: [{ field, value: v }],
        action: a,
      },
    ];
    view.settings = { ...view.settings, rules };
    save(view.settings);
    value.value = "";
    mailbox.value = "";
    drawRules();
  };
  const fields = select(
    MAIL_FIELDS.map((f) => [f, fieldTitle(f)] as [MailField, string]),
    field,
    (f) => (field = f),
  );
  fields.classList.add("mail-rule-select");
  fields.setAttribute("aria-label", t("If"));

  const kinds = select<Kind>(
    [
      ["junk", ruleLine({ kind: "junk" })],
      ["important", ruleLine({ kind: "important" })],
      ["read", ruleLine({ kind: "markRead" })],
      ["move", t("Move to mailbox…")],
    ],
    kind,
    (k) => {
      kind = k;
      mailbox.style.display = k === "move" ? "" : "none";
    },
  );
  kinds.classList.add("mail-rule-select");
  kinds.setAttribute("aria-label", t("Then"));

  const builderCard = card(
    h(
      "div",
      { class: "mail-rule-grid" },
      h("b", { class: "mail-if", text: t("If") }),
      fields,
      h("span", { class: "mail-rule-connector", text: t("contains") }),
      value,
      h("b", { class: "mail-if", text: t("Then") }),
      kinds,
      h("div", { class: "mail-rule-then-tail" }, mailbox, button(t("Add"), add, "primary")),
    ),
  );
  builderCard.classList.add("mail-rule-builder");

  const list = card();
  list.classList.add("mail-card");
  list.dataset.anchor = "mail.rules";

  const drawRules = () => {
    clear(list);
    if (view.settings.rules.length === 0) {
      list.append(emptyState(t("No rules yet. Example: if the sender contains “newsletter”, move to Junk."), "idle", "#34D399"));
      return;
    }
    const activeCount = view.settings.rules.filter((r) => r.enabled).length;
    list.append(
      h(
        "div",
        { class: "mail-section-head mail-rules-head" },
        h("h2", { class: "mail-section-title", text: `Regras configuradas (${view.settings.rules.length})` }),
        h("span", { class: "mail-chips-count", text: `${activeCount} ${activeCount === 1 ? "ativa" : "ativas"}` }),
      ),
    );
    view.settings.rules.forEach((r, idx) => {
      if (idx > 0) list.append(divider());
      const cond = r.conditions[0];
      const actionTone =
        r.action.kind === "junk" ? "warn" : r.action.kind === "important" ? "accent" : "neutral";
      const rowContent = cond
        ? h(
            "div",
            { class: "mail-rule-summary grow" },
            h("span", { class: "mail-rule-field-tag", text: fieldTitle(cond.field) }),
            h("span", { class: "mail-rule-op", text: t("contains") }),
            h("code", { class: "mail-rule-term", text: cond.value }),
            h("span", { class: "mail-rule-arrow", text: "→", "aria-hidden": "true" }),
            h("span", { class: `mail-rule-outcome ${actionTone}`, text: ruleLine(r.action) }),
          )
        : h("span", { class: "grow", text: r.name });

      const delBtn = iconButton("trash", t("Delete"), () => {
        view.settings = { ...view.settings, rules: view.settings.rules.filter((x) => x.id !== r.id) };
        save(view.settings);
        drawRules();
      });
      delBtn.classList.add("mail-delete-btn");

      const rowEl = h(
        "div",
        { class: r.enabled ? "hub-list-row mail-rule-row" : "hub-list-row mail-rule-row is-paused" },
        toggle(r.enabled, (on) => {
          view.settings = { ...view.settings, rules: view.settings.rules.map((x) => (x.id === r.id ? { ...x, enabled: on } : x)) };
          save(view.settings);
          drawRules();
        }),
        rowContent,
        !r.enabled ? h("span", { class: "mail-rule-paused-tag", text: "Pausada" }) : null,
        delBtn,
      );
      list.append(rowEl);
    });
  };
  drawRules();

  const mainCol = h("div", { class: "mail-triage-main" }, builderCard, list);
  page.append(h("div", { class: "mail-triage-workbench" }, mainCol, triageAside(ctx, "mailRules", view)));
}

// ── Palavras proibidas e VIPs ────────────────────────────────────────────────

function chipsPage(
  page: HTMLElement,
  ctx: HubContext,
  view: TriageView,
  id: "mailBanned" | "mailVIP",
  key: "banned" | "vip",
  placeholder: string,
  empty: string,
  color: string,
  asideTitle: string,
  asideTips: string[],
) {
  const hint = autoSortHint(view, () => ctx.navigate(id));
  if (hint) page.append(hint);
  const countEl = h("span", { class: "mail-chips-count" });
  const chips = h("div", { class: "hub-chips mail-token-list" });
  const feedback = h("div", { class: "mail-status-line is-empty", role: "status", "aria-live": "polite" });
  const field = input(placeholder, () => add(), placeholder);
  field.classList.add("mail-chips-input");
  field.addEventListener("input", () => {
    if (!feedback.classList.contains("is-empty")) {
      feedback.className = "mail-status-line is-empty";
      feedback.textContent = "";
    }
  });

  const add = () => {
    const w = field.value.trim();
    const list = view.settings[key];
    if (!w) {
      field.focus();
      return;
    }
    if (list.some((x) => x.toLowerCase() === w.toLowerCase())) {
      feedback.className = "mail-status-line";
      feedback.textContent = `“${w}” já está na lista.`;
      field.focus();
      return;
    }
    view.settings = { ...view.settings, [key]: [...list, w] };
    save(view.settings);
    field.value = "";
    feedback.className = "mail-status-line is-empty";
    feedback.textContent = "";
    draw();
  };
  const draw = () => {
    clear(chips);
    const list = view.settings[key];
    countEl.textContent = `${list.length} ${list.length === 1 ? "item" : "itens"}`;
    if (list.length === 0) {
      chips.append(emptyState(empty, "idle", color));
      return;
    }
    const dotTone = key === "vip" ? "accent" : "warn";
    for (const w of list) {
      const remove = h("button", {
        type: "button",
        title: `${t("Delete")} ${w}`,
        "aria-label": `${t("Delete")} ${w}`,
      });
      remove.append(icon("x"));
      remove.addEventListener("click", () => {
        view.settings = { ...view.settings, [key]: view.settings[key].filter((x) => x !== w) };
        save(view.settings);
        draw();
      });
      const textCls = w.includes("@") ? "mail-token-text mono" : "mail-token-text";
      chips.append(
        h(
          "span",
          { class: "hub-chip mail-token-chip" },
          h("span", { class: `mail-token-dot ${dotTone}`, "aria-hidden": "true" }),
          h("span", { class: textCls, text: w }),
          remove,
        ),
      );
    }
  };

  const chipsCard = card(
    h(
      "div",
      { class: "mail-chips-head" },
      field,
      countEl,
      button(t("Add"), add, "primary"),
    ),
    feedback,
    divider(),
    chips,
  );
  chipsCard.classList.add("mail-chips-card");
  chipsCard.dataset.anchor = id === "mailBanned" ? "mail.banned" : "mail.vip";

  const tipsCard = card(
    h("h2", { class: "mail-section-title", text: asideTitle }),
    ...asideTips.map((tip) => h("p", { class: "mail-aside-tip", text: tip })),
  );
  tipsCard.classList.add("mail-card");
  tipsCard.classList.add("mail-aside-card");

  const mainCol = h("div", { class: "mail-triage-main" }, chipsCard);
  const sideCol = h("div", { class: "mail-triage-side" }, triageAside(ctx, id, view), tipsCard);
  page.append(h("div", { class: "mail-triage-workbench" }, mainCol, sideCol));
  draw();
}

export async function mailBannedPage(page: HTMLElement, ctx: HubContext) {
  page.classList.add("mail-page");
  const view = await load();
  page.append(
    ctx.hero(
      "mailBanned",
      t("Banned words"),
      t("A message whose sender, subject or text holds one of these words or addresses goes straight to Junk. Whole words only: “sale” does not catch “Salesforce”."),
      [],
      true,
    ),
  );
  chipsPage(
    page,
    ctx,
    view,
    "mailBanned",
    "banned",
    t("A word, a phrase or an address"),
    t("No banned words. Add a word, or click “Always ignore this sender” on a message in the island."),
    LUMEN_WINDOWS ? "#FF5A4E" : "#FF453A",
    "Como o bloqueio combina termos",
    [
      "Palavras inteiras apenas: “promo” nunca bloqueia “promoção”.",
      "Você também pode adicionar endereços completos ou usar “Sempre ignorar este remetente” pela Ilha.",
    ],
  );
}

export async function mailVIPPage(page: HTMLElement, ctx: HubContext) {
  page.classList.add("mail-page");
  const view = await load();
  page.append(
    ctx.hero(
      "mailVIP",
      t("VIP"),
      t("Messages from these addresses or domains are always flagged important, and no rule ever moves them."),
      [],
      true,
    ),
  );
  chipsPage(
    page,
    ctx,
    view,
    "mailVIP",
    "vip",
    t("paul@client.com or @client.com"),
    t("No VIPs yet. Add an address, or a whole domain with @."),
    LUMEN_WINDOWS ? "#E99A3D" : "#F5A524",
    "Dicas de remetentes VIP",
    [
      "Use um endereço exato (ex.: ana@cliente.exemplo) para contatos individuais.",
      "Use @dominio.com para proteger toda a empresa ou equipe acima de qualquer regra.",
    ],
  );
}
