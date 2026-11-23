// The Mail card (classic Outlook) — a DOM port of MailCardView / MailRow / MailDetailView
// (MailViews.swift). Sizes, colours and wording are the Mac's.
//
// The list: the newest messages of every inbox, unread ones tinted, a flag on the important
// ones; a search (free words, from:, subject:) and quick filters (All, Unread, Important,
// Today); Load more. A click opens the message: its sender, subject and the start of its text,
// with Open in Outlook, Reply (Outlook's own window: nothing is sent from Coucou), Mark as read,
// Important, Junk, and in ⋯ Always important (VIP) and Always ignore this sender.
//
// Built once and synced in place: the search field keeps its text and the keyboard.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { effectiveMailUnread } from "../core/compact";
import {
  MAIL_BLUE, MAIL_FILTERS, MAIL_ID, MailBridge, filterTitle, messageId, senderName,
  type MailCardData, type MailFilter, type MailMessage,
} from "../core/mail";
import { timeAgo } from "./integrations";
import { language, t, tl } from "../i18n/i18n";

export interface MailCardHost {
  /** The island takes the keyboard for the search field, and gives it back. */
  keyboard(on: boolean): void;
}

export interface MailCard {
  el: HTMLElement;
  sync(): void;
}

function data(): MailCardData {
  const d = (State.integrations[MAIL_ID]?.data ?? {}) as Partial<MailCardData>;
  return {
    running: d.running ?? false,
    windows: d.windows ?? true,
    backend: d.backend ?? "mailcraft",
    mode: d.mode ?? "control",
    unread: d.unread ?? 0,
    messages: Array.isArray(d.messages) ? d.messages : [],
    shown: d.shown ?? 12,
    maxShown: d.maxShown ?? 60,
    error: d.error ?? null,
  };
}

