// E-mail pessoal (MailCraft principal + Outlook clássico secundário):
// leitura, resumo da caixa e preparo de rascunhos sem envio automático, além
// de regras, palavras proibidas, VIPs e diário de triagem.

import { h, clear } from "../../views/dom";
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
  type MailAction,
  type MailBackend,
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
  if (view.settings.autoSort) return null;
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

/** Renderiza o texto retornado pelo resumo de caixa separando marcadores editoriais e parágrafos. */
function renderSummaryBody(target: HTMLElement, summary: string, warnings: string[] = []) {
  clear(target);
  const lines = summary
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const bullets: string[] = [];
  const paragraphs: string[] = [];
  for (const line of lines) {
    if (/^[•\-–]\s+/.test(line)) bullets.push(line.replace(/^[•\-–]\s+/, ""));
    else paragraphs.push(line);
  }
  for (const p of paragraphs) {
    target.append(h("p", { class: "mail-summary-prose", text: p }));
  }
  if (bullets.length > 0) {
    const ul = h("ul", { class: "mail-summary-list" });
    for (const item of bullets) ul.append(h("li", { text: item }));
    target.append(ul);
  }
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
  const statusLabel = view.settings.autoSort ? "Triagem automática ligada" : "Triagem automática desligada";
  const statusTone = view.settings.autoSort ? "mail-aside-state ok" : "mail-aside-state";

  const guide = card(
    h(
      "div",
      { class: "mail-section-head" },
      h("h2", { class: "mail-section-title", text: "Ordem de prioridade" }),
    ),
    h("div", {
      class: "mail-section-sub",
      text: "Ao chegar um e-mail no Outlook clássico, o Lumen avalia nesta sequência sem apagar mensagens:",
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
      "MailCraft como provedor principal (leitura, resumo e rascunhos); Outlook clássico para triagem local.",
      [],
      true,
    ),
  );
  const redraw = () => ctx.navigate("mailOverview");

  const persistCfg = async (next: MailCraftConfig) => {
    cfg = (await MailBridge.configSet(next)) ?? next;
  };

  // ── Coluna principal: Resumo & Rascunho seguro ─────────────────────────────
  const summaryWrap = h("div", { class: "mail-summary-output is-empty", role: "status", "aria-live": "polite" });
  const summaryMeta = h("div", { class: "mail-summary-meta" });
  const summaryBody = h("div", { class: "mail-summary-body" });
  summaryWrap.append(summaryMeta, summaryBody);

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
        // Relê a config: o comando liga o canal de controle e pode ter gravado o provedor.
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

  let summarizing = false;
  const summarizeBtn = button("Resumir caixa agora", () => {
    if (summarizing) return;
    summarizing = true;
    summarizeBtn.disabled = true;
    summarizeBtn.setAttribute("aria-busy", "true");
    summarizeBtn.textContent = "Resumindo…";
    summaryWrap.classList.remove("is-empty");
    summaryMeta.className = "mail-summary-meta";
    summaryMeta.textContent = "Lendo mensagens recentes…";
    renderSummaryBody(summaryBody, "Preparando resumo com o provedor ativo.");
    void MailBridge.summarizeInbox(12)
      .then((res) => {
        summaryMeta.className = "mail-summary-meta";
        summaryMeta.textContent = `${res.count} mensagens · ${res.unread} não lidas · ${res.backend}`;
        renderSummaryBody(summaryBody, res.summary, res.warnings);
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
  }, "primary");

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
          text: "Leitura sob demanda das mensagens recentes. O Lumen nunca altera nem envia e-mails daqui.",
        }),
      ),
      h("div", { class: "mail-section-actions" }, openBtn, summarizeBtn, openStatus),
    ),
    summaryWrap,
  );
  inboxCard.classList.add("mail-card");

  const draftTo = input("contato@exemplo.com", undefined, "Destinatário do rascunho");
  const draftSubject = input("Assunto do rascunho", undefined, "Assunto do rascunho");
  const draftBody = h("textarea", {
    class: "hub-input mail-draft-body",
    placeholder: "Escreva a mensagem — o Lumen salva apenas como rascunho para sua revisão.",
    "aria-label": "Corpo do rascunho",
    rows: "4",
  }) as HTMLTextAreaElement;
  const draftStatus = h("div", { class: "mail-status-line is-empty", role: "status", "aria-live": "polite" });

  let savingDraft = false;
  const submitDraft = () => {
    if (savingDraft) return;
    savingDraft = true;
    draftBtn.disabled = true;
    draftBtn.setAttribute("aria-busy", "true");
    draftBtn.textContent = "Salvando rascunho…";
    draftStatus.className = "mail-status-line";
    draftStatus.setAttribute("role", "status");
    draftStatus.textContent = "Gravando rascunho para revisão…";
    void MailBridge.prepareDraft({
      to: draftTo.value.trim(),
      subject: draftSubject.value.trim(),
      body: draftBody.value,
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
      draftBtn,
    ),
    draftStatus,
  );
  draftCard.classList.add("mail-card");

  const primaryCol = h("div", { class: "mail-primary" }, inboxCard, draftCard);

  // ── Coluna lateral: Conexão, provedor e métricas Outlook ───────────────────
  const connBadge = h("span", { class: "mail-conn-pill" });
  const statusReadout = h("div", { class: "mail-status-readout", role: "status", "aria-live": "polite" });
  setConnectionReadout(
    connBadge,
    statusReadout,
    "idle",
    "Não verificado",
    "Valide a conexão local com o MailCraft quando ele estiver aberto.",
  );

  const portInput = input("7991", undefined, "Porta local de controle do MailCraft");
  portInput.classList.add("mail-port-input");
  portInput.type = "number";
  portInput.value = String(cfg.port);

  const mcpInput = input("Ex.: mailcraft-cli mcp --connect 127.0.0.1:7991", undefined, "Comando do servidor MCP");
  mcpInput.value = cfg.mcpCommand;
  const mcpField = h(
    "label",
    { class: cfg.mode === "mcp" ? "mail-mcp-field" : "mail-mcp-field is-hidden" },
    h("span", { class: "mail-field-label", text: "Comando MCP (opcional)" }),
    mcpInput,
  );

  const backendSel = select<MailBackend>(
    [
      ["mailcraft", "MailCraft (principal)"],
      ["outlook", "Outlook clássico (secundário)"],
    ],
    cfg.backend,
    (b) => void persistCfg({ ...cfg, backend: b }),
  );
  backendSel.classList.add("mail-config-select");
  backendSel.setAttribute("aria-label", "Provedor ativo de e-mail");

  const modeSel = select<MailCraftMode>(
    [
      ["control", "Loopback HMAC (127.0.0.1)"],
      ["mcp", "Servidor MCP stdio"],
    ],
    cfg.mode,
    (m) => {
      mcpField.classList.toggle("is-hidden", m !== "mcp");
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
        `${st.accounts} conta(s), caixa ${st.mailboxState} · leitura e rascunho (${st.mode})`,
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

  const testBtn = button("Testar conexão", () => {
    if (testingConn) return;
    testingConn = true;
    testBtn.disabled = true;
    testBtn.setAttribute("aria-busy", "true");
    testBtn.textContent = "Verificando…";
    const portNum = Math.max(1, Math.min(65535, Number.parseInt(portInput.value, 10) || 7991));
    const draftCfg: MailCraftConfig = { ...cfg, port: portNum, mcpCommand: mcpInput.value.trim() };
    setConnectionReadout(connBadge, statusReadout, "busy", "Verificando…", "Validando canal seguro com o MailCraft…");
    void persistCfg(draftCfg)
      .then(() => MailBridge.testConnection(draftCfg))
      .then(renderConnectionResult)
      .catch((err) => {
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
        testBtn.disabled = false;
        testBtn.removeAttribute("aria-busy");
        testBtn.textContent = "Testar conexão";
      });
  });

  const configCard = card(
    h(
      "div",
      { class: "mail-section-head" },
      h("h2", { class: "mail-section-title", text: "Conexão e provedor" }),
      connBadge,
    ),
    row("Provedor ativo", "MailCraft padrão ou Outlook clássico.", backendSel),
    divider(),
    row("Transporte local", "Canal autenticado sem permissão de envio.", modeSel),
    divider(),
    h(
      "div",
      { class: "mail-port-row" },
      h("label", { class: "mail-port-field" }, h("span", { class: "mail-field-label", text: "Porta local" }), portInput),
      testBtn,
    ),
    mcpField,
    statusReadout,
  );
  configCard.classList.add("mail-card");

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
  sortCard.dataset.anchor = "mail.autosort";

  const sidebarCol = h("div", { class: "mail-sidebar" }, configCard, sortCard);
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
