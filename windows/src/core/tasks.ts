// Tarefas do Lumen (L4): tipos em paridade com todo.json v2, atalhos de agenda,
// busca sem acento e parser de propostas enviadas no chat.

export type TodoPriority = "low" | "normal" | "high" | "urgent";
export type TodoRepeatFrequency = "daily" | "weekly" | "monthly";
export type TodoDateShortcut = "today" | "tomorrow" | "inThreeDays" | "nextMonday";
export type TodoBucket = "overdue" | "today" | "upcoming" | "unscheduled" | "done";
export type TodoProposalState = "unspecified" | "explicit" | "ambiguous";

export interface TodoSubtask {
  id: string;
  text: string;
  done: boolean;
}

export interface TodoRecurrence {
  frequency: TodoRepeatFrequency;
  weekdays?: number[];
  day_of_month?: number | null;
}

export interface TodoSource {
  session_id: string;
  item_id: string;
  excerpt: string;
  clock?: string | null;
}

export interface TodoItem {
  id: string;
  text: string;
  due: string;
  due_date: string | null;
  due_time: string | null;
  time_zone: string;
  alert_minutes: number;
  priority: TodoPriority;
  subtasks: TodoSubtask[];
  recurrence: TodoRecurrence | null;
  series_id: string | null;
  done: boolean;
  created_at: string;
  done_at: string | null;
  snoozed_until: string | null;
  source?: TodoSource | null;
  source_stale?: boolean;
  version: number;
  deleted_at: string | null;
}

export interface TodoShortcutSettings {
  enabled_date_shortcuts: TodoDateShortcut[];
  time_shortcuts_minutes: [number, number, number];
  default_alert_minutes: number;
}

export interface TodoProposal {
  id: string;
  title: string;
  date_state: TodoProposalState;
  due_date: string | null;
  date_phrase: string | null;
  time_state: TodoProposalState;
  due_time: string | null;
  time_phrase: string | null;
  priority: TodoPriority;
}

export interface TasksSnapshot {
  items: TodoItem[];
  shortcuts: TodoShortcutSettings;
  windows_notifications_available: boolean;
  voice_ready: boolean;
  recording: boolean;
  quarantined_file: string | null;
}

export interface TaskReminderPayload {
  id: string;
  text: string;
  due_date: string | null;
  due_time: string | null;
  priority: TodoPriority;
  alert_minutes: number;
}

export interface ConfirmProposalInput {
  text: string;
  due_date?: string | null;
  due_time?: string | null;
  time_zone?: string;
  alert_minutes?: number;
  priority?: TodoPriority;
  subtasks?: string[];
  recurrence?: TodoRecurrence | null;
}

export interface CreateTaskInput extends ConfirmProposalInput {
  due?: string;
  source?: TodoSource | null;
}

export interface UpdateTaskInput {
  id: string;
  text?: string;
  due?: string;
  due_date?: string | null;
  due_time?: string | null;
  time_zone?: string;
  alert_minutes?: number;
  priority?: TodoPriority;
  recurrence?: TodoRecurrence | null;
  clear_snooze?: boolean;
}

export interface ToggleDoneOutcome {
  item: TodoItem;
  next_occurrence?: TodoItem | null;
}

export interface GroupedTasks {
  overdue: TodoItem[];
  today: TodoItem[];
  upcoming: TodoItem[];
  unscheduled: TodoItem[];
  done: TodoItem[];
}

export const DEFAULT_SHORTCUT_SETTINGS: TodoShortcutSettings = {
  enabled_date_shortcuts: ["today", "tomorrow", "inThreeDays", "nextMonday"],
  time_shortcuts_minutes: [540, 840, 1200],
  default_alert_minutes: 20,
};

export const DATE_SHORTCUT_LABELS: Record<TodoDateShortcut, string> = {
  today: "Hoje",
  tomorrow: "Amanhã",
  inThreeDays: "Daqui a 3 dias",
  nextMonday: "Semana que vem",
};

export const PRIORITY_LABELS: Record<TodoPriority, string> = {
  low: "Baixa",
  normal: "Normal",
  high: "Alta",
  urgent: "Urgente",
};

const PRIORITY_RANK: Record<TodoPriority, number> = {
  urgent: 0,
  high: 1,
  normal: 2,
  low: 3,
};

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const TIME_RE = /^\d{2}:\d{2}$/;
const TODO_FENCE_RE = /```lumen-todo\s*\r?\n([\s\S]*?)```/;

