// Página Tarefas do Hub (L4 + Redesign Editorial Desktop):
// Lista limpa em Master-Detail responsivo com barra de captura, filtros sem acento,
// painel inspetor dedicado para propostas da IA ("IA propõe, usuário confirma") e
// edição completa de tarefas sem vazar controles comprimidos da Ilha.

import { clear, h } from "../../views/dom";
import {
  GoogleBridge,
  TasksBridge,
  onEvent,
  type GoogleCalendarEvent,
  type GoogleStatus,
} from "../../core/bridge";
import { Sound } from "../../core/sound";
import {
  DATE_SHORTCUT_LABELS,
  DEFAULT_SHORTCUT_SETTINGS,
  PRIORITY_LABELS,
  buildRecurrence,
  dateForShortcut,
  formatCalendarButtonLabel,
  formatClockButtonLabel,
  formatDueLabel,
  formatTimeMinutes,
  groupTasks,
  recurrenceLabel,
  searchTodos,
  timeChipLabel,
  type TaskSyncState,
  type TasksSnapshot,
  type TodoBucket,
  type TodoDateShortcut,
  type TodoItem,
  type TodoPriority,
  type TodoProposal,
  type TodoShortcutSettings,
} from "../../core/tasks";
import type { HubContext } from "../shell";
import { button, icon, segmented } from "../ui";
import { createCalendarPopover, createClockPopover } from "../schedule-picker";

type FilterKind = "all" | TodoBucket;

let activeCleanup: (() => void) | null = null;

