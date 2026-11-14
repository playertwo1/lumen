// Tarefas (L4): buckets, atalhos de agenda, busca sem acento, parser de proposta no chat
// e card editável com regra "IA propõe, usuário confirma".

import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, internals, sent } from "./tauri.mjs";
import {
  bucketFor,
  dateForShortcut,
  formatDueLabel,
  formatTimeMinutes,
  groupTasks,
  searchTodos,
  splitTodoProposal,
  timeChipLabel,
} from "../src/core/tasks.ts";
import { chatTodoProposalCard } from "../src/views/tasks.ts";

const defaultInvoke = internals.invoke;
let customInvoke = null;

function fakeTodo(overrides = {}) {
  return {
    id: "t-1",
    text: "Revisão do relatório mensal",
    due: "",
    due_date: "2026-11-14",
    due_time: "14:00",
    time_zone: "America/Sao_Paulo",
    alert_minutes: 20,
    priority: "normal",
    subtasks: [],
    recurrence: null,
    series_id: null,
    done: false,
    created_at: "2026-11-14T10:00:00Z",
    done_at: null,
    snoozed_until: null,
    source: null,
    source_stale: false,
    version: 1,
    deleted_at: null,
    ...overrides,
  };
}

beforeEach(() => {
  installFakeDom();
  calls.length = 0;
  customInvoke = null;
  internals.invoke = async (cmd, args) => {
    if (customInvoke) {
      const handled = await customInvoke(cmd, args);
      if (handled !== undefined) {
        calls.push([cmd, args]);
        return handled;
      }
    }
    return defaultInvoke(cmd, args);
  };
});

afterEach(() => {
  customInvoke = null;
  internals.invoke = defaultInvoke;
});

test("agrupa Hoje, Próximas, Atrasadas, Sem data e Concluídas ignorando tombstones", () => {
  const items = [
    fakeTodo({ id: "atrasada", due_date: "2026-11-12" }),
    fakeTodo({ id: "hoje", due_date: "2026-11-14" }),
    fakeTodo({ id: "proxima", due_date: "2026-11-18" }),
    fakeTodo({ id: "semdata", due_date: null, due_time: null }),
    fakeTodo({ id: "feita", due_date: "2026-11-14", done: true, done_at: "2026-11-14T11:00:00Z" }),
    fakeTodo({ id: "apagada", deleted_at: "2026-11-14T12:00:00Z" }),
  ];
  assert.equal(bucketFor(items[0], "2026-11-14"), "overdue");
  assert.equal(bucketFor(items[1], "2026-11-14"), "today");
  assert.equal(bucketFor(items[2], "2026-11-14"), "upcoming");
  assert.equal(bucketFor(items[3], "2026-11-14"), "unscheduled");
  assert.equal(bucketFor(items[4], "2026-11-14"), "done");

  const grouped = groupTasks(items, "2026-11-14");
  assert.deepEqual(grouped.overdue.map((i) => i.id), ["atrasada"]);
  assert.deepEqual(grouped.today.map((i) => i.id), ["hoje"]);
  assert.deepEqual(grouped.upcoming.map((i) => i.id), ["proxima"]);
  assert.deepEqual(grouped.unscheduled.map((i) => i.id), ["semdata"]);
  assert.deepEqual(grouped.done.map((i) => i.id), ["feita"]);
});

test("atalhos de data e horário (L4.2) calculam Hoje, Amanhã, +3 dias, Próxima segunda e 9h/14h/20h", () => {
  // Sábado 2026-11-14 local -> próxima segunda é 2026-11-16
  const saturday = new Date(2026, 10, 14, 10, 0, 0);
  assert.equal(dateForShortcut("today", saturday), "2026-11-14");
  assert.equal(dateForShortcut("tomorrow", saturday), "2026-11-15");
  assert.equal(dateForShortcut("inThreeDays", saturday), "2026-11-17");
  assert.equal(dateForShortcut("nextMonday", saturday), "2026-11-16");

  // Segunda 2026-11-16 local -> próxima segunda pula +7 dias (2026-11-23)
  const monday = new Date(2026, 10, 16, 8, 0, 0);
  assert.equal(dateForShortcut("nextMonday", monday), "2026-11-23");

  assert.equal(formatTimeMinutes(540), "09:00");
  assert.equal(formatTimeMinutes(840), "14:00");
  assert.equal(formatTimeMinutes(1200), "20:00");
  assert.equal(timeChipLabel(540), "9h");
  assert.equal(timeChipLabel(840), "14h");
  assert.equal(timeChipLabel(1200), "20h");
});

