// Página Tarefas do Hub (L4.1–L4.4): lista única (Atrasadas, Hoje, Próximas,
// Sem data e Concluídas), captura por texto ou microfone com cartão editável,
// atalhos de agenda/horário, subtarefas, busca sem acento e pré-visualização de aviso.

import { clear, h } from "../../views/dom";
import { createProposalEditor } from "../../views/tasks";
import { TasksBridge, onEvent } from "../../core/bridge";
import {
  DATE_SHORTCUT_LABELS,
  DEFAULT_SHORTCUT_SETTINGS,
  PRIORITY_LABELS,
  dateForShortcut,
  formatDueLabel,
  formatTimeMinutes,
  groupTasks,
  recurrenceLabel,
  searchTodos,
  timeChipLabel,
  type TasksSnapshot,
  type TodoBucket,
  type TodoDateShortcut,
  type TodoItem,
  type TodoPriority,
  type TodoProposal,
} from "../../core/tasks";
import type { HubContext } from "../shell";
import { button, segmented } from "../ui";

type FilterKind = "all" | TodoBucket;

let activeCleanup: (() => void) | null = null;

function emptySnapshot(): TasksSnapshot {
  return {
    items: [],
    shortcuts: { ...DEFAULT_SHORTCUT_SETTINGS },
    windows_notifications_available: false,
    voice_ready: false,
    recording: false,
    quarantined_file: null,
  };
}

