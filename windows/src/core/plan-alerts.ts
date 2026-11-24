// Avisos progressivos de cota (50%, 40%, 30%, 20%, 10% restantes) para Claude, Codex e Antigravity.
// Módulo puro sem dependência de DOM para testes determinísticos.

import { effectivePct, type PlanUsage, type PlanWindow } from "./plan";
import type { SoundName } from "./sound";

export type PlanProvider = "claude" | "codex" | "agy";
export type PlanWindowKey = "fiveHour" | "sevenDay";
export type QuotaThreshold = 50 | 40 | 30 | 20 | 10;

/** Patamares monitorados (% restante antes de zerar), do menor (mais urgente) ao maior. */
export const QUOTA_THRESHOLDS_ASC: readonly QuotaThreshold[] = [10, 20, 30, 40, 50];

export interface QuotaThresholdStyle {
  threshold: QuotaThreshold;
  level: 1 | 2 | 3 | 4 | 5;
  color: string;
  urgencyLabel: string;
  sound: SoundName;
  ttlMs: number;
  urgent: boolean;
}

const THRESHOLD_STYLES: Record<QuotaThreshold, QuotaThresholdStyle> = {
  50: {
    threshold: 50,
    level: 1,
    color: "#FACC15",
    urgencyLabel: "Atenção",
    sound: "blip",
    ttlMs: 8000,
    urgent: false,
  },
  40: {
    threshold: 40,
    level: 2,
    color: "#F59E0B",
    urgencyLabel: "Alerta",
    sound: "blip",
    ttlMs: 8000,
    urgent: false,
  },
  30: {
    threshold: 30,
    level: 3,
    color: "#F97316",
    urgencyLabel: "Elevado",
    sound: "rate",
    ttlMs: 10_000,
    urgent: false,
  },
  20: {
    threshold: 20,
    level: 4,
    color: "#F4505E",
    urgencyLabel: "Alto",
    sound: "rate",
    ttlMs: 12_000,
    urgent: true,
  },
  10: {
    threshold: 10,
    level: 5,
    color: "#FF2E4C",
    urgencyLabel: "Crítico",
    sound: "error",
    ttlMs: 12_000,
    urgent: true,
  },
};

export const PROVIDER_LABEL: Record<PlanProvider, string> = {
  claude: "Claude",
  codex: "Codex",
  agy: "Antigravity",
};

export const WINDOW_LABEL: Record<PlanWindowKey, string> = {
  fiveHour: "5h",
  sevenDay: "Semana",
};

/** Tolerância para pequenas variações de clock/arredondamento no resetsAt retornado pela CLI/API. */
const RESET_JITTER_MS = 120_000;

export interface PlanWindowAlertRecord {
  resetsAt: number;
  lowestAlerted: QuotaThreshold;
}

export type PlanAlertStateMap = Record<string, PlanWindowAlertRecord>;

export interface PlanThresholdAlert {
  provider: PlanProvider;
  providerLabel: string;
  windowKey: PlanWindowKey;
  windowLabel: string;
  threshold: QuotaThreshold;
  remainingPct: number;
  usedPct: number;
  resetsAt: number;
  style: QuotaThresholdStyle;
  islandText: string;
  toastTitle: string;
  toastBody: string;
}

export function quotaThresholdForRemaining(remainingPct: number): QuotaThreshold | null {
  if (!Number.isFinite(remainingPct)) return null;
  const clamped = Math.max(0, Math.min(100, remainingPct));
  for (const t of QUOTA_THRESHOLDS_ASC) {
    if (clamped <= t) return t;
  }
  return null;
}

export function quotaThresholdStyle(threshold: QuotaThreshold): QuotaThresholdStyle {
  return THRESHOLD_STYLES[threshold];
}

export function remainingPctForWindow(window: PlanWindow, now = Date.now()): number {
  return Math.max(0, Math.min(100, 100 - effectivePct(window, now)));
}