test("busca sem acento e case-insensitive inclui título e subtarefas", () => {
  const items = [
    fakeTodo({
      id: "consulta",
      text: "Marcar revisão médica anual",
      subtasks: [{ id: "s1", text: "Levar exame de sangue", done: false }],
    }),
    fakeTodo({ id: "outra", text: "Comprar café" }),
  ];
  assert.deepEqual(searchTodos(items, "revisao medica").map((i) => i.id), ["consulta"]);
  assert.deepEqual(searchTodos(items, "EXAME SANGUE").map((i) => i.id), ["consulta"]);
  assert.deepEqual(searchTodos(items, "cafe").map((i) => i.id), ["outra"]);
});

test("splitTodoProposal extrai proposta do chat e descarta datas ambíguas sem adivinhar", () => {
  const md = [
    "Posso registrar isso para você.",
    "```lumen-todo",
    JSON.stringify({
      id: "p-amb",
      title: "Enviar contrato assinado",
      date_state: "ambiguous",
      due_date: "2026-11-20",
      date_phrase: "sexta",
      time_state: "ambiguous",
      due_time: "09:00",
      time_phrase: "de manhã",
      priority: "high",
    }),
    "```",
  ].join("\n");

  const { text, proposal, isAmbiguous } = splitTodoProposal(md);
  assert.equal(text, "Posso registrar isso para você.");
  assert.ok(proposal);
  assert.equal(isAmbiguous, true);
  // Segurança: datas/horas marcadas como ambiguous nunca passam como preenchidas.
  assert.equal(proposal.due_date, null);
  assert.equal(proposal.due_time, null);
  assert.equal(proposal.date_phrase, "sexta");
  assert.equal(proposal.time_phrase, "de manhã");
});

test("chatTodoProposalCard nunca grava sem clique explícito em Confirmar", async () => {
  customInvoke = (cmd, args) => {
    if (cmd !== "tasks_confirm_proposal") return undefined;
    const { input } = args;
    return fakeTodo({
      id: "saved-99",
      text: input.text,
      due_date: input.due_date,
      due_time: input.due_time,
      alert_minutes: input.alert_minutes,
      priority: input.priority,
    });
  };

  const card = chatTodoProposalCard({
    id: "prop-chat-1",
    title: "Pagar guia",
    date_state: "ambiguous",
    due_date: null,
    date_phrase: "sexta",
    time_state: "unspecified",
    due_time: null,
    time_phrase: null,
    priority: "normal",
  });

  // Montar o cartão não pode disparar gravação automática.
  assert.equal(calls.length, 0);
  const banner = card.find(".task-ambiguous-banner")[0];
  assert.ok(banner.textContent.includes("sexta"));

  // Usuário escolhe "Amanhã" e "14h" e clica em Confirmar.
  const chips = card.find(".task-chip");
  const amanha = chips.find((b) => b.textContent === "Amanhã");
  const h14 = chips.find((b) => b.textContent === "14h");
  assert.ok(amanha && h14);
  amanha.fire("click");
  h14.fire("click");

  const confirmBtn = card.find("BUTTON").find((b) => b.textContent === "Confirmar");
  assert.ok(confirmBtn);
  confirmBtn.fire("click");
  await new Promise((r) => setTimeout(r, 0));

  const confirmCalls = sent("tasks_confirm_proposal");
  assert.equal(confirmCalls.length, 1);
  assert.equal(confirmCalls[0].input.due_date, dateForShortcut("tomorrow"));
  assert.equal(confirmCalls[0].input.due_time, "14:00");
  assert.equal(formatDueLabel({ due_date: "2099-01-02", due_time: "09:00", alert_minutes: 20 }), "02/01/2099 · 09:00 · aviso 20m");
});
