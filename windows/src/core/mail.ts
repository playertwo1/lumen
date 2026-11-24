// The Mail pill on Windows (classic Outlook, src-tauri/src/mail): the types the island and
// the Coucou window read, and the calls to Rust. Port of MailController / MailTriageStore /
// MailAgent's shapes; the wording is the Mac's (MailViews.swift, MailHubPages.swift).

import { invoke } from "@tauri-apps/api/core";
import { IS_TAURI } from "./bridge";
import { t } from "../i18n/i18n";

export const MAIL_ID = "integration_mail";
export const MAIL_BLUE = "#2F8CFF";

export interface MailMessage {
  entryId: string;
  storeId: string;
  account: string;
  senderName: string;
  address: string;
  subject: string;
  /** Milliseconds since 1970. */
  date: number;
  unread: boolean;
  flagged: boolean;
  local: string;
}

export type MailFilter = "all" | "unread" | "flagged" | "today";
export const MAIL_FILTERS: MailFilter[] = ["all", "unread", "flagged", "today"];

export type MailAction =
  | { kind: "junk" }
  | { kind: "important" }
  | { kind: "markRead" }
  | { kind: "move"; mailbox: string };

export type MailField = "sender" | "domain" | "subject" | "text" | "account";
export const MAIL_FIELDS: MailField[] = ["sender", "domain", "subject", "text", "account"];

export interface MailRule {
  id: string;
  name: string;
  enabled: boolean;
  conditions: { field: MailField; value: string }[];
  action: MailAction;
}

export interface MailSettings {
  autoSort: boolean;
  autoSortSince: number | null;
  banned: string[];
  vip: string[];
  rules: MailRule[];
}

export type MailReason =
  | { kind: "vip"; value: string }
  | { kind: "banned"; value: string }
  | { kind: "rule"; value: string }
  | { kind: "manual" };

export interface JournalEntry {
  id: string;
  date: number;
  account: string;
  storeId: string;
  entryId: string;
  sender: string;
  subject: string;
  action: MailAction;
  reason: MailReason;
  undone: boolean;
}

export interface SentEntry {
  id: string;
  date: number;
  to: string[];
  subject: string;
  attachments: string[];
}

export interface TriageView {
  settings: MailSettings;
  journal: JournalEntry[];
  sent: SentEntry[];
  today: { sorted: number; junk: number; important: number };
  windows: boolean;
  outlookOpen: boolean;
  backend?: MailBackend;
  classified?: ClassifiedMail[];
}

export type MailCategory = "conta" | "assinatura" | "importante" | "compra" | "notificacao" | "outros" | "promocao" | "spam";

/** Uma mensagem da triagem do MailCraft. O Lumen só recomenda: nunca apaga. */
export interface ClassifiedMail {
  message: MailMessage;
  category: MailCategory;
  recommend: "manter" | "ler" | "apagar";
  reason: string;
}

export interface ClassifyResult {
  items: ClassifiedMail[];
  note: string | null;
  totalReported?: number | null;
}

export interface PreparedMail {
  to: string[];
  cc: string[];
  subject: string;
  body: string;
  attachments: string[];
  problem: "noPage" | "badAddress" | null;
}

export type PlanPhase =
  | { kind: "preparing" }
  | { kind: "ready" }
  | { kind: "countdown"; seconds: number }
  | { kind: "running"; done: number; total: number }
  | { kind: "done"; ok: number; failed: number }
  | { kind: "cancelled" };

export interface PlanRun {
  phase: PlanPhase;
  summary: string;
  sorts: { action: MailAction; messages: MailMessage[] }[];
  replies: [MailMessage, string][];
  saves: [MailMessage, string][];
  mails: PreparedMail[];
  notes: { kind: "missing" | "noPdf" | "fileMissing"; value: string }[];
  testSentTo: string | null;
  myAddress: string | null;
  stepCount: number;
}

export type MailBackend = "mailcraft" | "outlook";
export type MailCraftMode = "control" | "mcp";

export interface MailCraftConfig {
  backend: MailBackend;
  mode: MailCraftMode;
  host: string;
  port: number;
  mcpCommand: string;
}

export interface MailCraftStatus {
  connected: boolean;
  mode: string;
  endpoint: string;
  keyFound: boolean;
  keyPath: string | null;
  accounts: number;
  mailboxState: string;
  error: string | null;
}

export interface InboxSummaryResult {
  summary: string;
  count: number;
  unread: number;
  backend: string;
  warnings: string[];
}

export interface DraftResult {
  saved: boolean;
  provider: string;
  composeId: number | null;
  openedWindow: boolean;
  note: string;
}

/** What the card shows, as Rust sends it in the pill's `integration` data. */
export interface MailCardData {
  running: boolean;
  windows: boolean;
  backend?: MailBackend;
  mode?: MailCraftMode;
  unread: number;
  messages: MailMessage[];
  shown: number;
  maxShown: number;
  error?: string | null;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!IS_TAURI) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    console.error(`[coucou] ${cmd} failed`, err);
    return null;
  }
}