export function isValidIsoDate(value: string | null | undefined): value is string {
  if (!value || !DATE_RE.test(value)) return false;
  const [y, m, d] = value.split("-").map(Number);
  if (m < 1 || m > 12 || d < 1 || d > 31) return false;
  const check = new Date(Date.UTC(y, m - 1, d));
  return check.getUTCFullYear() === y && check.getUTCMonth() === m - 1 && check.getUTCDate() === d;
}

export function isValidClockTime(value: string | null | undefined): value is string {
  if (!value || !TIME_RE.test(value)) return false;
  const [hh, mm] = value.split(":").map(Number);
  return hh >= 0 && hh <= 23 && mm >= 0 && mm <= 59;
}

export function formatLocalDate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function todayIsoLocal(now = new Date()): string {
  return formatLocalDate(now);
}

export function dateForShortcut(shortcut: TodoDateShortcut, now = new Date()): string {
  const base = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  switch (shortcut) {
    case "today":
      return formatLocalDate(base);
    case "tomorrow":
      base.setDate(base.getDate() + 1);
      return formatLocalDate(base);
    case "inThreeDays":
      base.setDate(base.getDate() + 3);
      return formatLocalDate(base);
    case "nextMonday": {
      const dow = base.getDay(); // 0=Dom, 1=Seg ... 6=Sáb
      const delta = dow === 1 ? 7 : (8 - dow) % 7;
      base.setDate(base.getDate() + delta);
      return formatLocalDate(base);
    }
  }
}

export function formatTimeMinutes(totalMinutes: number): string {
  const clamped = Math.max(0, Math.min(1439, Math.floor(totalMinutes)));
  const hh = String(Math.floor(clamped / 60)).padStart(2, "0");
  const mm = String(clamped % 60).padStart(2, "0");
  return `${hh}:${mm}`;
}

export function timeChipLabel(totalMinutes: number): string {
  const clamped = Math.max(0, Math.min(1439, Math.floor(totalMinutes)));
  const hh = Math.floor(clamped / 60);
  const mm = clamped % 60;
  return mm === 0 ? `${hh}h` : `${String(hh).padStart(2, "0")}:${String(mm).padStart(2, "0")}`;
}

/** Normaliza texto para busca em português ignorando maiúsculas e acentos. */
export function foldPt(text: string): string {
  return text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .trim();
}

export function bucketFor(
  item: Pick<TodoItem, "done" | "due_date">,
  todayIso = todayIsoLocal(),
): TodoBucket {
  if (item.done) return "done";
  if (!isValidIsoDate(item.due_date)) return "unscheduled";
  if (item.due_date < todayIso) return "overdue";
  if (item.due_date === todayIso) return "today";
  return "upcoming";
}

function comparePending(a: TodoItem, b: TodoItem): number {
  const da = a.due_date ?? "9999-99-99";
  const db = b.due_date ?? "9999-99-99";
  if (da !== db) return da < db ? -1 : 1;
  const ta = a.due_time ?? "99:99";
  const tb = b.due_time ?? "99:99";
  if (ta !== tb) return ta < tb ? -1 : 1;
  const pa = PRIORITY_RANK[a.priority] ?? 2;
  const pb = PRIORITY_RANK[b.priority] ?? 2;
  if (pa !== pb) return pa - pb;
  return a.created_at < b.created_at ? -1 : a.created_at > b.created_at ? 1 : 0;
}

export function groupTasks(items: TodoItem[], todayIso = todayIsoLocal()): GroupedTasks {
  const groups: GroupedTasks = {
    overdue: [],
    today: [],
    upcoming: [],
    unscheduled: [],
    done: [],
  };
  for (const item of items) {
    if (item.deleted_at) continue;
    groups[bucketFor(item, todayIso)].push(item);
  }
  groups.overdue.sort(comparePending);
  groups.today.sort(comparePending);
  groups.upcoming.sort(comparePending);
  groups.unscheduled.sort(comparePending);
  groups.done.sort((a, b) => {
    const ka = a.done_at ?? a.created_at;
    const kb = b.done_at ?? b.created_at;
    return ka > kb ? -1 : ka < kb ? 1 : 0;
  });
  return groups;
}

