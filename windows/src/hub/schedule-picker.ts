// Seletores táteis de agenda para o Hub (sem digitação de data/hora):
// - Calendário popover mensal com navegação < > e cliques diretos nos dias
// - Relógio analógico interativo 24h (horas -> minutos) + criação de predefinidos

import { clear, h } from "../views/dom";
import {
  buildMonthGrid,
  formatLocalDate,
  isValidClockTime,
  isValidIsoDate,
  monthYearTitle,
  timeChipLabel,
  todayIsoLocal,
} from "../core/tasks";
import { button, icon } from "./ui";

const WEEKDAYS_PT = ["D", "S", "T", "Q", "Q", "S", "S"] as const;
const DIAL_SIZE = 212;
const DIAL_CENTER = DIAL_SIZE / 2;
const DIAL_RADIUS = 80;

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

function parseIsoParts(iso: string | null): { year: number; month: number } {
  if (isValidIsoDate(iso)) {
    const [y, m] = iso.split("-").map(Number);
    return { year: y, month: m - 1 };
  }
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() };
}

function parseClockParts(hhmm: string | null): { hour: number; minute: number } {
  if (isValidClockTime(hhmm)) {
    const [hNum, mNum] = hhmm.split(":").map(Number);
    return { hour: hNum, minute: mNum };
  }
  return { hour: 9, minute: 0 };
}

export interface CalendarPopoverOptions {
  initialDate: string | null;
  onSelect: (isoDate: string | null) => void;
  onClose: () => void;
}

export function createCalendarPopover(opts: CalendarPopoverOptions): HTMLElement {
  const initial = parseIsoParts(opts.initialDate);
  let viewYear = initial.year;
  let viewMonth = initial.month;
  const todayIso = todayIsoLocal();

  const popover = h("div", {
    class: "tasks-picker-popover tasks-calendar-popover",
    role: "dialog",
    "aria-label": "Calendário para escolher o dia",
  });

  const titleEl = h("span", { class: "tasks-cal-title" });
  const gridEl = h("div", { class: "tasks-cal-grid" });

  const prevBtn = h(
    "button",
    {
      type: "button",
      class: "hub-icon tasks-cal-nav",
      title: "Mês anterior",
      "aria-label": "Mês anterior",
      onclick: () => {
        if (viewMonth === 0) {
          viewMonth = 11;
          viewYear -= 1;
        } else {
          viewMonth -= 1;
        }
        renderMonth();
      },
    },
    icon("chevronLeft"),
  );

  const nextBtn = h(
    "button",
    {
      type: "button",
      class: "hub-icon tasks-cal-nav",
      title: "Próximo mês",
      "aria-label": "Próximo mês",
      onclick: () => {
        if (viewMonth === 11) {
          viewMonth = 0;
          viewYear += 1;
        } else {
          viewMonth += 1;
        }
        renderMonth();
      },
    },
    icon("chevronRight"),
  );

  const closeBtn = h(
    "button",
    {
      type: "button",
      class: "hub-icon tasks-cal-close",
      title: "Fechar calendário",
      "aria-label": "Fechar calendário",
      onclick: () => opts.onClose(),
    },
    icon("x"),
  );

  const weekdaysRow = h(
    "div",
    { class: "tasks-cal-weekdays", "aria-hidden": "true" },
    ...WEEKDAYS_PT.map((label) => h("span", { text: label })),
  );

  function renderMonth() {
    titleEl.textContent = monthYearTitle(viewYear, viewMonth);
    clear(gridEl);
    for (const cell of buildMonthGrid(viewYear, viewMonth)) {
      const selected = opts.initialDate === cell.iso;
      const isToday = cell.iso === todayIso;
      const [y, m, d] = cell.iso.split("-");
      const btn = h("button", {
        type: "button",
        class: [
          "tasks-cal-day",
          cell.inMonth ? "" : "out",
          isToday ? "today" : "",
          selected ? "selected" : "",
        ]
          .filter(Boolean)
          .join(" "),
        text: String(cell.day),
        "data-iso": cell.iso,
        "aria-label": `${d}/${m}/${y}`,
        "aria-pressed": selected,
        onclick: () => {
          opts.onSelect(cell.iso);
          opts.onClose();
        },
      });
      gridEl.append(btn);
    }
  }

  renderMonth();

  const todayBtn = button(
    "Ir para hoje",
    () => {
      const now = new Date();
      viewYear = now.getFullYear();
      viewMonth = now.getMonth();
      opts.onSelect(formatLocalDate(now));
      opts.onClose();
    },
    "",
    true,
  );

  const clearBtn = button(
    "Sem data",
    () => {
      opts.onSelect(null);
      opts.onClose();
    },
    "",
    true,
  );

  popover.append(
    h(
      "div",
      { class: "tasks-picker-head" },
      h("div", { class: "tasks-cal-month-nav" }, prevBtn, titleEl, nextBtn),
      closeBtn,
    ),
    weekdaysRow,
    gridEl,
    h("div", { class: "tasks-picker-foot" }, clearBtn, todayBtn),
  );

  return popover;
}

