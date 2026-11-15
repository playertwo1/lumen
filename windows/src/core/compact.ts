// Conteúdo dinâmico do Centro Contextual no Notch Compacto (Modo 2A + Centro Contextual).
// Mantido livre de DOM para facilitar testes determinísticos de prioridade e largura.

import type { TodoItem } from "./tasks";

export type CompactCenterAction =
  | { kind: "approval" }
  | { kind: "focus"; pillId: string }
  | { kind: "task"; taskId: string }
  | { kind: "music" }
  | { kind: "view"; view: "overview" | "taskCapture" };

export interface CompactNotice {
  text: string;
  color: string;
  expiresAt: number;
  action?: CompactCenterAction | null;
}

export interface CompactInputs {
  now: Date;
  pendingApproval: { pillName?: string; agent?: string; questions?: unknown } | null;
  notice: CompactNotice | null;
  whatsapp: { unread: number; conversations: { name: string; unread: number }[] } | null;
  mailUnread: number;
  todos: TodoItem[];
  music: { title: string; artist: string } | null;
}

export interface CompactCenterItem {
  kind: "approval" | "notice" | "whatsapp" | "mail" | "task" | "music";
  text: string;
  dot: string | null;
  note: boolean;
  urgent: boolean;
  action: CompactCenterAction | null;
}

const UPCOMING_WINDOW_MS = 2 * 3_600_000;

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

function localDateKey(d: Date): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

function parseTodoDueMs(item: TodoItem): number | null {
  if (!item.due_date || !item.due_time) return null;
  const dm = /^(\d{4})-(\d{2})-(\d{2})$/.exec(item.due_date);
  const tm = /^(\d{2}):(\d{2})/.exec(item.due_time);
  if (!dm || !tm) return null;
  const dt = new Date(
    Number(dm[1]),
    Number(dm[2]) - 1,
    Number(dm[3]),
    Number(tm[1]),
    Number(tm[2]),
    0,
    0,
  );
  const ms = dt.getTime();
  return Number.isFinite(ms) ? ms : null;
}

function nextRelevantTodo(todos: TodoItem[], now: Date): { item: TodoItem; overdue: boolean } | null {
  const nowMs = now.getTime();
  const todayKey = localDateKey(now);
  let best: { item: TodoItem; dueMs: number; overdue: boolean } | null = null;

  for (const item of todos) {
    if (item.done || item.deleted_at) continue;
    if (item.snoozed_until) {
      const snoozeMs = Date.parse(item.snoozed_until);
      if (Number.isFinite(snoozeMs) && snoozeMs > nowMs) continue;
    }
    const dueMs = parseTodoDueMs(item);
    if (dueMs == null) continue;
    const diff = dueMs - nowMs;
    const isTodayOverdue = item.due_date === todayKey && diff <= 0;
    const isSoon = diff > 0 && diff <= UPCOMING_WINDOW_MS;
    if (!isTodayOverdue && !isSoon) continue;
    if (!best || dueMs < best.dueMs) {
      best = { item, dueMs, overdue: isTodayOverdue };
    }
  }

  return best ? { item: best.item, overdue: best.overdue } : null;
}

/**
 * Resolve o item exibido no centro do Notch Compacto.
 * Retorna `null` quando não há evento/mensagem/tarefa ativa para que o notch
 * assuma o formato compacto enxuto (Opção 2A).
 */
export function compactCenter(input: CompactInputs): CompactCenterItem | null {
  if (input.pendingApproval) {
    const who = (input.pendingApproval.pillName || input.pendingApproval.agent || "").trim();
    const kindLabel = input.pendingApproval.questions ? "Pergunta" : "Aprovação";
    return {
      kind: "approval",
      text: who ? `${kindLabel} · ${who}` : `${kindLabel} pendente`,
      dot: "#F5A524",
      note: false,
      urgent: true,
      action: { kind: "approval" },
    };
  }

  const nowMs = input.now.getTime();
  if (input.notice && input.notice.expiresAt > nowMs && input.notice.text.trim()) {
    return {
      kind: "notice",
      text: input.notice.text.trim(),
      dot: input.notice.color,
      note: false,
      urgent: false,
      action: input.notice.action ?? null,
    };
  }

  if (input.whatsapp && input.whatsapp.unread > 0) {
    const unreadConvs = input.whatsapp.conversations.filter((c) => c.unread > 0);
    let text: string;
    if (unreadConvs.length === 1) {
      const c = unreadConvs[0];
      text = c.unread > 1 ? `${c.name} (${c.unread})` : `Nova msg · ${c.name}`;
    } else {
      text = `WhatsApp · ${input.whatsapp.unread} ${input.whatsapp.unread === 1 ? "nova" : "novas"}`;
    }
    return {
      kind: "whatsapp",
      text,
      dot: "#22C55E",
      note: false,
      urgent: false,
      action: { kind: "focus", pillId: "integration_whatsapp" },
    };
  }

  if (input.mailUnread > 0) {
    return {
      kind: "mail",
      text: `Mail · ${input.mailUnread} ${input.mailUnread === 1 ? "não lido" : "não lidos"}`,
      dot: "#38BDF8",
      note: false,
      urgent: false,
      action: { kind: "focus", pillId: "integration_mail" },
    };
  }

  const nextTask = nextRelevantTodo(input.todos, input.now);
  if (nextTask) {
    const { item, overdue } = nextTask;
    return {
      kind: "task",
      text: overdue ? `Atrasada · ${item.text}` : `${item.due_time} · ${item.text}`,
      dot: overdue ? "#F5A524" : "#22D3EE",
      note: false,
      urgent: overdue,
      action: { kind: "task", taskId: item.id },
    };
  }

  if (input.music && input.music.title) {
    return {
      kind: "music",
      text: input.music.artist ? `${input.music.title} · ${input.music.artist}` : input.music.title,
      dot: null,
      note: true,
      urgent: false,
      action: { kind: "music" },
    };
  }

  return null;
}
