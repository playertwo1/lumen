// Views de Tarefas na Ilha e no Chat (L4.2 / L4.3 / L4.4):
// - taskCapture: captura rápida por texto ou microfone
// - taskConfirm: card editável de confirmação (nunca salva data/hora ambígua sem clique)
// - taskReminder: lembrete acionável na Ilha (Adiar 10m / Concluir / Abrir)
// - chatTodoProposalCard: card editável dentro da bolha do chat para ```lumen-todo

import { clear, h, svg } from "./dom";
import { ICONS } from "./icons";
import { TasksBridge, onEvent } from "../core/bridge";
import { washRGBA } from "../core/layout";
import { Sound } from "../core/sound";
import {
  DATE_SHORTCUT_LABELS,
  DEFAULT_SHORTCUT_SETTINGS,
  PRIORITY_LABELS,
  buildRecurrence,
  dateForShortcut,
  formatDueLabel,
  formatTimeMinutes,
  isAmbiguousProposal,
  timeChipLabel,
  type TaskReminderPayload,
  type TodoDateShortcut,
  type TodoItem,
  type TodoPriority,
  type TodoProposal,
  type TodoShortcutSettings,
} from "../core/tasks";
import type { ViewActions, ViewHost } from "./views";

let pendingProposal: TodoProposal | null = null;
let pendingShortcuts: TodoShortcutSettings = { ...DEFAULT_SHORTCUT_SETTINGS };
let activeReminder: TaskReminderPayload | null = null;

const refreshConfirmCallbacks = new Set<() => void>();
const refreshReminderCallbacks = new Set<() => void>();

export function setPendingTaskProposal(
  proposal: TodoProposal,
  shortcuts?: TodoShortcutSettings,
) {
  pendingProposal = proposal;
  if (shortcuts) pendingShortcuts = shortcuts;
  refreshConfirmCallbacks.forEach((fn) => fn());
}

export function setActiveTaskReminder(payload: TaskReminderPayload) {
  activeReminder = payload;
  refreshReminderCallbacks.forEach((fn) => fn());
}

const MIC_ICON = "M12 3.5a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0v-5a3 3 0 0 1 3-3zM6.5 11.5a5.5 5.5 0 0 0 11 0M12 17v3.5";

function taskCard(wash: "amber" | "green" | "cyan", body: HTMLElement): HTMLElement {
  return h(
    "div",
    { class: "card task-island-card", style: `--wash:${washRGBA(wash)}` },
    body,
  );
}

export interface ProposalEditorOptions {
  proposal: TodoProposal;
  shortcuts?: TodoShortcutSettings;
  compact?: boolean;
  onConfirmed: (item: TodoItem) => void;
  onDiscard: () => void;
  onDirty?: (dirty: boolean) => void;
}

/**
 * Editor compartilhado entre `taskConfirm` (Ilha) e `chatTodoProposalCard` (Chat).
 * Datas e horas só seguem preenchidas se estiverem `explicit` — caso contrário ficam nulas
 * até o usuário escolher explicitamente um atalho ou campo.
 */