export interface ClockPopoverOptions {
  mode: "time" | "preset";
  initialTime: string | null;
  presets?: number[];
  onApplyTime?: (hhmm: string | null) => void;
  onSavePreset?: (minutes: number) => Promise<void> | void;
  onRemovePreset?: (minutes: number) => Promise<void> | void;
  onClose: () => void;
}

export function createClockPopover(opts: ClockPopoverOptions): HTMLElement {
  const initial = parseClockParts(opts.initialTime);
  let hour = initial.hour;
  let minute = initial.minute;
  let step: "hour" | "minute" = "hour";
  let period: "am" | "pm" = hour >= 12 ? "pm" : "am";
  let busy = false;
  let presetList = [...(opts.presets ?? [])];

  const popover = h("div", {
    class: "tasks-picker-popover tasks-clock-popover",
    role: "dialog",
    "aria-label": opts.mode === "preset" ? "Criar horário predefinido" : "Relógio para escolher horário",
  });

  const headTitle = h("span", {
    class: "tasks-picker-title",
    text: opts.mode === "preset" ? "Novo horário predefinido" : "Escolher horário",
  });

  const closeBtn = h(
    "button",
    {
      type: "button",
      class: "hub-icon",
      title: "Fechar relógio",
      "aria-label": "Fechar relógio",
      onclick: () => opts.onClose(),
    },
    icon("x"),
  );

  const hourReadoutBtn = h("button", {
    type: "button",
    class: "tasks-clock-digit on",
    "aria-label": "Selecionar hora",
    onclick: () => {
      step = "hour";
      syncUI();
    },
  }) as HTMLButtonElement;

  const minuteReadoutBtn = h("button", {
    type: "button",
    class: "tasks-clock-digit",
    "aria-label": "Selecionar minutos",
    onclick: () => {
      step = "minute";
      syncUI();
    },
  }) as HTMLButtonElement;

  const amBtn = h("button", {
    type: "button",
    class: "tasks-chip",
    text: "00h–11h",
    onclick: () => {
      period = "am";
      if (hour >= 12) hour -= 12;
      step = "hour";
      syncUI();
    },
  }) as HTMLButtonElement;

  const pmBtn = h("button", {
    type: "button",
    class: "tasks-chip",
    text: "12h–23h",
    onclick: () => {
      period = "pm";
      if (hour < 12) hour += 12;
      step = "hour";
      syncUI();
    },
  }) as HTMLButtonElement;

  const dialEl = h("div", { class: "tasks-clock-dial" });
  const errEl = h("div", { class: "tasks-picker-err", hidden: true });
  errEl.hidden = true;
  const presetsWrap = h("div", { class: "tasks-clock-presets" });

  const applyBtn = button("", () => void handleApply(), "primary", true);

  function currentHhmm(): string {
    return `${pad2(hour)}:${pad2(minute)}`;
  }

  function renderDial() {
    clear(dialEl);

    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", `0 0 ${DIAL_SIZE} ${DIAL_SIZE}`);
    svg.setAttribute("class", "tasks-clock-svg");
    svg.setAttribute("aria-hidden", "true");

    const tickIndex = step === "hour" ? hour % 12 : Math.round(minute / 5) % 12;
    const angleRad = ((tickIndex * 30 - 90) * Math.PI) / 180;
    const handX = DIAL_CENTER + Math.cos(angleRad) * DIAL_RADIUS;
    const handY = DIAL_CENTER + Math.sin(angleRad) * DIAL_RADIUS;

    const handLine = document.createElementNS("http://www.w3.org/2000/svg", "line");
    handLine.setAttribute("x1", String(DIAL_CENTER));
    handLine.setAttribute("y1", String(DIAL_CENTER));
    handLine.setAttribute("x2", handX.toFixed(1));
    handLine.setAttribute("y2", handY.toFixed(1));
    handLine.setAttribute("class", "tasks-clock-hand");

    const centerDot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    centerDot.setAttribute("cx", String(DIAL_CENTER));
    centerDot.setAttribute("cy", String(DIAL_CENTER));
    centerDot.setAttribute("r", "4");
    centerDot.setAttribute("class", "tasks-clock-center-dot");

    const tipDot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    tipDot.setAttribute("cx", handX.toFixed(1));
    tipDot.setAttribute("cy", handY.toFixed(1));
    tipDot.setAttribute("r", "16");
    tipDot.setAttribute("class", "tasks-clock-tip-dot");

    svg.append(handLine, centerDot, tipDot);
    dialEl.append(svg);

    for (let i = 0; i < 12; i++) {
      const slotRad = ((i * 30 - 90) * Math.PI) / 180;
      const xPct = ((DIAL_CENTER + Math.cos(slotRad) * DIAL_RADIUS) / DIAL_SIZE) * 100;
      const yPct = ((DIAL_CENTER + Math.sin(slotRad) * DIAL_RADIUS) / DIAL_SIZE) * 100;

      if (step === "hour") {
        const valHour = period === "am" ? i : i + 12;
        const active = hour === valHour;
        const labelText = pad2(valHour);
        const btn = h("button", {
          type: "button",
          class: `tasks-clock-num${active ? " on" : ""}`,
          text: labelText,
          style: `left:${xPct.toFixed(2)}%;top:${yPct.toFixed(2)}%`,
          "data-hour": String(valHour),
          "aria-label": `${valHour} horas`,
          "aria-pressed": active,
          onclick: () => {
            hour = valHour;
            step = "minute";
            syncUI();
          },
        });
        dialEl.append(btn);
      } else {
        const valMin = i * 5;
        const active = minute === valMin;
        const labelText = pad2(valMin);
        const btn = h("button", {
          type: "button",
          class: `tasks-clock-num${active ? " on" : ""}`,
          text: labelText,
          style: `left:${xPct.toFixed(2)}%;top:${yPct.toFixed(2)}%`,
          "data-minute": String(valMin),
          "aria-label": `${valMin} minutos`,
          "aria-pressed": active,
          onclick: () => {
            minute = valMin;
            syncUI();
          },
        });
        dialEl.append(btn);
      }
    }
  }

  function renderPresetManager() {
    clear(presetsWrap);
    if (opts.mode !== "preset") return;
    presetsWrap.append(
      h("span", {
        class: "tasks-field-label",
        text: `Atalhos atuais (${presetList.length}/8)`,
      }),
    );
    const list = h("div", { class: "tasks-chip-row" });
    for (const mins of presetList) {
      const label = timeChipLabel(mins);
      const pill = h(
        "span",
        { class: "tasks-preset-pill" },
        h("span", { text: label }),
        h("button", {
          type: "button",
          class: "tasks-preset-remove",
          "aria-label": `Remover horário predefinido ${label}`,
          text: "×",
          onclick: () => void handleRemovePreset(mins),
        }),
      );
      list.append(pill);
    }
    if (presetList.length === 0) {
      list.append(h("span", { class: "tasks-privacy-caption", text: "Nenhum atalho salvo." }));
    }
    presetsWrap.append(list);
  }

  async function handleRemovePreset(mins: number) {
    if (!opts.onRemovePreset || busy) return;
    busy = true;
    errEl.hidden = true;
    errEl.setAttribute("hidden", "");
    try {
      await opts.onRemovePreset(mins);
      presetList = presetList.filter((m) => m !== mins);
      renderPresetManager();
      syncUI();
    } catch (err) {
      errEl.textContent = String(err).replace(/^Error:\s*/, "");
      errEl.hidden = false;
      errEl.removeAttribute("hidden");
    } finally {
      busy = false;
    }
  }

  async function handleApply() {
    if (busy) return;
    if (opts.mode === "time") {
      opts.onApplyTime?.(currentHhmm());
      opts.onClose();
      return;
    }
    if (!opts.onSavePreset) return;
    const totalMinutes = hour * 60 + minute;
    busy = true;
    applyBtn.disabled = true;
    errEl.hidden = true;
    errEl.setAttribute("hidden", "");
    try {
      await opts.onSavePreset(totalMinutes);
      opts.onClose();
    } catch (err) {
      errEl.textContent = String(err).replace(/^Error:\s*/, "");
      errEl.hidden = false;
      errEl.removeAttribute("hidden");
    } finally {
      busy = false;
      applyBtn.disabled = false;
    }
  }

  function syncUI() {
    hourReadoutBtn.textContent = pad2(hour);
    minuteReadoutBtn.textContent = pad2(minute);
    hourReadoutBtn.classList.toggle("on", step === "hour");
    minuteReadoutBtn.classList.toggle("on", step === "minute");
    amBtn.classList.toggle("on", period === "am");
    amBtn.setAttribute("aria-pressed", String(period === "am"));
    pmBtn.classList.toggle("on", period === "pm");
    pmBtn.setAttribute("aria-pressed", String(period === "pm"));
    applyBtn.textContent =
      opts.mode === "preset"
        ? `Salvar predefinido ${currentHhmm()}`
        : `Definir ${currentHhmm()}`;
    renderDial();
    renderPresetManager();
  }

  const fineMinRow = h(
    "div",
    { class: "tasks-clock-fine" },
    button(
      "-1 min",
      () => {
        minute = (minute + 59) % 60;
        step = "minute";
        syncUI();
      },
      "",
      true,
    ),
    button(
      "+1 min",
      () => {
        minute = (minute + 1) % 60;
        step = "minute";
        syncUI();
      },
      "",
      true,
    ),
  );

  const foot =
    opts.mode === "preset"
      ? h(
          "div",
          { class: "tasks-picker-foot" },
          button("Cancelar", () => opts.onClose(), "", true),
          applyBtn,
        )
      : h(
          "div",
          { class: "tasks-picker-foot" },
          button(
            "Sem hora",
            () => {
              opts.onApplyTime?.(null);
              opts.onClose();
            },
            "",
            true,
          ),
          applyBtn,
        );

  syncUI();

  popover.append(
    h("div", { class: "tasks-picker-head" }, headTitle, closeBtn),
    h(
      "div",
      { class: "tasks-clock-bar" },
      h(
        "div",
        { class: "tasks-clock-readout" },
        hourReadoutBtn,
        h("span", { class: "tasks-clock-colon", text: ":" }),
        minuteReadoutBtn,
      ),
      h("div", { class: "tasks-clock-period" }, amBtn, pmBtn),
    ),
    dialEl,
    fineMinRow,
    presetsWrap,
    errEl,
    foot,
  );

  return popover;
}