export const MailBridge = {
  refresh: () => call<void>("mail_refresh"),
  more: () => call<void>("mail_more"),
  search: (query: string, filter: MailFilter) => call<MailMessage[]>("mail_search", { query, filter }),
  detail: (message: MailMessage) => call<{ text: string; attachments: string[] }>("mail_detail", { message }),
  open: (message: MailMessage, reply: boolean) => call<void>("mail_open", { message, reply }),
  act: (message: MailMessage, kind: "read" | "flag" | "unflag" | "junk" | "ignore" | "vip") =>
    call<void>("mail_act", { message, kind }),
  /** Como `act`, mas o erro do Rust chega à tela. */
  actChecked: async (message: MailMessage, kind: "read" | "flag" | "delete" | "junk" | "undelete" | "unjunk"): Promise<void> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    await invoke<void>("mail_act", { message, kind });
  },
  /** Mostra a mensagem no MailCraft (ou no Outlook) e traz a janela para a frente; o erro chega à tela. */
  openChecked: async (message: MailMessage): Promise<void> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    await invoke<void>("mail_open", { message, reply: false });
  },
  configGet: () => call<MailCraftConfig>("mail_config_get"),
  configSet: (config: MailCraftConfig) => call<MailCraftConfig>("mail_config_set", { config }),
  testConnection: async (config?: MailCraftConfig): Promise<MailCraftStatus | null> =>
    IS_TAURI ? invoke<MailCraftStatus>("mail_test_connection", config ? { config } : {}) : null,
  summarizeInbox: async (limit?: number): Promise<InboxSummaryResult> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    return invoke<InboxSummaryResult>("mail_summarize_inbox", { limit });
  },
  classify: async (limit?: number): Promise<ClassifyResult> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    return invoke<ClassifyResult>("mail_classify", { limit });
  },
  // Sem `call`: a mensagem de erro do Rust (ex.: MailCraft não encontrado) precisa chegar à tela.
  openMailCraft: async (): Promise<string> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    return invoke<string>("mail_open_mailcraft");
  },
  /** Fecha o MailCraft (pedido normal, sem forçar) e o abre de novo com o canal de controle. */
  restartMailCraft: async (): Promise<string> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    return invoke<string>("mail_restart_mailcraft");
  },
  prepareDraft: async (payload: { replyToId?: string | null; to: string; subject: string; body: string }): Promise<DraftResult> => {
    if (!IS_TAURI) throw new Error("Disponível apenas no app desktop.");
    return invoke<DraftResult>("mail_prepare_draft", payload);
  },
  triage: () => call<TriageView>("mail_triage"),
  setTriage: (settings: MailSettings) => call<void>("mail_triage_set", { settings }),
  clearJournal: () => call<void>("mail_journal_clear"),
  undo: (id: string) => call<void>("mail_undo", { id }),
  planShow: (id: string, text: string) => call<PlanRun>("mail_plan_show", { id, text }),
  planTest: (id: string) => call<void>("mail_plan_test", { id }),
  planStart: (id: string) => call<void>("mail_plan_start", { id }),
  planCancel: (id: string) => call<void>("mail_plan_cancel", { id }),
};

/** The chat's Mail switch: off at each start, like the Mac's. */
export const MailChat = { on: false };

export function messageId(m: MailMessage): string {
  return `${m.storeId}/${m.entryId}`;
}

/** "Name" from the message, else the address before "@". */
export function senderName(m: { senderName: string; address: string }): string {
  return m.senderName.trim() || m.address.split("@")[0] || m.address;
}

/** "Name" from "Name <address>" (the journal keeps the sender that way). */
export function nameOf(sender: string): string {
  const lt = sender.indexOf("<");
  const name = (lt >= 0 ? sender.slice(0, lt) : sender).trim().replace(/^"|"$/g, "");
  return name || sender.replace(/[<>]/g, "").split("@")[0];
}

export function filterTitle(f: MailFilter): string {
  switch (f) {
    case "all": return t("All");
    case "unread": return t("Unread");
    case "flagged": return t("Important");
    case "today": return t("Today");
  }
}

export function fieldTitle(f: MailField): string {
  switch (f) {
    case "sender": return t("Sender");
    case "domain": return t("Sender’s domain");
    case "subject": return t("Subject");
    case "text": return t("Text");
    case "account": return t("Account");
  }
}

/** "Move to Junk", in a rule. */
export function ruleLine(a: MailAction): string {
  switch (a.kind) {
    case "junk": return t("Move to Junk");
    case "important": return t("Mark important");
    case "markRead": return t("Mark as read");
    case "move": return t("Move to {0}", { 0: a.mailbox });
  }
}

/** "Moved to Junk", in the journal. */
export function journalLine(a: MailAction): string {
  switch (a.kind) {
    case "junk": return t("Moved to Junk");
    case "important": return t("Marked important");
    case "markRead": return t("Marked as read");
    case "move": return t("Moved to {0}", { 0: a.mailbox });
  }
}

export function reasonLine(r: MailReason): string {
  switch (r.kind) {
    case "vip": return t("VIP {0}", { 0: r.value });
    case "banned": return t("banned word “{0}”", { 0: r.value });
    case "rule": return t("rule “{0}”", { 0: r.value });
    case "manual": return t("by hand");
  }
}

/** The answer without its ```coucou-mail block, and whether it held one (mail/plan.rs). */
export function splitPlan(text: string): { prose: string; plan: boolean } {
  const fence = "```coucou-mail";
  const start = text.indexOf(fence);
  if (start < 0) return { prose: text, plan: false };
  const after = text.slice(start + fence.length);
  const end = after.indexOf("```");
  const prose = (text.slice(0, start) + (end >= 0 ? after.slice(end + 3) : "")).trim();
  return { prose, plan: end >= 0 };
}

/** A new rule's name: "Sender contains “newsletter” → Move to Junk". */
export function ruleName(field: MailField, value: string, action: MailAction): string {
  return t("{0} contains “{1}” → {2}", { 0: fieldTitle(field), 1: value, 2: ruleLine(action) });
}