export function createProposalEditor(opts: ProposalEditorOptions): {
  el: HTMLElement;
  focus(): void;
} {
  const shortcuts = opts.shortcuts ?? DEFAULT_SHORTCUT_SETTINGS;
  let title = opts.proposal.title;
  let dueDate: string | null = opts.proposal.date_state === "explicit" ? opts.proposal.due_date : null;
  let dueTime: string | null = opts.proposal.time_state === "explicit" ? opts.proposal.due_time : null;
  let priority: TodoPriority = opts.proposal.priority ?? "normal";
  let alertMinutes: number = shortcuts.default_alert_minutes;
  let recurrenceFreq = "";
  let saving = false;

  const titleInput = h("input", {
    type: "text",
    class: "task-edit-input",
    value: title,
    placeholder: "Título da tarefa",
    spellcheck: "false",
  }) as HTMLInputElement;
  titleInput.addEventListener("input", () => {
    title = titleInput.value;
    opts.onDirty?.(true);
    syncButtons();
  });

  const ambHints: string[] = [];
  if (opts.proposal.date_state === "ambiguous") {
    ambHints.push(
      opts.proposal.date_phrase
        ? `Data ambígua ("${opts.proposal.date_phrase}") — confirme o dia`
        : "Data ambígua — escolha o dia",
    );
  }
  if (opts.proposal.time_state === "ambiguous") {
    ambHints.push(
      opts.proposal.time_phrase
        ? `Hora ambígua ("${opts.proposal.time_phrase}") — escolha o horário`
        : "Hora ambígua — escolha o horário",
    );
  }
  const ambBanner = ambHints.length > 0
    ? h("div", { class: "task-ambiguous-banner", text: ambHints.join(" · ") })
    : null;

  const dateInput = h("input", {
    type: "date",
    class: "task-native-picker",
    value: dueDate ?? "",
  }) as HTMLInputElement;
  const timeInput = h("input", {
    type: "time",
    class: "task-native-picker",
    value: dueTime ?? "",
  }) as HTMLInputElement;

  const datePillsRow = h("div", { class: "task-chip-row" });
  const dateButtons = new Map<TodoDateShortcut | "none", HTMLButtonElement>();
  for (const key of shortcuts.enabled_date_shortcuts) {
    const b = h("button", {
      type: "button",
      class: "task-chip",
      text: DATE_SHORTCUT_LABELS[key],
      onclick: () => {
        dueDate = dateForShortcut(key);
        dateInput.value = dueDate;
        opts.onDirty?.(true);
        syncChips();
      },
    }) as HTMLButtonElement;
    dateButtons.set(key, b);
    datePillsRow.append(b);
  }
  const noDateBtn = h("button", {
    type: "button",
    class: "task-chip",
    text: "Sem data",
    onclick: () => {
      dueDate = null;
      dueTime = null;
      dateInput.value = "";
      timeInput.value = "";
      opts.onDirty?.(true);
      syncChips();
    },
  }) as HTMLButtonElement;
  dateButtons.set("none", noDateBtn);
  datePillsRow.append(noDateBtn, dateInput);

  dateInput.addEventListener("change", () => {
    dueDate = dateInput.value || null;
    if (!dueDate) {
      dueTime = null;
      timeInput.value = "";
    }
    opts.onDirty?.(true);
    syncChips();
  });

  const timePillsRow = h("div", { class: "task-chip-row" });
  const timeButtons = new Map<string, HTMLButtonElement>();
  for (const mins of shortcuts.time_shortcuts_minutes) {
    const hhmm = formatTimeMinutes(mins);
    const b = h("button", {
      type: "button",
      class: "task-chip",
      text: timeChipLabel(mins),
      onclick: () => {
        if (!dueDate) {
          dueDate = dateForShortcut("today");
          dateInput.value = dueDate;
        }
        dueTime = hhmm;
        timeInput.value = hhmm;
        opts.onDirty?.(true);
        syncChips();
      },
    }) as HTMLButtonElement;
    timeButtons.set(hhmm, b);
    timePillsRow.append(b);
  }
  const noTimeBtn = h("button", {
    type: "button",
    class: "task-chip",
    text: "Sem hora",
    onclick: () => {
      dueTime = null;
      timeInput.value = "";
      opts.onDirty?.(true);
      syncChips();
    },
  }) as HTMLButtonElement;
  timeButtons.set("none", noTimeBtn);
  timePillsRow.append(noTimeBtn, timeInput);

  timeInput.addEventListener("change", () => {
    dueTime = timeInput.value || null;
    if (dueTime && !dueDate) {
      dueDate = dateForShortcut("today");
      dateInput.value = dueDate;
    }
    opts.onDirty?.(true);
    syncChips();
  });

  const prioSelect = h(
    "select",
    { class: "task-select", "aria-label": "Prioridade" },
    ...(["low", "normal", "high", "urgent"] as TodoPriority[]).map((p) =>
      h("option", { value: p, text: `Prioridade: ${PRIORITY_LABELS[p]}` }),
    ),
  ) as HTMLSelectElement;
  prioSelect.value = priority;
  prioSelect.addEventListener("change", () => {
    priority = prioSelect.value as TodoPriority;
  });

  const alertSelect = h(
    "select",
    { class: "task-select", "aria-label": "Pré-alerta" },
    h("option", { value: "0", text: "Aviso: na hora" }),
    h("option", { value: "5", text: "Aviso: 5 min antes" }),
    h("option", { value: "20", text: "Aviso: 20 min antes" }),
    h("option", { value: "60", text: "Aviso: 1 h antes" }),
  ) as HTMLSelectElement;
  alertSelect.value = String(alertMinutes);
  alertSelect.addEventListener("change", () => {
    alertMinutes = Number(alertSelect.value) || 0;
  });

  const recSelect = h(
    "select",
    { class: "task-select", "aria-label": "Repetição" },
    h("option", { value: "", text: "Não repetir" }),
    h("option", { value: "daily", text: "Diária" }),
    h("option", { value: "weekly", text: "Semanal" }),
    h("option", { value: "monthly", text: "Mensal" }),
  ) as HTMLSelectElement;
  recSelect.value = recurrenceFreq;
  recSelect.addEventListener("change", () => {
    recurrenceFreq = recSelect.value;
  });

  const metaRow = h("div", { class: "task-meta-row" }, prioSelect, alertSelect, recSelect);

  const errorEl = h("div", { class: "task-error", hidden: true });
  const confirmBtn = h("button", {
    type: "button",
    class: "btn primary",
    text: "Confirmar",
    onclick: () => void submit(),
  }) as HTMLButtonElement;
  const discardBtn = h("button", {
    type: "button",
    class: "btn secondary",
    text: "Descartar",
    onclick: () => opts.onDiscard(),
  }) as HTMLButtonElement;

  function syncChips() {
    for (const [k, btn] of dateButtons) {
      const active = k === "none" ? dueDate === null : dueDate === dateForShortcut(k);
      btn.classList.toggle("on", active);
    }
    for (const [k, btn] of timeButtons) {
      const active = k === "none" ? dueTime === null : dueTime === k;
      btn.classList.toggle("on", active);
    }
    alertSelect.disabled = !dueDate || !dueTime;
  }

  function syncButtons() {
    confirmBtn.disabled = saving || title.trim().length === 0;
    confirmBtn.textContent = saving ? "Salvando…" : "Confirmar";
  }

  async function submit() {
    const cleanTitle = title.trim();
    if (!cleanTitle || saving) return;
    saving = true;
    errorEl.hidden = true;
    syncButtons();
    try {
      const item = await TasksBridge.confirmProposal({
        text: cleanTitle,
        due_date: dueDate,
        due_time: dueTime,
        alert_minutes: alertMinutes,
        priority,
        recurrence: buildRecurrence(recurrenceFreq, dueDate),
      });
      Sound.play("approve");
      opts.onConfirmed(item);
    } catch (err) {
      errorEl.textContent = String(err).replace(/^Error:\s*/, "");
      errorEl.hidden = false;
      Sound.play("error");
    } finally {
      saving = false;
      syncButtons();
    }
  }

  titleInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      opts.onDiscard();
    }
  });

  syncChips();
  syncButtons();

  const el = h(
    "div",
    { class: opts.compact ? "task-editor compact" : "task-editor" },
    ambBanner,
    titleInput,
    datePillsRow,
    timePillsRow,
    metaRow,
    errorEl,
    h("div", { class: "actions" }, confirmBtn, discardBtn),
  );

  return {
    el,
    focus() {
      titleInput.focus();
      titleInput.select();
    },
  };
}