function formatSyncTimestamp(iso: string | null | undefined): string {
  if (!iso) return "";
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return iso;
  return parsed.toLocaleString("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function safeSyncError(raw: string | null | undefined): string {
  if (!raw) return "";
  return raw.trim().replace(/\s+/g, " ").slice(0, 120);
}

function emptySnapshot(): TasksSnapshot {
  return {
    items: [],
    shortcuts: { ...DEFAULT_SHORTCUT_SETTINGS },
    windows_notifications_available: false,
    voice_ready: false,
    recording: false,
    quarantined_file: null,
    sync: {
      state: "local_only",
      paired: false,
      device_id: "",
      pending_outbox: 0,
      pending_peer: null,
      last_synced_at: null,
      last_error: null,
      quarantined_count: 0,
      poison_count: 0,
      revoked: false,
      conflicts: [],
      unsynced_item_ids: [],
    },
  };
}

function setElementHidden(el: HTMLElement, hidden: boolean) {
  el.hidden = hidden;
  if (hidden) el.setAttribute("hidden", "");
  else el.removeAttribute("hidden");
}

interface ScheduleFieldValues {
  title: string;
  dueDate: string | null;
  dueTime: string | null;
  priority: TodoPriority;
  alertMinutes: number;
  recurrenceFreq: string;
}

interface ScheduleFieldsHandle {
  el: HTMLElement;
  titleInput: HTMLInputElement;
  getValues(): ScheduleFieldValues;
  setShortcuts(next: TodoShortcutSettings): void;
  hasOpenPopover(): boolean;
  closePopover(restoreFocus?: boolean): void;
  focusTitle(): void;
}

function createScheduleFields(
  initial: ScheduleFieldValues,
  initialShortcuts: TodoShortcutSettings,
  opts: {
    onSubmitShortcut: () => void;
    onCancelShortcut: () => void;
    onTitleChange?: (title: string) => void;
    onChange?: (values: ScheduleFieldValues) => void;
    onSavePreset?: (minutes: number) => Promise<TodoShortcutSettings>;
    onRemovePreset?: (minutes: number) => Promise<TodoShortcutSettings>;
  },
): ScheduleFieldsHandle {
  let shortcuts = initialShortcuts;
  let title = initial.title;
  let dueDate = initial.dueDate;
  let dueTime = initial.dueTime;
  let priority = initial.priority;
  let alertMinutes = initial.alertMinutes;
  let recurrenceFreq = initial.recurrenceFreq;
  let activePicker: "calendar" | "time" | "preset" | null = null;
  let lastTriggerBtn: HTMLButtonElement | null = null;

  const titleInput = h("input", {
    type: "text",
    class: "hub-input",
    value: title,
    placeholder: "Título da tarefa",
    "aria-label": "Título da tarefa",
    spellcheck: "false",
  }) as HTMLInputElement;
  titleInput.value = title;

  const calendarBtnLabel = h("span", { class: "tasks-picker-btn-label" });
  const calendarBtn = h(
    "button",
    {
      type: "button",
      class: "hub-btn tasks-picker-btn",
      "aria-expanded": "false",
      onclick: () => togglePicker("calendar", calendarBtn),
    },
    icon("calendar"),
    calendarBtnLabel,
  ) as HTMLButtonElement;

  const clockBtnLabel = h("span", { class: "tasks-picker-btn-label" });
  const clockBtn = h(
    "button",
    {
      type: "button",
      class: "hub-btn tasks-picker-btn",
      "aria-expanded": "false",
      onclick: () => togglePicker("time", clockBtn),
    },
    icon("clock"),
    clockBtnLabel,
  ) as HTMLButtonElement;

  const addPresetBtn = h(
    "button",
    {
      type: "button",
      class: "hub-btn sm tasks-preset-add",
      "aria-label": "Criar horário predefinido",
      "aria-expanded": "false",
      onclick: () => togglePicker("preset", addPresetBtn),
    },
    icon("plus"),
    h("span", { text: "Predefinido" }),
  ) as HTMLButtonElement;

  const prioSelect = h(
    "select",
    { class: "hub-select", "aria-label": "Prioridade" },
    ...(["low", "normal", "high", "urgent"] as TodoPriority[]).map((p) =>
      h("option", { value: p, text: PRIORITY_LABELS[p] }),
    ),
  ) as HTMLSelectElement;
  prioSelect.value = priority;

  const alertSelect = h(
    "select",
    { class: "hub-select", "aria-label": "Pré-alerta" },
    h("option", { value: "0", text: "Na hora" }),
    h("option", { value: "5", text: "5 min antes" }),
    h("option", { value: "20", text: "20 min antes" }),
    h("option", { value: "60", text: "1 h antes" }),
  ) as HTMLSelectElement;
  alertSelect.value = String(alertMinutes);

  const alertHint = h("span", {
    class: "tasks-field-hint",
    text: "Defina dia e horário para ativar o aviso",
    hidden: true,
  });

  const recSelect = h(
    "select",
    { class: "hub-select", "aria-label": "Repetição" },
    h("option", { value: "", text: "Não repetir" }),
    h("option", { value: "daily", text: "Diária" }),
    h("option", { value: "weekly", text: "Semanal" }),
    h("option", { value: "monthly", text: "Mensal" }),
  ) as HTMLSelectElement;
  recSelect.value = recurrenceFreq;

  const dateChipsRow = h("div", { class: "tasks-chip-row" });
  const dateButtons = new Map<TodoDateShortcut | "none", HTMLButtonElement>();
  const timeChipsRow = h("div", { class: "tasks-chip-row" });
  const timeButtons = new Map<string, HTMLButtonElement>();

  const datePopoverSlot = h("div", { class: "tasks-picker-slot", hidden: true });
  setElementHidden(datePopoverSlot, true);
  const timePopoverSlot = h("div", { class: "tasks-picker-slot", hidden: true });
  setElementHidden(timePopoverSlot, true);

  function snapshotValues(): ScheduleFieldValues {
    return {
      title: titleInput.value.trim(),
      dueDate,
      dueTime: dueDate ? dueTime : null,
      priority,
      alertMinutes,
      recurrenceFreq,
    };
  }

  function emitChange() {
    opts.onChange?.(snapshotValues());
  }

  function rebuildDateChips() {
    clear(dateChipsRow);
    dateButtons.clear();
    for (const key of shortcuts.enabled_date_shortcuts) {
      const btn = h("button", {
        type: "button",
        class: "tasks-chip",
        text: DATE_SHORTCUT_LABELS[key],
        "aria-pressed": "false",
        onclick: () => {
          dueDate = dateForShortcut(key);
          syncUI();
          emitChange();
        },
      }) as HTMLButtonElement;
      dateButtons.set(key, btn);
      dateChipsRow.append(btn);
    }
    const noDateBtn = h("button", {
      type: "button",
      class: "tasks-chip tasks-chip-neutral",
      text: "Sem data",
      "aria-pressed": "false",
      onclick: () => {
        dueDate = null;
        dueTime = null;
        syncUI();
        emitChange();
      },
    }) as HTMLButtonElement;
    dateButtons.set("none", noDateBtn);
    dateChipsRow.append(noDateBtn);
  }

  function rebuildTimeChips() {
    clear(timeChipsRow);
    timeButtons.clear();
    for (const mins of shortcuts.time_shortcuts_minutes) {
      const hhmm = formatTimeMinutes(mins);
      const btn = h("button", {
        type: "button",
        class: "tasks-chip",
        text: timeChipLabel(mins),
        "aria-pressed": "false",
        onclick: () => {
          if (!dueDate) {
            dueDate = dateForShortcut("today");
          }
          dueTime = hhmm;
          syncUI();
          emitChange();
        },
      }) as HTMLButtonElement;
      timeButtons.set(hhmm, btn);
      timeChipsRow.append(btn);
    }
    const noTimeBtn = h("button", {
      type: "button",
      class: "tasks-chip tasks-chip-neutral",
      text: "Sem hora",
      "aria-pressed": "false",
      onclick: () => {
        dueTime = null;
        syncUI();
        emitChange();
      },
    }) as HTMLButtonElement;
    timeButtons.set("none", noTimeBtn);
    timeChipsRow.append(noTimeBtn, addPresetBtn);
  }

  function closePopover(restoreFocus = false) {
    activePicker = null;
    clear(datePopoverSlot);
    clear(timePopoverSlot);
    setElementHidden(datePopoverSlot, true);
    setElementHidden(timePopoverSlot, true);
    calendarBtn.setAttribute("aria-expanded", "false");
    clockBtn.setAttribute("aria-expanded", "false");
    addPresetBtn.setAttribute("aria-expanded", "false");
    if (restoreFocus && lastTriggerBtn && lastTriggerBtn.isConnected) {
      lastTriggerBtn.focus();
    }
    lastTriggerBtn = null;
  }

  function openCalendar() {
    const pop = createCalendarPopover({
      initialDate: dueDate,
      onSelect: (iso) => {
        dueDate = iso;
        if (!dueDate) dueTime = null;
        syncUI();
        emitChange();
      },
      onClose: () => closePopover(true),
    });
    clear(datePopoverSlot);
    datePopoverSlot.append(pop);
    setElementHidden(datePopoverSlot, false);
    calendarBtn.setAttribute("aria-expanded", "true");
    scrollSlotInsideInspector(datePopoverSlot);
  }

  function scrollSlotInsideInspector(slot: HTMLElement) {
    const body = typeof el.closest === "function"
      ? (el.closest(".tasks-inspector-body") as HTMLElement | null)
      : null;
    if (!body) return;
    requestAnimationFrame(() => {
      const slotBottom = slot.offsetTop + slot.offsetHeight;
      if (slotBottom > body.scrollTop + body.clientHeight) {
        body.scrollTop = Math.max(0, slot.offsetTop - 12);
      }
    });
  }

  function openClock(mode: "time" | "preset") {
    const pop = createClockPopover({
      mode,
      initialTime: dueTime,
      presets: shortcuts.time_shortcuts_minutes,
      onApplyTime: (hhmm) => {
        dueTime = hhmm;
        if (dueTime && !dueDate) dueDate = dateForShortcut("today");
        syncUI();
        emitChange();
      },
      onSavePreset: async (minutes) => {
        if (!opts.onSavePreset) return;
        const next = await opts.onSavePreset(minutes);
        shortcuts = next;
        rebuildTimeChips();
        if (mode === "preset") {
          dueTime = formatTimeMinutes(minutes);
          if (!dueDate) dueDate = dateForShortcut("today");
        }
        syncUI();
        emitChange();
      },
      onRemovePreset: async (minutes) => {
        if (!opts.onRemovePreset) return;
        const next = await opts.onRemovePreset(minutes);
        shortcuts = next;
        rebuildTimeChips();
        syncUI();
      },
      onClose: () => closePopover(true),
    });
    clear(timePopoverSlot);
    timePopoverSlot.append(pop);
    setElementHidden(timePopoverSlot, false);
    if (mode === "time") clockBtn.setAttribute("aria-expanded", "true");
    else addPresetBtn.setAttribute("aria-expanded", "true");
    scrollSlotInsideInspector(timePopoverSlot);
  }

  function togglePicker(kind: "calendar" | "time" | "preset", trigger: HTMLButtonElement) {
    if (activePicker === kind) {
      closePopover(false);
      return;
    }
    closePopover(false);
    activePicker = kind;
    lastTriggerBtn = trigger;
    if (kind === "calendar") openCalendar();
    else openClock(kind);
  }

  function syncUI() {
    for (const [k, btn] of dateButtons) {
      const active = k === "none" ? dueDate === null : dueDate === dateForShortcut(k);
      btn.classList.toggle("on", active);
      btn.setAttribute("aria-pressed", String(active));
    }
    for (const [k, btn] of timeButtons) {
      const active = k === "none" ? dueTime === null : dueTime === k;
      btn.classList.toggle("on", active);
      btn.setAttribute("aria-pressed", String(active));
    }

    const calLabel = formatCalendarButtonLabel(dueDate);
    calendarBtnLabel.textContent = calLabel;
    calendarBtn.classList.toggle("on", dueDate !== null);
    calendarBtn.setAttribute("aria-label", dueDate ? `Abrir calendário (${calLabel})` : "Abrir calendário");

    const clkLabel = formatClockButtonLabel(dueTime);
    clockBtnLabel.textContent = clkLabel;
    clockBtn.classList.toggle("on", dueTime !== null);
    clockBtn.setAttribute("aria-label", dueTime ? `Abrir relógio (${clkLabel})` : "Abrir relógio");

    const alertDisabled = !dueDate || !dueTime;
    alertSelect.disabled = alertDisabled;
    setElementHidden(alertHint, !alertDisabled);
  }

  titleInput.addEventListener("input", () => {
    title = titleInput.value;
    opts.onTitleChange?.(title);
    emitChange();
  });
  titleInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      opts.onSubmitShortcut();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (activePicker) closePopover(true);
      else opts.onCancelShortcut();
    }
  });
  prioSelect.addEventListener("change", () => {
    priority = prioSelect.value as TodoPriority;
    emitChange();
  });
  alertSelect.addEventListener("change", () => {
    alertMinutes = Number(alertSelect.value) || 0;
    emitChange();
  });
  recSelect.addEventListener("change", () => {
    recurrenceFreq = recSelect.value;
    emitChange();
  });

  rebuildDateChips();
  rebuildTimeChips();
  syncUI();

  const el = h(
    "div",
    { class: "tasks-inspector-section" },
    h(
      "div",
      { class: "tasks-field-stack" },
      h("span", { class: "tasks-field-label", text: "Título" }),
      titleInput,
    ),
    h(
      "div",
      { class: "tasks-field-stack" },
      h(
        "div",
        { class: "tasks-schedule-head" },
        h("span", { class: "tasks-field-label", text: "Dia" }),
        calendarBtn,
      ),
      dateChipsRow,
      datePopoverSlot,
    ),
    h(
      "div",
      { class: "tasks-field-stack" },
      h(
        "div",
        { class: "tasks-schedule-head" },
        h("span", { class: "tasks-field-label", text: "Horário" }),
        clockBtn,
      ),
      timeChipsRow,
      timePopoverSlot,
    ),
    h(
      "div",
      { class: "tasks-inspector-grid" },
      h("label", { class: "tasks-field-stack" }, h("span", { class: "tasks-field-label", text: "Prioridade" }), prioSelect),
      h(
        "label",
        { class: "tasks-field-stack" },
        h("span", { class: "tasks-field-label", text: "Pré-alerta" }),
        alertSelect,
        alertHint,
      ),
    ),
    h(
      "label",
      { class: "tasks-field-stack" },
      h("span", { class: "tasks-field-label", text: "Repetição" }),
      recSelect,
    ),
  );

  return {
    el,
    titleInput,
    getValues: snapshotValues,
    setShortcuts(next) {
      shortcuts = next;
      rebuildTimeChips();
      syncUI();
    },
    hasOpenPopover: () => activePicker !== null,
    closePopover,
    focusTitle() {
      titleInput.focus();
      titleInput.select();
    },
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
  let draftTargetKey: string | null = null;
  let activeDraft: ScheduleFieldValues | null = null;
  let activeFieldsHandle: ScheduleFieldsHandle | null = null;
  let activeSubmit: (() => void) | null = null;
  let lastInvokerKey: { kind: "capture" } | { kind: "edit"; taskId: string } | null = null;
  let undoItem: TodoItem | null = null;
  let statusError = "";
  let busy = false;
  let syncing = false;
  let recording = snap.recording;

  const captureInput = h("input", {
    type: "text",
    class: "hub-input tasks-capture-input",
    placeholder: "Nova tarefa… ex.: ligar pro contador amanhã 9h",
    "aria-label": "Nova tarefa",
    spellcheck: "false",
  }) as HTMLInputElement;
  captureInput.value = "";

  const searchInput = h("input", {
    type: "search",
    class: "hub-input tasks-search-input",
    placeholder: "Buscar tarefas…",
    "aria-label": "Buscar tarefas",
    value: query,
    spellcheck: "false",
  }) as HTMLInputElement;
  searchInput.value = query;

  const proposeBtn = button("Propor", () => void handleProposeText(), "primary");
  const micBtn = button(recording ? "Concluir voz" : "Ditar", () => void handleToggleVoice());

  const syncBar = h("div", { class: "tasks-sync-bar", role: "status", "aria-live": "polite" });
  const errorBanner = h("div", { class: "tasks-banner warn", role: "alert", hidden: true });
  const statusBanner = h("div", { class: "tasks-banner info", role: "status", "aria-live": "polite", hidden: true });
  const undoSlot = h("div", { role: "status", "aria-live": "polite" });
  const filterSlot = h("div");
  const listSlot = h("div", { class: "tasks-sections" });
  const inspectorSlot = h("aside", { class: "tasks-inspector", hidden: true, tabindex: "-1" });
  const googleSlot = h("section", { class: "tasks-google-panel", hidden: true });
  const workspaceEl = h("div", { class: "tasks-workspace" });
  let syncNowBtn: HTMLButtonElement | null = null;
  let googleStatus: GoogleStatus | null = null;
  let googleEvents: GoogleCalendarEvent[] = [];
  let googleBusy = false;
  let googleNotice = "";
  let googleErr = "";
  let googleConfirmRestore = false;

  function formatCalendarTime(ev: GoogleCalendarEvent): string {
    if (ev.allDay) {
      const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(ev.start);
      return m ? `${m[3]}/${m[2]} · Dia inteiro` : `${ev.start} · Dia inteiro`;
    }
    const parsed = new Date(ev.start);
    if (Number.isNaN(parsed.getTime())) return ev.start;
    const d = String(parsed.getDate()).padStart(2, "0");
    const mo = String(parsed.getMonth() + 1).padStart(2, "0");
    const hh = String(parsed.getHours()).padStart(2, "0");
    const mm = String(parsed.getMinutes()).padStart(2, "0");
    return `${d}/${mo} · ${hh}:${mm}`;
  }

  function renderGooglePanel() {
    clear(googleSlot);
    if (!googleStatus) {
      setElementHidden(googleSlot, true);
      return;
    }
    setElementHidden(googleSlot, false);

    const headBadge = googleStatus.connected
      ? h("span", { class: "tasks-sync-pill synced", text: googleStatus.email ?? "Google conectado" })
      : h("span", { class: "tasks-sync-pill local", text: "Google desconectado" });
    const head = h(
      "div",
      { class: "tasks-google-head" },
      h("div", { class: "tasks-google-title-wrap" }, h("h3", { text: "Google Calendar e Backup" }), headBadge),
    );

    if (!googleStatus.connected) {
      googleSlot.append(
        head,
        h("p", {
          class: "tasks-privacy-caption",
          text: googleStatus.configured
            ? "Projeto OAuth configurado. Entre com sua conta Google em Ajustes → Integrações para ver sua agenda e ativar backup."
            : "Configure sua conta Google em Ajustes → Integrações para ver eventos do Calendar e fazer backup em Segunda Mente/Lumen-PC.",
        }),
      );
      return;
    }

    const actions = h("div", { class: "tasks-google-actions" });
    const refreshEventsBtn = button("Atualizar agenda", () => void refreshGoogleState());
    refreshEventsBtn.disabled = googleBusy;
    const backupBtn = button(googleBusy ? "Sincronizando…" : "Fazer backup agora", () => {
      void (async () => {
        googleBusy = true;
        googleErr = "";
        googleNotice = "";
        googleConfirmRestore = false;
        renderGooglePanel();
        try {
          const res = await GoogleBridge.backupNow();
          googleStatus = await GoogleBridge.status();
          googleNotice = `Backup salvo em ${res.driveFolder} (${res.notesCount} notas).`;
        } catch (err) {
          googleErr = String(err).replace(/^Error:\s*/, "");
        } finally {
          googleBusy = false;
          renderGooglePanel();
        }
      })();
    });
    backupBtn.disabled = googleBusy;

    const restoreBtn = button("Restaurar backup", () => {
      googleConfirmRestore = true;
      googleNotice = "";
      googleErr = "";
      renderGooglePanel();
    });
    restoreBtn.disabled = googleBusy;
    actions.append(refreshEventsBtn, backupBtn, restoreBtn);
    head.append(actions);

    const lastBakLabel = googleStatus.lastBackupAt
      ? new Date(googleStatus.lastBackupAt).toLocaleString("pt-BR")
      : "Ainda não realizado";
    const metaRow = h("div", {
      class: "tasks-privacy-caption",
      text: `Drive: ${googleStatus.driveFolder} · Backup semanal: ${googleStatus.weeklyBackupEnabled ? "ativado" : "desativado"} · Último backup: ${lastBakLabel}`,
    });

    const eventsWrap = h("div", { class: "tasks-google-events" });
    if (googleEvents.length === 0) {
      eventsWrap.append(h("div", { class: "tasks-privacy-caption", text: "Nenhum evento próximo nos próximos 14 dias." }));
    } else {
      for (const ev of googleEvents) {
        eventsWrap.append(
          h(
            "div",
            { class: "tasks-google-event-row" },
            h("span", { class: "tasks-google-event-time", text: formatCalendarTime(ev) }),
            h("span", { class: "tasks-google-event-name", text: ev.summary }),
          ),
        );
      }
    }

    let confirmRestoreBox: HTMLElement | null = null;
    if (googleConfirmRestore) {
      const confirmBtn = button(
        "Confirmar restauração",
        () => {
          void (async () => {
            googleBusy = true;
            googleErr = "";
            googleNotice = "";
            renderGooglePanel();
            try {
              const res = await GoogleBridge.restoreBackup();
              googleConfirmRestore = false;
              await reload();
              googleStatus = await GoogleBridge.status();
              googleNotice = `Backup restaurado (${res.notesRestored} notas). Snapshot local salvo antes.`;
            } catch (err) {
              googleErr = String(err).replace(/^Error:\s*/, "");
            } finally {
              googleBusy = false;
              renderGooglePanel();
            }
          })();
        },
        "primary",
      );
      confirmBtn.disabled = googleBusy;
      const cancelBtn = button("Cancelar", () => {
        googleConfirmRestore = false;
        renderGooglePanel();
      });
      cancelBtn.disabled = googleBusy;
      confirmRestoreBox = h(
        "div",
        { class: "tasks-google-confirm" },
        h("span", {
          class: "tasks-privacy-caption",
          text: "Restaurar sobrescreve tarefas, notas (.json) e personas com a cópia da nuvem (um snapshot de segurança será gravado antes).",
        }),
        h("div", { class: "tasks-google-actions" }, confirmBtn, cancelBtn),
      );
    }

    const noticeEl = googleNotice ? h("div", { class: "tasks-banner info", text: googleNotice }) : null;
    const errEl = googleErr ? h("div", { class: "tasks-banner warn", text: googleErr }) : null;
    googleSlot.append(head, metaRow, eventsWrap);
    if (confirmRestoreBox) googleSlot.append(confirmRestoreBox);
    if (noticeEl) googleSlot.append(noticeEl);
    if (errEl) googleSlot.append(errEl);
  }

  async function refreshGoogleState() {
    try {
      const st = await GoogleBridge.status();
      googleStatus = st;
      if (st.connected) {
        googleEvents = await GoogleBridge.listCalendarEvents({ maxResults: 6 });
      } else {
        googleEvents = [];
      }
      renderGooglePanel();
      if (editingId) renderInspector();
    } catch {
      // Sem backend Google no ambiente de teste isolado ou falha transitória.
    }
  }

  function updateCaptureButtons() {
    proposeBtn.disabled = busy || recording || (captureInput.value ?? "").trim().length === 0;
    micBtn.textContent = recording ? "Concluir voz" : "Ditar";
    micBtn.classList.toggle("primary", recording);
    micBtn.disabled = busy;
    captureInput.disabled = busy || recording;
    if (syncNowBtn) syncNowBtn.disabled = syncing || busy;
    errorBanner.textContent = statusError;
    setElementHidden(errorBanner, !statusError);
  }

  function renderSyncBar() {
    clear(syncBar);
    syncNowBtn = null;
    const sync = snap.sync;
    const state: TaskSyncState = sync?.state ?? "local_only";
    const isSynced = state === "synced";
    const pillClass =
      state === "synced"
        ? "tasks-sync-pill synced"
        : state === "error" || state === "revoked"
          ? "tasks-sync-pill warn"
          : state === "waiting_phone" || state === "saved_on_pc"
            ? "tasks-sync-pill pending"
            : "tasks-sync-pill local";

    let detailText = " · celular não pareado";
    if (state === "synced") {
      const formattedAt = formatSyncTimestamp(sync?.last_synced_at);
      detailText = formattedAt ? ` · última confirmação ${formattedAt}` : "";
    } else if (state === "saved_on_pc") {
      const n = sync?.pending_outbox ?? 0;
      detailText = ` · ${n} ${n === 1 ? "alteração pendente" : "alterações pendentes"} para enviar`;
    } else if (state === "waiting_phone") {
      detailText = " · aguardando o celular abrir a caixa";
    } else if (state === "revoked") {
      detailText = " · pareamento revogado (pareie novamente em Configurações)";
    } else if (state === "error") {
      const safeErr = safeSyncError(sync?.last_error);
      detailText = safeErr
        ? ` · falha na última sincronização (${safeErr})`
        : " · falha na última sincronização";
    }

    if (sync?.paired) {
      syncNowBtn = button(syncing ? "Sincronizando…" : "Sincronizar agora", () => void handleSyncNow(), "", true);
      syncNowBtn.classList.add("tasks-sync-now-btn");
      syncNowBtn.setAttribute("data-testid", "tasks-sync-now");
      syncNowBtn.disabled = syncing || busy;
    }

    syncBar.append(
      h(
        "div",
        { class: "tasks-sync-row" },
        h(
          "div",
          { class: "tasks-sync-status" },
          h("span", { class: pillClass, text: isSynced ? "Sincronizado" : "Salvo neste PC" }),
          detailText ? h("span", { class: "tasks-sync-detail", text: detailText }) : null,
        ),
        syncNowBtn,
      ),
    );

    const notices: string[] = [];
    if (sync && sync.quarantined_count > 0) {
      notices.push(
        sync.quarantined_count === 1
          ? "1 atualização em quarentena (versão mais nova)"
          : `${sync.quarantined_count} atualizações em quarentena (versão mais nova)`,
      );
    }
    if (sync && sync.poison_count > 0) {
      notices.push(
        sync.poison_count === 1
          ? "1 mensagem inválida ignorada"
          : `${sync.poison_count} mensagens inválidas ignoradas`,
      );
    }
    if (notices.length > 0) {
      syncBar.append(h("div", { class: "tasks-sync-notice", text: notices.join(" · ") }));
    }
  }

  function renderBanners() {
    renderSyncBar();
    const notes: string[] = [];
    if (snap.quarantined_file) {
      notes.push(`Arquivo todo.json corrompido movido para quarentena: ${snap.quarantined_file}.`);
    }
    if (!snap.windows_notifications_available) {
      notes.push("Notificações nativas do Windows indisponíveis nesta sessão — avisos abrem direto na Ilha.");
    }
    statusBanner.textContent = notes.join(" ");
    setElementHidden(statusBanner, notes.length === 0);
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

  async function saveTimePreset(minutes: number): Promise<TodoShortcutSettings> {
    const base = snap.shortcuts ?? { ...DEFAULT_SHORTCUT_SETTINGS };
    const existing = base.time_shortcuts_minutes ?? [];
    if (existing.includes(minutes)) return base;
    const nextList = [...existing, minutes].sort((a, b) => a - b);
    const saved = await TasksBridge.saveShortcutSettings({
      ...base,
      time_shortcuts_minutes: nextList,
    });
    snap = { ...snap, shortcuts: saved };
    return saved;
  }

  async function removeTimePreset(minutes: number): Promise<TodoShortcutSettings> {
    const base = snap.shortcuts ?? { ...DEFAULT_SHORTCUT_SETTINGS };
    const nextList = (base.time_shortcuts_minutes ?? []).filter((m) => m !== minutes);
    const saved = await TasksBridge.saveShortcutSettings({
      ...base,
      time_shortcuts_minutes: nextList,
    });
    snap = { ...snap, shortcuts: saved };
    return saved;
  }

  function closeInspector(restoreFocus = false) {
    const returnKey = restoreFocus ? lastInvokerKey : null;
    draftProposal = null;
    editingId = null;
    draftTargetKey = null;
    activeDraft = null;
    activeFieldsHandle = null;
    activeSubmit = null;
    lastInvokerKey = null;
    renderInspector();
    renderList();
    if (returnKey?.kind === "capture") {
      captureInput.focus();
    } else if (returnKey?.kind === "edit") {
      const btn = listSlot.querySelector(
        `[data-task-id="${returnKey.taskId}"] .tasks-edit-btn`,
      ) as HTMLElement | null;
      btn?.focus();
    }
  }

  function buildProposalInspector(proposal: TodoProposal, key: string): HTMLElement[] {
    let saving = false;
    const ambHints: string[] = [];
    if (proposal.date_state === "ambiguous") {
      ambHints.push(
        proposal.date_phrase
          ? `Data ambígua ("${proposal.date_phrase}") — confirme o dia`
          : "Data ambígua — escolha o dia",
      );
    }
    if (proposal.time_state === "ambiguous") {
      ambHints.push(
        proposal.time_phrase
          ? `Hora ambígua ("${proposal.time_phrase}") — escolha o horário`
          : "Hora ambígua — escolha o horário",
      );
    }

    const errBox = h("div", { class: "tasks-banner warn", role: "alert", hidden: true });
    setElementHidden(errBox, true);

    const confirmBtn = button("Confirmar tarefa", () => void submitProposal(), "primary");
    const discardBtn = button("Descartar", () => closeInspector(true));

    function syncSubmitState(titleValue: string) {
      confirmBtn.disabled = saving || titleValue.trim().length === 0;
      confirmBtn.textContent = saving ? "Salvando…" : "Confirmar tarefa";
    }

    const initialValues: ScheduleFieldValues =
      draftTargetKey === key && activeDraft
        ? activeDraft
        : {
            title: proposal.title,
            dueDate: proposal.date_state === "explicit" ? proposal.due_date : null,
            dueTime: proposal.time_state === "explicit" ? proposal.due_time : null,
            priority: proposal.priority ?? "normal",
            alertMinutes: snap.shortcuts.default_alert_minutes,
            recurrenceFreq: "",
          };
    draftTargetKey = key;
    activeDraft = { ...initialValues };

    const fields = createScheduleFields(initialValues, snap.shortcuts, {
      onSubmitShortcut: () => void submitProposal(),
      onCancelShortcut: () => closeInspector(true),
      onTitleChange: (val) => syncSubmitState(val),
      onChange: (vals) => {
        draftTargetKey = key;
        activeDraft = vals;
      },
      onSavePreset: saveTimePreset,
      onRemovePreset: removeTimePreset,
    });
    activeFieldsHandle = fields;
    activeSubmit = () => void submitProposal();

    async function submitProposal() {
      const vals = fields.getValues();
      if (!vals.title || saving) return;
      saving = true;
      setElementHidden(errBox, true);
      syncSubmitState(vals.title);
      try {
        const item = await TasksBridge.confirmProposal({
          text: vals.title,
          due_date: vals.dueDate,
          due_time: vals.dueTime,
          alert_minutes: vals.alertMinutes,
          priority: vals.priority,
          recurrence: buildRecurrence(vals.recurrenceFreq, vals.dueDate),
        });
        Sound.play("approve");
        draftProposal = null;
        draftTargetKey = null;
        activeDraft = null;
        activeFieldsHandle = null;
        activeSubmit = null;
        lastInvokerKey = null;
        focusTaskId = item.id;
        await reload();
      } catch (err) {
        errBox.textContent = String(err).replace(/^Error:\s*/, "");
        setElementHidden(errBox, false);
        Sound.play("error");
      } finally {
        saving = false;
        syncSubmitState(fields.titleInput.value);
      }
    }

    syncSubmitState(initialValues.title);

    const closeBtn = h("button", {
      type: "button",
      class: "hub-icon",
      title: "Descartar proposta",
      "aria-label": "Descartar proposta",
      onclick: () => closeInspector(true),
    }, icon("x"));

    const body = h(
      "div",
      { class: "tasks-inspector-body" },
      ambHints.length > 0
        ? h("div", { class: "tasks-inspector-alert", text: ambHints.join(" · ") })
        : h("div", { class: "tasks-privacy-caption", text: "Revise dia, horário e prioridade antes de gravar." }),
      fields.el,
      errBox,
    );

    return [
      h("div", { class: "tasks-inspector-head" }, h("h2", { text: "Confirmar proposta" }), closeBtn),
      body,
      h("div", { class: "tasks-inspector-actions" }, discardBtn, confirmBtn),
    ];
  }

  function buildEditInspector(item: TodoItem, key: string): HTMLElement[] {
    let saving = false;
    const errBox = h("div", { class: "tasks-banner warn", role: "alert", hidden: true });
    setElementHidden(errBox, true);

    const saveBtn = button("Salvar alterações", () => void submitEdit(), "primary");
    const cancelBtn = button("Cancelar", () => closeInspector(true));
    const deleteBtn = button("Excluir", () => void handleDelete(item.id), "danger");
    deleteBtn.classList.add("tasks-inspector-delete");
    const previewBtn = item.due_date
      ? button("Testar aviso", () => void runMutation(() => TasksBridge.previewReminder(item.id)), "", true)
      : null;

    function syncSaveState(titleValue: string) {
      saveBtn.disabled = saving || titleValue.trim().length === 0;
      saveBtn.textContent = saving ? "Salvando…" : "Salvar alterações";
    }

    const initialValues: ScheduleFieldValues =
      draftTargetKey === key && activeDraft
        ? activeDraft
        : {
            title: item.text,
            dueDate: item.due_date ?? null,
            dueTime: item.due_time ?? null,
            priority: item.priority,
            alertMinutes: item.alert_minutes,
            recurrenceFreq: item.recurrence?.frequency ?? "",
          };
    draftTargetKey = key;
    activeDraft = { ...initialValues };

    const fields = createScheduleFields(initialValues, snap.shortcuts, {
      onSubmitShortcut: () => void submitEdit(),
      onCancelShortcut: () => closeInspector(true),
      onTitleChange: (val) => syncSaveState(val),
      onChange: (vals) => {
        draftTargetKey = key;
        activeDraft = vals;
      },
      onSavePreset: saveTimePreset,
      onRemovePreset: removeTimePreset,
    });
    activeFieldsHandle = fields;
    activeSubmit = () => void submitEdit();

    async function submitEdit() {
      const vals = fields.getValues();
      if (!vals.title || saving) return;
      saving = true;
      setElementHidden(errBox, true);
      syncSaveState(vals.title);
      try {
        await TasksBridge.update({
          id: item.id,
          text: vals.title,
          due_date: vals.dueDate,
          due_time: vals.dueTime,
          alert_minutes: vals.alertMinutes,
          priority: vals.priority,
          recurrence: buildRecurrence(vals.recurrenceFreq, vals.dueDate),
          clear_snooze: true,
        });
        editingId = null;
        draftTargetKey = null;
        activeDraft = null;
        activeFieldsHandle = null;
        activeSubmit = null;
        lastInvokerKey = null;
        focusTaskId = item.id;
        await reload();
      } catch (err) {
        errBox.textContent = String(err).replace(/^Error:\s*/, "");
        setElementHidden(errBox, false);
      } finally {
        saving = false;
        syncSaveState(fields.titleInput.value);
      }
    }

    syncSaveState(initialValues.title);

    const subInput = h("input", {
      type: "text",
      class: "hub-input",
      placeholder: "Nova subtarefa…",
      "aria-label": "Nova subtarefa",
      spellcheck: "false",
    }) as HTMLInputElement;
    const addSubBtn = button(
      "+ Subtarefa",
      () => {
        const txt = subInput.value.trim();
        if (!txt) return;
        subInput.value = "";
        void runMutation(() => TasksBridge.addSubtask(item.id, txt));
      },
      "",
      true,
    );
    subInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        e.stopPropagation();
        addSubBtn.click();
      }
    });

    const subtasksList =
      item.subtasks.length > 0
        ? h(
            "div",
            { class: "tasks-inspector-subtasks" },
            ...item.subtasks.map((sub) => {
              const subCb = h("input", {
                type: "checkbox",
                "aria-label": `Subtarefa ${sub.text}`,
                onchange: () => void runMutation(() => TasksBridge.toggleSubtask(item.id, sub.id)),
              }) as HTMLInputElement;
              subCb.checked = sub.done;
              const removeBtn = button(
                "×",
                () => void runMutation(() => TasksBridge.removeSubtask(item.id, sub.id)),
                "",
                true,
              );
              removeBtn.setAttribute("aria-label", `Remover subtarefa ${sub.text}`);
              return h(
                "div",
                { class: sub.done ? "tasks-subtask done" : "tasks-subtask" },
                h("label", { class: "tasks-subtask-label" }, subCb, h("span", { text: sub.text })),
                removeBtn,
              );
            }),
          )
        : null;

    const closeBtn = h("button", {
      type: "button",
      class: "hub-icon",
      title: "Fechar editor",
      "aria-label": "Fechar editor",
      onclick: () => closeInspector(true),
    }, icon("x"));

    const calStatusMsg = h("div", { class: "tasks-privacy-caption", hidden: true });
    setElementHidden(calStatusMsg, true);
    const calConfirmHost = h("div", { class: "tasks-calendar-confirm-host", hidden: true });
    setElementHidden(calConfirmHost, true);

    const openCalPreviewBtn = googleStatus?.connected
      ? button("Agendar no Google Calendar", () => {
          clear(calConfirmHost);
          setElementHidden(errBox, true);
          const live = fields.getValues();
          if (!live.dueDate) {
            errBox.textContent = "Defina uma data acima antes de enviar ao Google Calendar.";
            setElementHidden(errBox, false);
            return;
          }
          const durationSelect = h("select", { class: "hub-input tasks-cal-duration" }) as HTMLSelectElement;
          for (const mins of [15, 30, 60, 90]) {
            durationSelect.append(h("option", { value: String(mins), text: `${mins} min` }));
          }
          durationSelect.value = "30";

          const titlePreview = fields.titleInput.value.trim() || item.text;
          const timeLabel = live.dueTime ? `${live.dueTime}` : "Dia inteiro";
          const confirmBtn = button(
            "Confirmar evento no Calendar",
            () => {
              void (async () => {
                confirmBtn.disabled = true;
                cancelBtn.disabled = true;
                setElementHidden(errBox, true);
                try {
                  const ev = await GoogleBridge.createEventFromTask({
                    taskId: item.id,
                    confirmed: true,
                    overrideDate: live.dueDate,
                    overrideTime: live.dueTime || null,
                    durationMinutes: Number(durationSelect.value) || 30,
                  });
                  calConfirmHost.hidden = true;
                  clear(calConfirmHost);
                  calStatusMsg.textContent = `Evento criado no Calendar: ${ev.summary}.`;
                  setElementHidden(calStatusMsg, false);
                  void refreshGoogleState();
                } catch (err) {
                  errBox.textContent = String(err).replace(/^Error:\s*/, "");
                  setElementHidden(errBox, false);
                  confirmBtn.disabled = false;
                  cancelBtn.disabled = false;
                }
              })();
            },
            "primary",
          );
          const cancelBtn = button("Cancelar", () => {
            calConfirmHost.hidden = true;
            clear(calConfirmHost);
          });

          calConfirmHost.append(
            h("div", {
              class: "tasks-calendar-preview",
              text: `${titlePreview} · ${live.dueDate} (${timeLabel})`,
            }),
          );
          if (live.dueTime) {
            calConfirmHost.append(
              h("div", { class: "tasks-google-actions" }, h("span", { class: "tasks-field-label", text: "Duração" }), durationSelect),
            );
          }
          calConfirmHost.append(
            h("div", {
              class: "tasks-privacy-caption",
              text: "Sem convidados · Criado somente com esta confirmação explícita.",
            }),
            h("div", { class: "tasks-google-actions" }, cancelBtn, confirmBtn),
          );
          setElementHidden(calConfirmHost, false);
        })
      : null;

    const calendarSection = googleStatus
      ? h(
          "div",
          { class: "tasks-inspector-section" },
          h("span", { class: "tasks-field-label", text: "Google Calendar" }),
          openCalPreviewBtn
            ? openCalPreviewBtn
            : h("span", {
                class: "tasks-privacy-caption",
                text: "Conecte sua conta Google em Ajustes → Integrações para enviar tarefas ao Calendar.",
              }),
          calConfirmHost,
          calStatusMsg,
        )
      : null;

    const body = h(
      "div",
      { class: "tasks-inspector-body" },
      fields.el,
      previewBtn
        ? h(
            "div",
            { class: "tasks-inspector-test-row" },
            h("span", { class: "tasks-privacy-caption", text: "Testar como o lembrete aparece na Ilha." }),
            previewBtn,
          )
        : null,
      h(
        "div",
        { class: "tasks-inspector-section" },
        h("span", { class: "tasks-field-label", text: "Subtarefas" }),
        subtasksList,
        h("div", { class: "tasks-subtask-add" }, subInput, addSubBtn),
      ),
      calendarSection,
      errBox,
    );

    return [
      h("div", { class: "tasks-inspector-head" }, h("h2", { text: "Editar tarefa" }), closeBtn),
      body,
      h("div", { class: "tasks-inspector-actions" }, deleteBtn, cancelBtn, saveBtn),
    ];
  }

  function renderInspector(focusField = false) {
    clear(inspectorSlot);
    activeFieldsHandle = null;
    activeSubmit = null;
    const editingItem =
      !draftProposal && editingId
        ? snap.items.find((it) => it.id === editingId && !it.deleted_at) ?? null
        : null;
    if (editingId && !editingItem && !draftProposal) {
      editingId = null;
    }

    const open = Boolean(draftProposal || editingItem);
    workspaceEl.classList.toggle("has-inspector", open);
    setElementHidden(inspectorSlot, !open);
    if (!open) {
      draftTargetKey = null;
      activeDraft = null;
      return;
    }

    const children = draftProposal
      ? buildProposalInspector(draftProposal, `proposal:${draftProposal.id}`)
      : buildEditInspector(editingItem!, `edit:${editingItem!.id}`);
    inspectorSlot.append(...children);

    if (focusField) {
      const firstInput = inspectorSlot.querySelector(".hub-input") as HTMLInputElement | null;
      firstInput?.focus();
    }
  }

  function renderTaskCard(item: TodoItem): HTMLElement {
    const isEditing = !draftProposal && editingId === item.id;
    const check = h("input", {
      type: "checkbox",
      class: "tasks-checkbox",
      "aria-label": `Concluir ${item.text}`,
      onchange: () => void runMutation(() => TasksBridge.toggleDone(item.id, !item.done)),
    }) as HTMLInputElement;
    check.checked = item.done;

    const dueText = item.due_date ? formatDueLabel(item) : "";
    const recText = recurrenceLabel(item.recurrence);
    const showPriority = item.priority === "urgent" || item.priority === "high";
    const prioBadge = showPriority
      ? h("span", {
          class: `tasks-prio ${item.priority}`,
          text: PRIORITY_LABELS[item.priority],
        })
      : null;

    // Quiet Row Rule + L6.3: só sinaliza na linha itens efetivamente pendentes de envio/sincronia.
    const showPendingSync = Boolean(snap.sync?.paired && (snap.sync.unsynced_item_ids ?? []).includes(item.id));
    const syncPill = showPendingSync
      ? h("span", {
          class: "tasks-chip-pill tasks-item-sync local",
          text: "Salvo neste PC",
        })
      : null;

    const subCount = item.subtasks.length;
    const subDone = item.subtasks.filter((s) => s.done).length;
    const subSummary = subCount > 0 ? `${subDone}/${subCount} subtarefas` : "";

    const metaParts: (HTMLElement | null)[] = [
      dueText ? h("span", { text: dueText }) : null,
      subSummary ? h("span", { text: subSummary }) : null,
      recText ? h("span", { class: "tasks-chip-pill", text: recText }) : null,
      item.snoozed_until ? h("span", { class: "tasks-chip-pill snoozed", text: "Adiada" }) : null,
      prioBadge,
      syncPill,
    ].filter(Boolean);

    const meta = metaParts.length > 0 ? h("div", { class: "tasks-item-meta" }, ...metaParts) : null;

    const editBtn = button(
      isEditing ? "Fechar" : "Editar",
      () => {
        draftProposal = null;
        if (isEditing) {
          closeInspector(true);
          return;
        }
        editingId = item.id;
        lastInvokerKey = { kind: "edit", taskId: item.id };
        renderInspector(true);
        renderList();
      },
      "",
      true,
    );
    editBtn.classList.add("tasks-edit-btn");

    const actionsRow = h(
      "div",
      { class: "tasks-item-actions" },
      !item.done && item.due_date
        ? button("+10m", () => void runMutation(() => TasksBridge.snooze(item.id, 10)), "", true)
        : null,
      editBtn,
      button("Excluir", () => void handleDelete(item.id), "danger", true),
    );

    const subtaskRows = h("div", { class: "tasks-subtasks" });
    for (const sub of item.subtasks) {
      const subCb = h("input", {
        type: "checkbox",
        "aria-label": `Subtarefa ${sub.text}`,
        onchange: () => void runMutation(() => TasksBridge.toggleSubtask(item.id, sub.id)),
      }) as HTMLInputElement;
      subCb.checked = sub.done;
      const removeSubBtn = button("×", () => void runMutation(() => TasksBridge.removeSubtask(item.id, sub.id)), "", true);
      removeSubBtn.setAttribute("aria-label", `Remover subtarefa ${sub.text}`);
      subtaskRows.append(
        h(
          "div",
          { class: sub.done ? "tasks-subtask done" : "tasks-subtask" },
          h("label", { class: "tasks-subtask-label" }, subCb, h("span", { text: sub.text })),
          removeSubBtn,
        ),
      );
    }

    const conflicts = (snap.sync?.conflicts ?? []).filter(
      (c) => c.entity_id === item.id && (!c.conflict_state || c.conflict_state === "pending_review"),
    );
    const conflictNodes = conflicts.map((c) => {
      let resolving = false;
      const resolve = (choice: "local" | "remote") => {
        if (resolving) return;
        resolving = true;
        keepLocalBtn.disabled = true;
        useRemoteBtn.disabled = true;
        void runMutation(() => TasksBridge.resolveConflict(c.id, choice), true).finally(() => {
          resolving = false;
          keepLocalBtn.disabled = false;
          useRemoteBtn.disabled = false;
        });
      };
      const keepLocalBtn = button("Manter texto deste PC", () => resolve("local"), "", true);
      const useRemoteBtn = button("Usar texto do celular", () => resolve("remote"), "primary", true);
      return h(
        "div",
        { class: "tasks-conflict-card", "data-conflict-id": c.id },
        h("div", {
          class: "tasks-conflict-title",
          text: `Conflito de edição (${c.field === "text" ? "Título" : "Subtarefa"})`,
        }),
        h(
          "div",
          { class: "tasks-conflict-options" },
          h("div", { class: "tasks-conflict-line", text: `Neste PC: ${c.local_text}` }),
          h("div", { class: "tasks-conflict-line", text: `No celular: ${c.remote_text}` }),
        ),
        h("div", { class: "tasks-conflict-actions" }, keepLocalBtn, useRemoteBtn),
      );
    });

    const row = h(
      "div",
      {
        class: [
          "tasks-item",
          item.done ? "done" : "",
          isEditing ? "editing" : "",
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
      ...conflictNodes,
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

  function applySnapshot(fresh: TasksSnapshot | null | undefined) {
    if (fresh) {
      snap = fresh;
      recording = fresh.recording;
    }
    renderBanners();
    updateCaptureButtons();
    renderInspector();
    renderUndo();
    renderList();
  }

  async function reload() {
    applySnapshot(await TasksBridge.state());
  }

  async function runMutation(op: () => Promise<unknown>, appliesSnapshot = false) {
    statusError = "";
    try {
      const result = await op();
      if (appliesSnapshot && result) {
        applySnapshot(result as TasksSnapshot);
      } else {
        await reload();
      }
    } catch (err) {
      statusError = String(err).replace(/^Error:\s*/, "");
      updateCaptureButtons();
    }
  }

  async function handleSyncNow() {
    if (syncing) return;
    syncing = true;
    renderBanners();
    updateCaptureButtons();
    try {
      await runMutation(() => TasksBridge.syncNow(), true);
    } finally {
      syncing = false;
      renderBanners();
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
      editingId = null;
      lastInvokerKey = { kind: "capture" };
      captureInput.value = "";
      renderInspector(true);
      renderList();
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
      editingId = null;
      lastInvokerKey = { kind: "capture" };
      recording = false;
      renderInspector(true);
      renderList();
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
  inspectorSlot.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (activeFieldsHandle?.hasOpenPopover()) {
        activeFieldsHandle.closePopover(true);
      } else {
        closeInspector(true);
      }
    } else if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      activeSubmit?.();
    }
  });
  searchInput.addEventListener("input", () => {
    query = searchInput.value ?? "";
    renderList();
  });

  const searchWrap = h("div", { class: "tasks-search-wrap" }, icon("search"), searchInput);
  const mainCol = h(
    "div",
    { class: "tasks-main" },
    undoSlot,
    h(
      "div",
      { class: "tasks-toolbar" },
      filterSlot,
      searchWrap,
    ),
    listSlot,
    googleSlot,
  );
  workspaceEl.append(mainCol, inspectorSlot);

  const pageHeader = typeof _ctx?.hero === "function"
    ? _ctx.hero(
        "tasks",
        "Tarefas",
        "Captura rápida, prazos claros e sincronia com a Segunda Mente.",
        [],
        true,
      )
    : h("header", { class: "tasks-page-head" }, h("h1", { text: "Tarefas" }));

  clear(container);
  container.append(
    h(
      "div",
      { class: "tasks-page" },
      pageHeader,
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
          { class: "tasks-capture-meta" },
          h(
            "div",
            {
              class: "tasks-privacy-caption",
              text: "Salvo localmente neste PC · Ctrl+Alt+K para captura rápida · Datas ambíguas sempre pedem confirmação",
            },
          ),
          syncBar,
        ),
        errorBanner,
        statusBanner,
      ),
      workspaceEl,
    ),
  );

  renderBanners();
  updateCaptureButtons();
  renderInspector();
  renderFilters();
  renderList();

  const offChanged = await onEvent("tasks-changed", () => void reload());
  const offVoice = await onEvent<boolean>("tasks-voice-state", (on) => {
    recording = Boolean(on);
    updateCaptureButtons();
  });
  const offFocus = await onEvent<string>("task-focus", (id) => {
    focusTaskId = id;
    if (snap.items.some((item) => item.id === id)) {
      renderList();
    } else {
      void reload();
    }
  });
  const offDismiss = await onEvent<string>("task-dismiss", () => void reload());
  const offGoogleBackup = await onEvent("google-backup-completed", () => void refreshGoogleState());
  void refreshGoogleState();
  activeCleanup = () => {
    offChanged();
    offVoice();
    offFocus();
    offDismiss();
    offGoogleBackup();
  };
  if (_ctx) _ctx.onLeave = () => {
    activeCleanup?.();
    activeCleanup = null;
  };
}