export function searchTodos(items: TodoItem[], query: string): TodoItem[] {
  const tokens = foldPt(query).split(/\s+/).filter(Boolean);
  const visible = items.filter((item) => !item.deleted_at);
  if (tokens.length === 0) return visible;
  return visible.filter((item) => {
    const haystack = foldPt(
      [
        item.text,
        item.due,
        item.due_date ?? "",
        item.due_time ?? "",
        PRIORITY_LABELS[item.priority] ?? item.priority,
        ...item.subtasks.map((s) => s.text),
      ].join(" "),
    );
    return tokens.every((tok) => haystack.includes(tok));
  });
}

export function formatDueLabel(
  item: Pick<TodoItem, "due_date" | "due_time" | "alert_minutes">,
  now = new Date(),
): string {
  if (!isValidIsoDate(item.due_date)) return "Sem data";
  const today = formatLocalDate(now);
  const tomorrow = dateForShortcut("tomorrow", now);
  let dateLabel: string;
  if (item.due_date === today) {
    dateLabel = "Hoje";
  } else if (item.due_date === tomorrow) {
    dateLabel = "Amanhã";
  } else {
    const [y, m, d] = item.due_date.split("-");
    dateLabel = `${d}/${m}/${y}`;
  }
  if (!isValidClockTime(item.due_time)) return dateLabel;
  const alert = item.alert_minutes > 0 ? ` · aviso ${item.alert_minutes}m` : " · no horário";
  return `${dateLabel} · ${item.due_time}${alert}`;
}

export function recurrenceLabel(rec: TodoRecurrence | null | undefined): string {
  if (!rec) return "";
  switch (rec.frequency) {
    case "daily":
      return "Todos os dias";
    case "weekly":
      return "Semanal";
    case "monthly":
      return rec.day_of_month ? `Todo dia ${rec.day_of_month}` : "Mensal";
  }
}

export function isAmbiguousProposal(proposal: Pick<TodoProposal, "date_state" | "time_state">): boolean {
  return proposal.date_state === "ambiguous" || proposal.time_state === "ambiguous";
}

function asProposalState(value: unknown): TodoProposalState {
  if (value === "explicit" || value === "ambiguous") return value;
  return "unspecified";
}

function asPriority(value: unknown): TodoPriority {
  if (value === "low" || value === "normal" || value === "high" || value === "urgent") return value;
  return "normal";
}

/**
 * Extrai um bloco ```lumen-todo { ... } ``` da resposta do chat sem inventar datas/horas
 * quando o estado estiver `ambiguous` ou `unspecified`.
 */
export function splitTodoProposal(markdown: string): {
  text: string;
  proposal: TodoProposal | null;
  isAmbiguous: boolean;
} {
  const match = markdown.match(TODO_FENCE_RE);
  if (!match) {
    return { text: markdown, proposal: null, isAmbiguous: false };
  }
  const stripped = markdown.replace(match[0], "").replace(/\n{3,}/g, "\n\n").trim();
  try {
    const raw = JSON.parse(match[1].trim()) as Record<string, unknown>;
    const title = typeof raw.title === "string"
      ? raw.title.trim()
      : typeof raw.text === "string"
        ? raw.text.trim()
        : "";
    if (!title) {
      return { text: markdown, proposal: null, isAmbiguous: false };
    }
    let date_state = asProposalState(raw.date_state);
    let due_date = typeof raw.due_date === "string" && isValidIsoDate(raw.due_date) ? raw.due_date : null;
    if (date_state !== "explicit" || !due_date) {
      due_date = null;
      if (date_state === "explicit") date_state = "unspecified";
    }
    let time_state = asProposalState(raw.time_state);
    let due_time = typeof raw.due_time === "string" && isValidClockTime(raw.due_time) ? raw.due_time : null;
    if (time_state !== "explicit" || !due_time) {
      due_time = null;
      if (time_state === "explicit") time_state = "unspecified";
    }
    const proposal: TodoProposal = {
      id: typeof raw.id === "string" && raw.id.trim() ? raw.id.trim() : `chat-${Date.now()}`,
      title,
      date_state,
      due_date,
      date_phrase: date_state === "ambiguous" && typeof raw.date_phrase === "string" ? raw.date_phrase : null,
      time_state,
      due_time,
      time_phrase: time_state === "ambiguous" && typeof raw.time_phrase === "string" ? raw.time_phrase : null,
      priority: asPriority(raw.priority),
    };
    return { text: stripped, proposal, isAmbiguous: isAmbiguousProposal(proposal) };
  } catch {
    return { text: markdown, proposal: null, isAmbiguous: false };
  }
}