// ── Island: Captura rápida (Ctrl+Alt+K) ──────────────────────────────────────

export function buildTaskCaptureView(actions: ViewActions): ViewHost {
  let recording = false;
  let busy = false;
  let statusError = "";

  const input = h("input", {
    type: "text",
    class: "chat-input",
    placeholder: "Ex.: pagar boleto amanhã 14h",
    spellcheck: "false",
  }) as HTMLInputElement;

  const micBtn = h("button", {
    type: "button",
    class: "dictate-btn",
    title: "Ditar tarefa pelo microfone",
    "aria-label": "Ditar tarefa",
    onclick: () => void toggleVoice(),
  }, svg(MIC_ICON, 13, { stroke: 1.8 })) as HTMLButtonElement;

  const proposeBtn = h("button", {
    type: "button",
    class: "send-btn",
    title: "Gerar cartão editável",
    "aria-label": "Propor tarefa",
    onclick: () => void submitText(),
  }, svg(ICONS.arrowUp, 11)) as HTMLButtonElement;

  const bar = h("div", { class: "chat-bar" }, input, micBtn, proposeBtn);
  const errorEl = h("div", { class: "task-error", hidden: true });

  const openHubBtn = h("button", {
    type: "button",
    class: "btn secondary",
    text: "Abrir Tarefas",
    onclick: () => {
      void TasksBridge.openPage(null);
      actions.collapse();
    },
  });
  const closeBtn = h("button", {
    type: "button",
    class: "btn secondary",
    text: "Fechar",
    onclick: () => {
      if (recording) void TasksBridge.voiceCancel();
      actions.holdOpen(false);
      actions.collapse();
    },
  });

  const body = h(
    "div",
    { class: "task-island-body" },
    h(
      "div",
      { class: "task-island-head" },
      h("span", { class: "title", text: "Nova tarefa" }),
      h("span", { class: "task-shortcut-badge", text: "Ctrl+Alt+K" }),
    ),
    bar,
    errorEl,
    h("div", { class: "actions" }, openHubBtn, closeBtn),
  );
  const el = h("div", { class: "view" }, taskCard("cyan", body));

  function sync() {
    micBtn.classList.toggle("on", recording);
    micBtn.disabled = busy;
    proposeBtn.disabled = busy || recording || input.value.trim().length === 0;
    input.disabled = busy || recording;
    input.placeholder = recording
      ? "Ouvindo… clique no microfone para concluir"
      : "Ex.: pagar boleto amanhã 14h";
    errorEl.textContent = statusError;
    errorEl.hidden = !statusError;
    actions.holdOpen(recording || busy || input.value.trim().length > 0);
  }

  async function toggleVoice() {
    if (busy) return;
    statusError = "";
    if (!recording) {
      try {
        await TasksBridge.voiceStart();
        recording = true;
        Sound.play("blip");
      } catch (err) {
        statusError = String(err).replace(/^Error:\s*/, "");
        Sound.play("error");
      }
      sync();
      return;
    }

    busy = true;
    sync();
    try {
      const proposal = await TasksBridge.voiceStop();
      recording = false;
      const snap = await TasksBridge.state();
      setPendingTaskProposal(proposal, snap?.shortcuts);
      actions.holdOpen(false);
      actions.setView("taskConfirm");
    } catch (err) {
      recording = false;
      statusError = String(err).replace(/^Error:\s*/, "");
      Sound.play("error");
    } finally {
      busy = false;
      sync();
    }
  }

  async function submitText() {
    const raw = input.value.trim();
    if (!raw || busy || recording) return;
    busy = true;
    statusError = "";
    sync();
    try {
      const [proposal, snap] = await Promise.all([
        TasksBridge.proposeFromText(raw),
        TasksBridge.state(),
      ]);
      input.value = "";
      setPendingTaskProposal(proposal, snap?.shortcuts);
      actions.holdOpen(false);
      actions.setView("taskConfirm");
    } catch (err) {
      statusError = String(err).replace(/^Error:\s*/, "");
      Sound.play("error");
    } finally {
      busy = false;
      sync();
    }
  }

  input.addEventListener("input", sync);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submitText();
    } else if (e.key === "Escape") {
      e.preventDefault();
      actions.holdOpen(false);
      actions.collapse();
    }
  });

  void onEvent<boolean>("tasks-voice-state", (on) => {
    recording = Boolean(on);
    sync();
  });

  return {
    el,
    sync,
    focus() {
      statusError = "";
      sync();
      input.focus();
    },
  };
}