export function formatPlanAlert(
  provider: PlanProvider,
  windowKey: PlanWindowKey,
  threshold: QuotaThreshold,
  remainingPct: number,
  usedPct: number,
  resetsAt: number,
): PlanThresholdAlert {
  const providerLabel = PROVIDER_LABEL[provider];
  const windowLabel = WINDOW_LABEL[windowKey];
  const style = quotaThresholdStyle(threshold);
  const roundedRemaining = Math.max(0, Math.min(100, Math.round(remainingPct)));
  const statusText = roundedRemaining <= 0 ? "cota zerada" : `restam ${roundedRemaining}%`;
  const islandText = `${providerLabel} · ${windowLabel}: ${statusText}`;
  const toastTitle = style.level >= 5
    ? `Lumen · Cota crítica (${providerLabel})`
    : `Lumen · Cota em ${threshold}% (${providerLabel})`;
  const toastBody = roundedRemaining <= 0
    ? `Janela ${windowLabel}: cota zerada antes do próximo reset.`
    : `Janela ${windowLabel}: restam ${roundedRemaining}% antes de zerar.`;

  return {
    provider,
    providerLabel,
    windowKey,
    windowLabel,
    threshold,
    remainingPct: roundedRemaining,
    usedPct: Math.round(usedPct),
    resetsAt,
    style,
    islandText,
    toastTitle,
    toastBody,
  };
}

function alertKey(provider: PlanProvider, windowKey: PlanWindowKey): string {
  return `${provider}:${windowKey}`;
}

export function restorePlanAlertState(raw: string | null): PlanAlertStateMap {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: PlanAlertStateMap = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (!v || typeof v !== "object" || Array.isArray(v)) continue;
      const rec = v as Record<string, unknown>;
      if (
        typeof rec.resetsAt === "number" && Number.isFinite(rec.resetsAt) &&
        typeof rec.lowestAlerted === "number" && (QUOTA_THRESHOLDS_ASC as readonly number[]).includes(rec.lowestAlerted)
      ) {
        out[k] = {
          resetsAt: rec.resetsAt,
          lowestAlerted: rec.lowestAlerted as QuotaThreshold,
        };
      }
    }
    return out;
  } catch {
    return {};
  }
}

export class PlanThresholdTracker {
  private state: PlanAlertStateMap;

  constructor(initial: PlanAlertStateMap = {}) {
    this.state = { ...initial };
  }

  snapshot(): PlanAlertStateMap {
    return { ...this.state };
  }

  reset(): void {
    this.state = {};
  }

  evaluate(provider: PlanProvider, usage: PlanUsage | null | undefined, now = Date.now()): PlanThresholdAlert[] {
    if (!usage) return [];
    const alerts: PlanThresholdAlert[] = [];

    for (const windowKey of ["fiveHour", "sevenDay"] as const) {
      const w = usage[windowKey];
      if (!w) continue;
      const key = alertKey(provider, windowKey);
      const effUsed = effectivePct(w, now);
      const remaining = remainingPctForWindow(w, now);

      // Janela já expirou / zerou o contador: limpa estado anterior desse ciclo.
      if (w.resetsAt <= now) {
        delete this.state[key];
        continue;
      }

      const prev = this.state[key];
      const sameCycle =
        prev != null &&
        Math.abs(w.resetsAt - prev.resetsAt) <= RESET_JITTER_MS &&
        remaining <= prev.lowestAlerted + 5;

      if (!sameCycle) {
        delete this.state[key];
      }

      const threshold = quotaThresholdForRemaining(remaining);
      if (threshold == null) continue;

      const currentPrev = this.state[key];
      if (currentPrev && threshold >= currentPrev.lowestAlerted) {
        // Atualiza resetsAt caso tenha oscilado dentro da tolerância, sem disparar de novo.
        this.state[key] = { resetsAt: w.resetsAt, lowestAlerted: currentPrev.lowestAlerted };
        continue;
      }

      this.state[key] = {
        resetsAt: w.resetsAt,
        lowestAlerted: threshold,
      };
      alerts.push(formatPlanAlert(provider, windowKey, threshold, remaining, effUsed, w.resetsAt));
    }

    // Ordena do mais urgente (menor % restante) para o menos urgente.
    alerts.sort((a, b) => a.threshold - b.threshold || a.remainingPct - b.remainingPct);
    return alerts;
  }
}
