import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import {
  PlanThresholdTracker,
  QUOTA_THRESHOLDS_ASC,
  quotaThresholdForRemaining,
  quotaThresholdStyle,
  restorePlanAlertState,
} from "../src/core/plan-alerts.ts";
import { compactCenter } from "../src/core/compact.ts";
import { DEFAULT_SETTINGS, State } from "../src/core/state.ts";
import {
  evaluateAndDispatchPlanAlerts,
  registerPlanAlertSink,
  resetPlanAlertTrackerForTests,
} from "../src/views/usage.ts";
import { calls, sent } from "./tauri.mjs";

const NOW = Date.UTC(2026, 10, 24, 15, 0, 0);
const RESET_5H = NOW + 3 * 3600_000;
const RESET_7D = NOW + 5 * 86_400_000;

function makeUsage(fiveUsedPct, sevenUsedPct, reset5 = RESET_5H, reset7 = RESET_7D) {
  return {
    fiveHour: fiveUsedPct == null ? undefined : { usedPct: fiveUsedPct, resetsAt: reset5 },
    sevenDay: sevenUsedPct == null ? undefined : { usedPct: sevenUsedPct, resetsAt: reset7 },
    updatedAt: NOW,
  };
}

beforeEach(() => {
  calls.length = 0;
  State.settings = { ...DEFAULT_SETTINGS, planThresholdAlerts: true };
  State.paused = false;
  registerPlanAlertSink(null);
  resetPlanAlertTrackerForTests();
});

test("patamares de cota restante (50, 40, 30, 20, 10) e cores crescentes de urgência", () => {
  assert.equal(quotaThresholdForRemaining(51), null);
  assert.equal(quotaThresholdForRemaining(50), 50);
  assert.equal(quotaThresholdForRemaining(41), 50);
  assert.equal(quotaThresholdForRemaining(40), 40);
  assert.equal(quotaThresholdForRemaining(31), 40);
  assert.equal(quotaThresholdForRemaining(30), 30);
  assert.equal(quotaThresholdForRemaining(21), 30);
  assert.equal(quotaThresholdForRemaining(20), 20);
  assert.equal(quotaThresholdForRemaining(11), 20);
  assert.equal(quotaThresholdForRemaining(10), 10);
  assert.equal(quotaThresholdForRemaining(0), 10);
  assert.equal(quotaThresholdForRemaining(Number.NaN), null);

  const styles = [...QUOTA_THRESHOLDS_ASC].reverse().map((t) => quotaThresholdStyle(t));
  assert.deepEqual(styles.map((s) => s.level), [1, 2, 3, 4, 5]);
  assert.equal(new Set(styles.map((s) => s.color)).size, 5);
  assert.deepEqual(styles.map((s) => s.urgent), [false, false, false, true, true]);
});

test("rastreia 5h e semana separadamente para Claude, Codex e Antigravity", () => {
  const tracker = new PlanThresholdTracker();

  const claudeAlerts = tracker.evaluate("claude", makeUsage(50, 60), NOW);
  assert.equal(claudeAlerts.length, 2);
  // 40% restante (semana) vem antes de 50% restante (5h) por maior urgência.
  assert.equal(claudeAlerts[0].windowKey, "sevenDay");
  assert.equal(claudeAlerts[0].threshold, 40);
  assert.equal(claudeAlerts[0].islandText, "Claude · Semana: restam 40%");

  assert.equal(claudeAlerts[1].windowKey, "fiveHour");
  assert.equal(claudeAlerts[1].threshold, 50);
  assert.equal(claudeAlerts[1].islandText, "Claude · 5h: restam 50%");

  const codexAlerts = tracker.evaluate("codex", makeUsage(80, 10), NOW);
  assert.equal(codexAlerts.length, 1);
  assert.equal(codexAlerts[0].provider, "codex");
  assert.equal(codexAlerts[0].threshold, 20);
  assert.equal(codexAlerts[0].style.urgent, true);
  assert.equal(codexAlerts[0].islandText, "Codex · 5h: restam 20%");

  const agyAlerts = tracker.evaluate("agy", makeUsage(15, 90), NOW);
  assert.equal(agyAlerts.length, 1);
  assert.equal(agyAlerts[0].provider, "agy");
  assert.equal(agyAlerts[0].windowKey, "sevenDay");
  assert.equal(agyAlerts[0].threshold, 10);
  assert.equal(agyAlerts[0].style.level, 5);
  assert.equal(agyAlerts[0].islandText, "Antigravity · Semana: restam 10%");
});