// ── Island: Confirmação editável da tarefa ───────────────────────────────────

export function buildTaskConfirmView(actions: ViewActions): ViewHost {
  const host = h("div", { class: "task-island-body" });
  const el = h("div", { class: "view" }, taskCard("amber", host));
  let editorFocus: (() => void) | null = null;
  let renderedId = "";

  function render() {
    clear(host);
    editorFocus = null;
    if (!pendingProposal) {
      host.append(
        h("div", { class: "title", text: "Nenhuma proposta ativa" }),
        h("div", { class: "sub", text: "Use Ctrl+Alt+K para capturar uma tarefa." }),
      );
      return;
    }
    const header = h(
      "div",
      { class: "task-island-head" },
      h("span", {
        class: "title",
        text: isAmbiguousProposal(pendingProposal)
          ? "Confirmar detalhes da tarefa"
          : "Confirmar tarefa",
      }),
    );
    const editor = createProposalEditor({
      proposal: pendingProposal,
      shortcuts: pendingShortcuts,
      compact: true,
      onDirty: (dirty) => actions.holdOpen(dirty),
      onConfirmed: (item) => {
        pendingProposal = null;
        actions.holdOpen(false);
        actions.collapse();
        void item;
      },
      onDiscard: () => {
        pendingProposal = null;
        actions.holdOpen(false);
        actions.setView("taskCapture");
      },
    });
    editorFocus = editor.focus;
    renderedId = pendingProposal.id;
    host.append(header, editor.el);
  }

  refreshConfirmCallbacks.add(() => {
    renderedId = "";
    render();
    actions.holdOpen(Boolean(pendingProposal));
  });

  return {
    el,
    sync() {
      if ((pendingProposal?.id ?? "") !== renderedId) render();
    },
    focus() {
      render();
      actions.holdOpen(Boolean(pendingProposal));
      editorFocus?.();
    },
  };
}