export function buildMailCard(host: MailCardHost): MailCard {
  const unreadTag = h("span", { class: "mail-unread" });
  const field = h("input", { class: "mail-search", type: "text", spellcheck: "false", placeholder: t("from:anne invoice") }) as HTMLInputElement;
  const closeSearch = h("button", { class: "mail-icon", title: tl("Close") }, svg(ICONS.xmark, 9));
  const searchBtn = h("button", { class: "mail-icon", title: tl("Search") }, svg(ICONS.magnifier, 10));
  const head = h("div", { class: "int-head mail-head" }, dot(MAIL_BLUE, 7), h("b", { text: "Outlook" }));
  const toolbar = h("div", { class: "mail-toolbar" });
  const filters = h("div", { class: "mail-filters" });
  const listBody = h("div", { class: "mail-rows" });
  const listView = h("div", { class: "mail-list" }, head, toolbar, filters, listBody);
  const detailView = h("div", { class: "mail-detail" });
  const el = h("div", { class: "int-card mail-card" }, listView, detailView);

  let searchOpen = false;
  let filter: MailFilter = "all";
  let results: MailMessage[] | null = null;
  let searching = false;
  let token = 0;
  let open: MailMessage | null = null;
  const previews = new Map<string, { text: string; attachments: string[] } | null>();
  let listKey = "";
  let refreshed = 0;

  const displayed = (): MailMessage[] => results ?? data().messages;

  async function runSearch() {
    const q = field.value.trim();
    if (!q && filter === "all") {
      results = null;
      searching = false;
      listKey = "";
      sync();
      return;
    }
    const mine = ++token;
    searching = true;
    listKey = "";
    sync();
    const found = await MailBridge.search(q, filter);
    if (mine !== token) return;
    searching = false;
    results = found ?? [];
    listKey = "";
    sync();
  }

  function setSearch(on: boolean) {
    searchOpen = on;
    if (on) {
      host.keyboard(true);
      window.setTimeout(() => field.focus(), 80);
    } else {
      field.value = "";
      filter = "all";
      results = null;
      token++;
      searching = false;
      host.keyboard(false);
    }
    listKey = "";
    sync();
  }

  searchBtn.addEventListener("click", () => setSearch(true));
  closeSearch.addEventListener("click", () => setSearch(false));
  field.addEventListener("keydown", (e) => {
    const key = (e as KeyboardEvent).key;
    if (key === "Enter") {
      e.preventDefault();
      void runSearch();
    } else if (key === "Escape") {
      e.preventDefault();
      setSearch(false);
    }
    e.stopPropagation();
  });

  function openMessage(m: MailMessage) {
    open = m;
    const id = messageId(m);
    if (!previews.has(id)) {
      previews.set(id, null);
      void MailBridge.detail(m).then((d) => {
        if (d) previews.set(id, d);
        else previews.delete(id);
        if (open && messageId(open) === id) drawDetail();
      });
    }
    drawDetail();
    sync();
  }

  function closeMessage() {
    open = null;
    listKey = "";
    sync();
  }

  async function act(m: MailMessage, kind: "read" | "flag" | "unflag" | "junk" | "ignore" | "vip") {
    // Shown at once; Outlook catches up.
    const id = messageId(m);
    const change = (list: MailMessage[] | null) =>
      list?.flatMap((x) => {
        if (messageId(x) !== id) return [x];
        if (kind === "junk" || kind === "ignore") return [];
        return [{ ...x, unread: kind === "read" ? false : x.unread, flagged: kind === "flag" || kind === "vip" ? true : kind === "unflag" ? false : x.flagged }];
      }) ?? null;
    results = change(results);
    if (open && messageId(open) === id) {
      open = kind === "junk" || kind === "ignore" ? null : change([open])?.[0] ?? null;
    }
    listKey = "";
    sync();
    await MailBridge.act(m, kind);
  }

  function row(m: MailMessage): HTMLElement {
    const r = h("button", { class: m.unread ? "int-row mail-row unread" : "int-row mail-row", title: m.subject },
      h("i", { class: m.unread ? "mail-dot on" : "mail-dot" }),
      h("span", { class: "mail-row-copy" },
        h("span", { class: "int-name mail-name", text: senderName(m) }),
        h("span", { class: "int-sub mail-subject", text: m.subject }),
      ),
    );
    if (m.flagged) r.append(h("span", { class: "mail-flag", text: "⚑" }));
    r.append(h("span", { class: "int-ago", text: timeAgo(m.date) }));
    r.addEventListener("click", () => openMessage(m));
    return r;
  }

  function syncList() {
    const d = data();
    const isMailCraft = (d.backend ?? "mailcraft") === "mailcraft";
    const badgeUnread = effectiveMailUnread(d.unread, State.mailUnreadDismissedAt);
    const list = displayed();
    const key = [
      language(), d.running, d.windows, d.backend, d.unread, badgeUnread, State.mailUnreadDismissedAt ?? "",
      d.error ?? "", searchOpen, filter, searching, results === null, d.shown,
      Math.floor(Date.now() / 60_000),
      ...list.map((m) => `${messageId(m)}|${m.unread}|${m.flagged}`),
    ].join("~");
    if (key === listKey) return;
    listKey = key;

    // The head: the unread count and the magnifier, or the search field.
    clear(head);
    clear(toolbar);
    head.append(dot(MAIL_BLUE, 7), h("b", { text: isMailCraft ? "MailCraft" : "Outlook (secundário)" }));
    if (searchOpen) {
      head.append(field);
      if (searching) head.append(h("span", { class: "mail-spinner" }));
      head.append(closeSearch);
    } else {
      head.append(h("span", { class: "mail-inbox-label", text: t("Inbox") }));
      // Mostra a contagem de avisos pendentes no notch + botão Limpar para silenciar o acumulado sem alterar a caixa.
      unreadTag.textContent = badgeUnread > 9999 ? "9999+" : String(badgeUnread);
      unreadTag.title = `${badgeUnread} não lidos no notch (caixa: ${d.unread})`;
      unreadTag.style.display = badgeUnread > 0 ? "" : "none";
      const clearBtn = badgeUnread > 0
        ? h(
            "button",
            {
              class: "mail-clear-btn",
              title: "Limpar notificação de não lidos do notch",
              "aria-label": "Limpar notificação de não lidos do notch",
              onclick: () => State.dismissMailUnread(d.unread),
            },
            svg(ICONS.bellSlash, 10, { stroke: 2 }),
            h("span", { text: "Limpar" }),
          )
        : null;
      const summarizeBtn = h("button", {
        class: "link-btn accent",
        title: "Resumir caixa de entrada sem enviar nada",
        text: "Resumir",
        onclick: () => {
          summarizeBtn.disabled = true;
          summarizeBtn.textContent = "Resumindo…";
          void MailBridge.summarizeInbox(10)
            .then((res) => {
              summarizeBtn.textContent = "Resumir";
              summarizeBtn.disabled = false;
              clear(listBody);
              listBody.append(
                h("div", { class: "mail-summary-box" },
                  h("b", { text: `Resumo da caixa (${res.count} lidas / ${res.unread} não lidas)` }),
                  h("div", { class: "mail-detail-text", text: res.summary }),
                  h("button", { class: "link-btn", text: "Voltar à lista", onclick: () => { listKey = ""; syncList(); } }),
                ),
              );
            })
            .catch((err) => {
              summarizeBtn.textContent = "Resumir";
              summarizeBtn.disabled = false;
              clear(listBody);
              listBody.append(h("div", { class: "mail-empty", text: String(err) }));
            });
        },
      }) as HTMLButtonElement;
      const draftBtn = h("button", {
        class: "link-btn",
        title: "Preparar rascunho sem enviar",
        text: "Rascunho",
        onclick: () => {
          const toInput = h("input", { class: "mail-search-input", placeholder: "Para (ex.: pessoa@exemplo.com)" }) as HTMLInputElement;
          const subjInput = h("input", { class: "mail-search-input", placeholder: "Assunto" }) as HTMLInputElement;
          const bodyInput = h("textarea", { class: "mail-search-input", placeholder: "Texto do rascunho (nunca envia sozinho)", rows: "3" }) as HTMLTextAreaElement;
          const noteEl = h("div", { class: "mail-empty", text: "" });
          const saveBtn = h("button", {
            class: "link-btn accent",
            text: "Salvar rascunho (sem enviar)",
            onclick: () => {
              saveBtn.disabled = true;
              void MailBridge.prepareDraft({ to: toInput.value, subject: subjInput.value, body: bodyInput.value })
                .then((res) => {
                  noteEl.textContent = res.note;
                  saveBtn.disabled = false;
                })
                .catch((err) => {
                  noteEl.textContent = String(err);
                  saveBtn.disabled = false;
                });
            },
          }) as HTMLButtonElement;
          host.keyboard(true);
          clear(listBody);
          listBody.append(
            h("div", { class: "mail-draft-box" },
              toInput,
              subjInput,
              bodyInput,
              h("div", { class: "mail-actions" },
                saveBtn,
                h("button", { class: "link-btn dim", text: "Cancelar", onclick: () => { host.keyboard(false); listKey = ""; syncList(); } }),
              ),
              noteEl,
            ),
          );
        },
      }) as HTMLButtonElement;
      head.append(unreadTag);
      if (clearBtn) head.append(clearBtn);
      toolbar.append(summarizeBtn, draftBtn);
      head.append(h("span", { class: "mail-spacer" }));
      if (searching) head.append(h("span", { class: "mail-spinner" }));
      head.append(searchBtn);
    }

    clear(filters);
    for (const f of MAIL_FILTERS) {
      filters.append(h("button", {
        class: f === filter ? "mail-chip on" : "mail-chip",
        text: filterTitle(f),
        onclick: () => {
          filter = f;
          void runSearch();
        },
      }));
    }
    filters.style.display = d.running ? "" : "none";

    clear(listBody);
    if (!isMailCraft && !d.windows) {
      listBody.append(h("div", { class: "mail-empty", text: t("Outlook is only reached on Windows.") }));
      return;
    }
    if (!d.running) {
      const msg = isMailCraft
        ? (d.error?.trim() || "Conecte o MailCraft (--control 7991 ou MCP local) em Central → E-mail.")
        : t("Open Outlook (the classic one) to see your mail here.");
      listBody.append(h("div", { class: "mail-empty", text: msg }));
      return;
    }
    if (list.length === 0) {
      listBody.append(h("div", { class: "mail-empty", text: results !== null ? t("Nothing found") : t("No messages") }));
      return;
    }
    for (const m of list) listBody.append(row(m));
    if (results === null && d.messages.length >= d.shown && d.shown < d.maxShown) {
      listBody.append(h("button", { class: "link-btn mail-more", text: t("Load more"), onclick: () => void MailBridge.more() }));
    }
  }

  let detailDismissedKey: number | null = State.mailUnreadDismissedAt;

  function drawDetail() {
    clear(detailView);
    const m = open;
    if (!m) return;
    const d = data();
    const badgeUnread = effectiveMailUnread(d.unread, State.mailUnreadDismissedAt);
    detailDismissedKey = State.mailUnreadDismissedAt;
    const back = h("button", { class: "int-back mail-back", title: tl("Back") }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 }));
    back.addEventListener("click", closeMessage);
    const preview = previews.get(messageId(m));
    const text = preview === null ? t("Loading…")
      : preview === undefined ? "Não foi possível carregar o e-mail. Volte à lista e tente de novo."
      : preview.text.trim() || " ";
    const isMc = m.storeId === "mailcraft";
    const actions = h("div", { class: "mail-actions" });
    const openError = h("span", { class: "mail-empty", role: "status" });
    actions.append(h("button", {
      class: "link-btn accent",
      text: isMc ? "Abrir no MailCraft" : t("Open in Outlook"),
      onclick: () => {
        openError.remove();
        void MailBridge.openChecked(m).catch(() => {
          openError.textContent = "Não foi possível abrir a mensagem no aplicativo.";
          actions.append(openError);
        });
      },
    }));
    actions.append(
      h("button", {
        class: isMc ? "link-btn accent" : "link-btn",
        text: isMc ? "Rascunho (sem enviar)" : t("Reply"),
        title: isMc ? "Preparar rascunho de resposta (sem enviar)" : t("Reply"),
        onclick: () => void MailBridge.open(m, true),
      }),
    );
    if (m.unread) actions.append(h("button", { class: "link-btn dim", text: t("Mark as read"), onclick: () => void act(m, "read") }));
    actions.append(
      h("button", {
        class: "link-btn amber",
        text: m.flagged ? t("Not important") : t("Important"),
        onclick: () => void act(m, m.flagged ? "unflag" : "flag"),
      }),
    );
    if (!isMc) {
      actions.append(h("button", { class: "link-btn dim", text: t("Junk"), onclick: () => void act(m, "junk") }));
      // ⋯: Always important (VIP), Always ignore this sender.
      const more = h("div", { class: "mail-more-menu" });
      const dots = h("button", { class: "link-btn dim mail-dots", text: "⋯" });
      dots.addEventListener("click", () => more.classList.toggle("on"));
      more.append(
        h("button", { text: t("Always important (VIP)"), onclick: () => void act(m, "vip") }),
        h("button", { text: t("Always ignore this sender"), onclick: () => void act(m, "ignore") }),
      );
      actions.append(h("span", { class: "mail-dots-wrap" }, dots, more));
    }
    const clearDetailBtn = badgeUnread > 0
      ? h(
          "button",
          {
            class: "mail-clear-btn detail",
            title: "Limpar notificação de não lidos do notch",
            "aria-label": "Limpar notificação de não lidos do notch",
            onclick: () => {
              State.dismissMailUnread(d.unread);
              drawDetail();
            },
          },
          svg(ICONS.bellSlash, 9.5, { stroke: 2 }),
          h("span", { text: `Limpar ${badgeUnread > 999 ? "999+" : badgeUnread}` }),
        )
      : null;
    const headChildren: Node[] = [
      back,
      h("i", { class: m.unread ? "mail-dot on" : "mail-dot" }),
      h("b", { class: "mail-from", text: senderName(m), title: m.address }),
      h("span", { class: "mail-spacer" }),
    ];
    if (clearDetailBtn) headChildren.push(clearDetailBtn);
    headChildren.push(h("span", { class: "int-ago", text: timeAgo(m.date) }));
    detailView.append(
      h("div", { class: "mail-detail-head" }, ...headChildren),
      h("div", { class: "mail-detail-subject", text: m.subject || " " }),
      h("div", { class: "mail-detail-text", text }),
      actions,
    );
  }

  function sync() {
    el.classList.toggle("detail-open", open !== null);
    if (open) {
      // The open message, kept up to date when a refresh changes it.
      const fresh = displayed().find((x) => messageId(x) === messageId(open!));
      if (
        (fresh && (fresh.unread !== open.unread || fresh.flagged !== open.flagged)) ||
        detailDismissedKey !== State.mailUnreadDismissedAt
      ) {
        if (fresh) open = fresh;
        drawDetail();
      }
    }
    syncList();
    // Opening the card reads Outlook again (at most every 15 s).
    if (State.focusTask?.id === MAIL_ID && State.mode === "expanded" && Date.now() - refreshed > 15_000) {
      refreshed = Date.now();
      void MailBridge.refresh();
    }
  }

  // Leaving the card gives the keyboard back.
  State.subscribe(() => {
    if (!searchOpen) return;
    if (State.mode !== "expanded" || State.view !== "overview" || State.focusTask?.id !== MAIL_ID) setSearch(false);
  });

  return { el, sync };
}