test("evita repetição no mesmo ciclo e emite apenas o patamar mais urgente em salto", () => {
  const tracker = new PlanThresholdTracker();

  // Salto direto de 0% -> 85% usado (15% restante) deve emitir só o patamar de 20%.
  const first = tracker.evaluate("codex", makeUsage(85, null), NOW);
  assert.equal(first.length, 1);
  assert.equal(first[0].threshold, 20);
  assert.equal(first[0].remainingPct, 15);

  // Oscilação leve no resetsAt (+45 s) e restante 18% não dispara novamente.
  const repeat = tracker.evaluate("codex", makeUsage(82, null, RESET_5H + 45_000), NOW);
  assert.equal(repeat.length, 0);

  // Ao cruzar para 9% restantes (91% usado), dispara o patamar crítico de 10%.
  const critical = tracker.evaluate("codex", makeUsage(91, null, RESET_5H + 45_000), NOW);
  assert.equal(critical.length, 1);
  assert.equal(critical[0].threshold, 10);
  assert.equal(critical[0].style.level, 5);
  assert.equal(critical[0].style.color, "#FF2E4C");
});

test("reinicia o histórico quando a janela expira ou muda de ciclo de reset", () => {
  const tracker = new PlanThresholdTracker();

  assert.equal(tracker.evaluate("claude", makeUsage(90, null, RESET_5H), NOW).length, 1);

  // Janela expirada (resetsAt <= now) zera estado e não dispara.
  assert.equal(tracker.evaluate("claude", makeUsage(90, null, NOW - 1000), NOW).length, 0);

  // Novo ciclo de reset permite alertar novamente a partir de 50%.
  const nextReset = RESET_5H + 5 * 3600_000;
  const fresh = tracker.evaluate("claude", makeUsage(50, null, nextReset), NOW);
  assert.equal(fresh.length, 1);
  assert.equal(fresh[0].threshold, 50);
});

test("restorePlanAlertState ignora payload inválido e preserva entradas válidas", () => {
  assert.deepEqual(restorePlanAlertState(null), {});
  assert.deepEqual(restorePlanAlertState("{invalido"), {});
  assert.deepEqual(restorePlanAlertState("[]"), {});

  const restored = restorePlanAlertState(
    JSON.stringify({
      "codex:fiveHour": { resetsAt: RESET_5H, lowestAlerted: 30 },
      "bad:entry": { resetsAt: RESET_5H, lowestAlerted: 25 },
      "bad:num": { resetsAt: "nope", lowestAlerted: 20 },
    }),
  );
  assert.deepEqual(restored, {
    "codex:fiveHour": { resetsAt: RESET_5H, lowestAlerted: 30 },
  });
});

test("evaluateAndDispatchPlanAlerts envia para ilha e Windows Toast e respeita pausa/configuração", () => {
  const delivered = [];
  registerPlanAlertSink((alert) => delivered.push(alert));

  const alerts = evaluateAndDispatchPlanAlerts("agy", makeUsage(70, null), NOW);
  assert.equal(alerts.length, 1);
  assert.equal(delivered.length, 1);
  assert.equal(delivered[0].threshold, 30);

  const toasts = sent("plan_alert_toast_show");
  assert.equal(toasts.length, 1);
  assert.equal(toasts[0].provider, "agy");
  assert.equal(toasts[0].windowKey, "fiveHour");
  assert.match(toasts[0].title, /30%/);
  assert.match(toasts[0].body, /restam 30%/);

  // Desligado nas configurações: não dispara sink nem toast.
  State.settings.planThresholdAlerts = false;
  assert.equal(evaluateAndDispatchPlanAlerts("agy", makeUsage(90, null), NOW).length, 0);
  assert.equal(delivered.length, 1);
  assert.equal(sent("plan_alert_toast_show").length, 1);

  // Pausado: também suprime.
  State.settings.planThresholdAlerts = true;
  State.paused = true;
  assert.equal(evaluateAndDispatchPlanAlerts("agy", makeUsage(90, null), NOW).length, 0);
  assert.equal(delivered.length, 1);
  assert.equal(sent("plan_alert_toast_show").length, 1);
});

test("compactCenter preserva flag urgent e ação plan no notice de cota", () => {
  const item = compactCenter({
    now: new Date(NOW),
    pendingApproval: null,
    notice: {
      text: "Codex · 5h: restam 10%",
      color: "#FF2E4C",
      expiresAt: NOW + 10_000,
      urgent: true,
      action: { kind: "plan", provider: "codex" },
    },
    whatsapp: null,
    mailUnread: 0,
    todos: [],
    music: null,
  });

  assert.ok(item);
  assert.equal(item.kind, "notice");
  assert.equal(item.urgent, true);
  assert.equal(item.dot, "#FF2E4C");
  assert.deepEqual(item.action, { kind: "plan", provider: "codex" });
});