// ── Island: Lembrete acionável ───────────────────────────────────────────────

export function buildTaskReminderView(actions: ViewActions): ViewHost {
  const heading = h("div", { class: "task-island-head" });
  const titleEl = h("div", { class: "title task-reminder-title" });
  const subEl = h("div", { class: "sub" });

  const snoozeBtn = h("button", {
    type: "button",
    class: "btn secondary",
    text: "Adiar 10m",
    onclick: () => {
      if (!activeReminder) return;
      const id = activeReminder.id;
      void TasksBridge.snooze(id, 10);
      actions.collapse();
    },
  });
  const doneBtn = h("button", {
    type: "button",
    class: "btn primary",
    text: "Concluir",
    onclick: () => {
      if (!activeReminder) return;
      const id = activeReminder.id;
      void TasksBridge.toggleDone(id, true);
      actions.collapse();
    },
  });
  const openBtn = h("button", {
    type: "button",
    class: "btn secondary",
    text: "Abrir",
    onclick: () => {
      const id = activeReminder?.id ?? null;
      void TasksBridge.openPage(id);
      actions.collapse();
    },
  });

  const body = h(
    "div",
    { class: "task-island-body" },
    heading,
    titleEl,
    subEl,
    h("div", { class: "actions" }, doneBtn, snoozeBtn, openBtn),
  );
  const el = h("div", { class: "view" }, taskCard("amber", body));

  function sync() {
    clear(heading);
    heading.append(
      h("span", { class: "task-reminder-badge", text: "Lembrete" }),
    );
    if (activeReminder) {
      heading.append(h("span", { class: "task-prio-tag", text: PRIORITY_LABELS[activeReminder.priority] }));
    }
    if (!activeReminder) {
      titleEl.textContent = "Nenhum lembrete ativo";
      subEl.textContent = "";
      return;
    }
    titleEl.textContent = activeReminder.text;
    subEl.textContent = formatDueLabel(activeReminder);
  }

  refreshReminderCallbacks.add(sync);

  return { el, sync };
}

// ── Chat: Card editável embutido em respostas com ```lumen-todo ──────────────

export function chatTodoProposalCard(initialProposal: TodoProposal): HTMLElement {
  const wrap = h("div", { class: "chat-todo-card" });
  const head = h(
    "div",
    { class: "chat-todo-head" },
    h("span", { class: "chat-todo-title", text: "Proposta de tarefa" }),
  );
  const slot = h("div");
  wrap.append(head, slot);

  function renderSaved(item: TodoItem) {
    clear(slot);
    slot.append(
      h(
        "div",
        { class: "chat-todo-done" },
        h("span", { text: `Salva: ${item.text} (${formatDueLabel(item)})` }),
        h("button", {
          type: "button",
          class: "btn secondary",
          text: "Abrir em Tarefas",
          onclick: () => void TasksBridge.openPage(item.id),
        }),
      ),
    );
  }

  const editor = createProposalEditor({
    proposal: initialProposal,
    shortcuts: DEFAULT_SHORTCUT_SETTINGS,
    compact: true,
    onConfirmed: (item) => renderSaved(item),
    onDiscard: () => {
      clear(slot);
      slot.append(h("div", { class: "chat-todo-discarded", text: "Proposta descartada." }));
    },
  });
  slot.append(editor.el);

  return wrap;
}