export async function tasksPage(container: HTMLElement, _ctx: HubContext): Promise<void> {
  activeCleanup?.();
  activeCleanup = null;

  let snap: TasksSnapshot = (await TasksBridge.state()) ?? emptySnapshot();
  let filter: FilterKind = "all";
  let query = "";
  let draftProposal: TodoProposal | null = null;
  let editingId: string | null = null;
  let focusTaskId: string | null = null;
  let undoItem: TodoItem | null = null;
  let statusError = "";
  let busy = false;
  let recording = snap.recording;

  const captureInput = h("input", {
    type: "text",
    class: "hub-input tasks-capture-input",
    placeholder: "Nova tarefa… ex.: ligar pro contador amanhã 9h",
    spellcheck: "false",
  }) as HTMLInputElement;

  const searchInput = h("input", {
    type: "search",
    class: "hub-input tasks-search-input",
    placeholder: "Buscar (sem acento)…",
    value: query,
    spellcheck: "false",
  }) as HTMLInputElement;

  const proposeBtn = button("Propor", () => void handleProposeText(), "primary", true);
  const micBtn = button(recording ? "Concluir voz" : "Ditar", () => void handleToggleVoice(), "", true);

  const errorBanner = h("div", { class: "tasks-banner warn", hidden: true });
  const statusBanner = h("div", { class: "tasks-banner info", hidden: true });
  const undoSlot = h("div");
  const proposalSlot = h("div");
  const filterSlot = h("div");
  const listSlot = h("div", { class: "tasks-sections" });

  function updateCaptureButtons() {
    proposeBtn.disabled = busy || recording || captureInput.value.trim().length === 0;
    micBtn.textContent = recording ? "Concluir voz" : "Ditar";
    micBtn.classList.toggle("primary", recording);
    micBtn.disabled = busy;
    captureInput.disabled = busy || recording;
    errorBanner.textContent = statusError;
    errorBanner.hidden = !statusError;
  }

  function renderBanners() {
    const notes: string[] = [];
    if (snap.quarantined_file) {
      notes.push(`Arquivo todo.json corrompido movido para quarentena: ${snap.quarantined_file}.`);
    }
    if (!snap.windows_notifications_available) {
      notes.push("Notificações nativas do Windows indisponíveis nesta sessão — avisos abrem direto na Ilha.");
    }
    statusBanner.textContent = notes.join(" ");
    statusBanner.hidden = notes.length === 0;
  }

  function renderUndo() {
    clear(undoSlot);
    if (!undoItem) return;
    const removed = undoItem;
    undoSlot.append(
      h(
        "div",
        { class: "tasks-undo-bar" },
        h("span", { text: `Tarefa removida: "${removed.text}"` }),
        button("Desfazer", () => void restoreRemoved(removed.id), "primary", true),
        button("Fechar", () => {
          undoItem = null;
          renderUndo();
        }, "", true),
      ),
    );
  }

  function renderProposal() {
    clear(proposalSlot);
    if (!draftProposal) return;
    const editor = createProposalEditor({
      proposal: draftProposal,
      shortcuts: snap.shortcuts,
      onConfirmed: async (item) => {
        draftProposal = null;
        focusTaskId = item.id;
        await reload();
      },
      onDiscard: () => {
        draftProposal = null;
        renderProposal();
      },
    });
    proposalSlot.append(
      h(
        "div",
        { class: "hub-card tasks-proposal-card" },
        h("div", { class: "hub-label", text: "Confirmar proposta antes de salvar" }),
        editor.el,
      ),
    );
  }

  function renderTaskCard(item: TodoItem): HTMLElement {
    const isEditing = editingId === item.id;
    const check = h("input", {
      type: "checkbox",
      class: "tasks-checkbox",
      "aria-label": `Concluir ${item.text}`,
      onchange: () => void runMutation(() => TasksBridge.toggleDone(item.id, !item.done)),
    }) as HTMLInputElement;
    check.checked = item.done;

    const prioBadge = h("span", {
      class: `tasks-prio ${item.priority}`,
      text: PRIORITY_LABELS[item.priority],
    });
    const recText = recurrenceLabel(item.recurrence);
    const meta = h(
      "div",
      { class: "tasks-item-meta" },
      h("span", { text: formatDueLabel(item) }),
      recText ? h("span", { class: "tasks-chip-pill", text: recText }) : null,
      item.snoozed_until ? h("span", { class: "tasks-chip-pill snoozed", text: "Adiada" }) : null,
      prioBadge,
    );

    const actionsRow = h(
      "div",
      { class: "tasks-item-actions" },
      !item.done && item.due_date
        ? button("+10m", () => void runMutation(() => TasksBridge.snooze(item.id, 10)), "", true)
        : null,
      !item.done && item.due_date
        ? button("Testar aviso", () => void runMutation(() => TasksBridge.previewReminder(item.id)), "", true)
        : null,
      button(isEditing ? "Fechar" : "Editar", () => {
        editingId = isEditing ? null : item.id;
        renderList();
      }, "", true),
      button("Excluir", () => void handleDelete(item.id), "danger", true),
    );

    const subtaskRows = h("div", { class: "tasks-subtasks" });
    for (const sub of item.subtasks) {
      const subCb = h("input", {
        type: "checkbox",
        onchange: () => void runMutation(() => TasksBridge.toggleSubtask(item.id, sub.id)),
      }) as HTMLInputElement;
      subCb.checked = sub.done;
      subtaskRows.append(
        h(
          "div",
          { class: sub.done ? "tasks-subtask done" : "tasks-subtask" },
          h("label", { class: "tasks-subtask-label" }, subCb, h("span", { text: sub.text })),
          button("×", () => void runMutation(() => TasksBridge.removeSubtask(item.id, sub.id)), "", true),
        ),
      );
    }

    let editPanel: HTMLElement | null = null;
    if (isEditing) {
      const titleField = h("input", {
        type: "text",
        class: "hub-input",
        value: item.text,
        spellcheck: "false",
      }) as HTMLInputElement;
      const dateField = h("input", {
        type: "date",
        class: "hub-input tasks-inline-date",
        value: item.due_date ?? "",
      }) as HTMLInputElement;
      const timeField = h("input", {
        type: "time",
        class: "hub-input tasks-inline-time",
        value: item.due_time ?? "",
      }) as HTMLInputElement;
      const prioField = h(
        "select",
        { class: "hub-select" },
        ...(["low", "normal", "high", "urgent"] as TodoPriority[]).map((p) =>
          h("option", { value: p, text: PRIORITY_LABELS[p] }),
        ),
      ) as HTMLSelectElement;
      prioField.value = item.priority;

      const alertField = h(
        "select",
        { class: "hub-select" },
        h("option", { value: "0", text: "Na hora" }),
        h("option", { value: "5", text: "5 min antes" }),
        h("option", { value: "20", text: "20 min antes" }),
        h("option", { value: "60", text: "1 h antes" }),
      ) as HTMLSelectElement;
      alertField.value = String(item.alert_minutes);

      const quickDates = h(
        "div",
        { class: "tasks-shortcuts-row" },
        ...snap.shortcuts.enabled_date_shortcuts.map((k: TodoDateShortcut) =>
          button(DATE_SHORTCUT_LABELS[k], () => {
            dateField.value = dateForShortcut(k);
          }, "", true),
        ),
        button("Sem data", () => {
          dateField.value = "";
          timeField.value = "";
        }, "", true),
      );
      const quickTimes = h(
        "div",
        { class: "tasks-shortcuts-row" },
        ...snap.shortcuts.time_shortcuts_minutes.map((m) =>
          button(timeChipLabel(m), () => {
            if (!dateField.value) dateField.value = dateForShortcut("today");
            timeField.value = formatTimeMinutes(m);
          }, "", true),
        ),
        button("Sem hora", () => {
          timeField.value = "";
        }, "", true),
      );

      const subInput = h("input", {
        type: "text",
        class: "hub-input",
        placeholder: "Nova subtarefa…",
        spellcheck: "false",
      }) as HTMLInputElement;
      const addSubBtn = button("+ Subtarefa", () => {
        const txt = subInput.value.trim();
        if (!txt) return;
        subInput.value = "";
        void runMutation(() => TasksBridge.addSubtask(item.id, txt));
      }, "", true);
      subInput.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          addSubBtn.click();
        }
      });

      const saveEditBtn = button("Salvar alterações", () => {
        const nextText = titleField.value.trim();
        if (!nextText) return;
        const nextDate = dateField.value || null;
        const nextTime = nextDate && timeField.value ? timeField.value : null;
        void runMutation(async () => {
          await TasksBridge.update({
            id: item.id,
            text: nextText,
            due_date: nextDate,
            due_time: nextTime,
            alert_minutes: Number(alertField.value) || 0,
            priority: prioField.value as TodoPriority,
            clear_snooze: true,
          });
          editingId = null;
        });
      }, "primary", true);

      editPanel = h(
        "div",
        { class: "tasks-edit-panel" },
        titleField,
        quickDates,
        quickTimes,
        h("div", { class: "tasks-inline-fields" }, dateField, timeField, prioField, alertField, saveEditBtn),
        h("div", { class: "tasks-subtask-add" }, subInput, addSubBtn),
      );
    }

    const row = h(
      "div",
      {
        class: [
          "tasks-item",
          item.done ? "done" : "",
          focusTaskId === item.id ? "focused" : "",
        ].filter(Boolean).join(" "),
        "data-task-id": item.id,
      },
      h(
        "div",
        { class: "tasks-item-top" },
        check,
        h(
          "div",
          { class: "tasks-item-main" },
          h("div", { class: "tasks-item-title", text: item.text }),
          meta,
        ),
        actionsRow,
      ),
      item.subtasks.length > 0 ? subtaskRows : null,
      editPanel,
    );
    return row;
  }

  function renderList() {
    clear(listSlot);
    const filtered = searchTodos(snap.items, query);
    const grouped = groupTasks(filtered);

    const sections: { key: TodoBucket; title: string; items: TodoItem[] }[] = [
      { key: "overdue", title: "Atrasadas", items: grouped.overdue },
      { key: "today", title: "Hoje", items: grouped.today },
      { key: "upcoming", title: "Próximas", items: grouped.upcoming },
      { key: "unscheduled", title: "Sem data", items: grouped.unscheduled },
      { key: "done", title: "Concluídas", items: grouped.done },
    ];

    const visibleSections = sections.filter((s) =>
      filter === "all"
        ? s.items.length > 0
        : s.key === filter,
    );

    if (visibleSections.length === 0 || visibleSections.every((s) => s.items.length === 0)) {
      listSlot.append(
        h(
          "div",
          { class: "hub-card hub-empty" },
          h("div", {
            text: query.trim()
              ? "Nenhuma tarefa encontrada para essa busca."
              : "Nenhuma tarefa por aqui ainda. Digite acima ou pressione Ctrl+Alt+K.",
          }),
        ),
      );
      return;
    }

    for (const sec of visibleSections) {
      if (sec.items.length === 0) continue;
      listSlot.append(
        h(
          "section",
          { class: "tasks-section" },
          h(
            "div",
            { class: "tasks-section-head" },
            h("span", { class: `hub-label tasks-sec-title ${sec.key}`, text: `${sec.title} (${sec.items.length})` }),
          ),
          h("div", { class: "hub-card tasks-group-card" }, ...sec.items.map(renderTaskCard)),
        ),
      );
    }

    if (focusTaskId) {
      const target = listSlot.querySelector(`[data-task-id="${focusTaskId}"]`);
      if (target && typeof (target as HTMLElement).scrollIntoView === "function") {
        (target as HTMLElement).scrollIntoView({ behavior: "smooth", block: "nearest" });
      }
    }
  }

  function renderFilters() {
    clear(filterSlot);
    filterSlot.append(
      segmented<FilterKind>(
        [
          ["all", "Todas"],
          ["today", "Hoje"],
          ["upcoming", "Próximas"],
          ["overdue", "Atrasadas"],
          ["unscheduled", "Sem data"],
          ["done", "Concluídas"],
        ],
        filter,
        (next) => {
          filter = next;
          renderList();
        },
      ),
    );
  }

  async function reload() {
    const fresh = await TasksBridge.state();
    if (fresh) {
      snap = fresh;
      recording = fresh.recording;
    }
    renderBanners();
    updateCaptureButtons();
    renderProposal();
    renderUndo();
    renderList();
  }

  async function runMutation(op: () => Promise<unknown>) {
    statusError = "";
    try {
      await op();
      await reload();
    } catch (err) {
      statusError = String(err).replace(/^Error:\s*/, "");
      updateCaptureButtons();
    }
  }

  async function handleDelete(id: string) {
    statusError = "";
    try {
      undoItem = await TasksBridge.delete(id);
      if (editingId === id) editingId = null;
      await reload();
    } catch (err) {
      statusError = String(err).replace(/^Error:\s*/, "");
      updateCaptureButtons();
    }
  }

  async function restoreRemoved(id: string) {
    undoItem = null;
    await runMutation(() => TasksBridge.restore(id));
  }

  async function handleProposeText() {
    const raw = captureInput.value.trim();
    if (!raw || busy || recording) return;
    busy = true;
    statusError = "";
    updateCaptureButtons();
    try {
      draftProposal = await TasksBridge.proposeFromText(raw);
      captureInput.value = "";
      renderProposal();
    } catch (err) {
      statusError = String(err).replace(/^Error:\s*/, "");
    } finally {
      busy = false;
      updateCaptureButtons();
    }
  }

  async function handleToggleVoice() {
    if (busy) return;
    statusError = "";
    if (!recording) {
      try {
        await TasksBridge.voiceStart();
        recording = true;
      } catch (err) {
        statusError = String(err).replace(/^Error:\s*/, "");
      }
      updateCaptureButtons();
      return;
    }

    busy = true;
    updateCaptureButtons();
    try {
      draftProposal = await TasksBridge.voiceStop();
      recording = false;
      renderProposal();
    } catch (err) {
      recording = false;
      statusError = String(err).replace(/^Error:\s*/, "");
    } finally {
      busy = false;
      updateCaptureButtons();
    }
  }

  captureInput.addEventListener("input", updateCaptureButtons);
  captureInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleProposeText();
    }
  });
  searchInput.addEventListener("input", () => {
    query = searchInput.value;
    renderList();
  });

  clear(container);
  container.append(
    h(
      "div",
      { class: "tasks-page" },
      h(
        "div",
        { class: "hub-card tasks-capture-card" },
        h(
          "div",
          { class: "tasks-capture-row" },
          captureInput,
          micBtn,
          proposeBtn,
        ),
        h(
          "div",
          { class: "tasks-privacy-caption", text: "Armazenamento 100% local (Todo/todo.json) · Atalho global Ctrl+Alt+K · Datas ambíguas sempre pedem confirmação." },
        ),
        errorBanner,
        statusBanner,
      ),
      proposalSlot,
      undoSlot,
      h(
        "div",
        { class: "tasks-toolbar" },
        filterSlot,
        searchInput,
      ),
      listSlot,
    ),
  );

  renderBanners();
  updateCaptureButtons();
  renderProposal();
  renderFilters();
  renderList();

  const offChanged = await onEvent("tasks-changed", () => void reload());
  const offVoice = await onEvent<boolean>("tasks-voice-state", (on) => {
    recording = Boolean(on);
    updateCaptureButtons();
  });
  const offFocus = await onEvent<string>("task-focus", (id) => {
    focusTaskId = id;
    renderList();
  });
  activeCleanup = () => {
    offChanged();
    offVoice();
    offFocus();
  };
}
