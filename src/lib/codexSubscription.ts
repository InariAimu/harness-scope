import type { CodexPlan, CodexReference } from "@/types/providerAccounts";
export const CODEX_PLANS: Record<
  CodexPlan,
  { name: string; monthlyUsd: number; multiplier: number }
> = {
  plus: { name: "Plus", monthlyUsd: 20, multiplier: 1 },
  pro100: { name: "Pro 100", monthlyUsd: 100, multiplier: 5 },
  pro200: { name: "Pro 200", monthlyUsd: 200, multiplier: 10 },
  pro500: { name: "Pro 500", monthlyUsd: 500, multiplier: 25 },
};
export function codexWindowLabel(minutes: number | undefined, zh: boolean) {
  if (!minutes || !Number.isFinite(minutes))
    return zh ? "额度窗口" : "Quota window";
  if (minutes === 300) return zh ? "5 小时窗口" : "5-hour window";
  if (minutes === 10080) return zh ? "每周窗口" : "Weekly window";
  if (minutes % 1440 === 0)
    return zh ? `${minutes / 1440} 天窗口` : `${minutes / 1440}-day window`;
  if (minutes % 60 === 0)
    return zh ? `${minutes / 60} 小时窗口` : `${minutes / 60}-hour window`;
  return zh ? `${minutes} 分钟窗口` : `${minutes}-minute window`;
}

/** Use calibrated results only when every official window is usable. */
export function codexForecastSummary(data: CodexReference) {
  const calibrated = data.calibration;
  if (
    !data.sample.reason &&
    calibrated.currentWindowTarget &&
    calibrated.windows.length > 0 &&
    calibrated.windows.every((w) => w.status === "ready")
  ) {
    const next = calibrated.windows.reduce((a, b) =>
      a.runtimeSeconds! <= b.runtimeSeconds! ? a : b,
    );
    const limit = calibrated.windows.reduce((a, b) =>
      Number(a.requiredIntensity) <= Number(b.requiredIntensity) ? a : b,
    );
    return {
      basis: "calibrated" as const,
      intensity: limit.requiredIntensity!,
      requestsPerHour: limit.requiredRequestsPerHour!,
      durationMinutes: limit.durationMinutes,
      percentPerHour: limit.modelPercentPerHour,
      next: {
        kind: next.coversReset ? ("reset" as const) : ("exhaustion" as const),
        at: data.asOf + next.runtimeSeconds!,
      },
    };
  }
  if (
    data.sample.reason ||
    !data.nextEvent ||
    !data.windows.length ||
    data.windows.some(
      (w) => w.status !== "ready" || w.requiredIntensity == null,
    )
  )
    return null;
  const limit = data.windows.reduce((a, b) =>
    Number(a.requiredIntensity) <= Number(b.requiredIntensity) ? a : b,
  );
  return {
    basis: "reference" as const,
    intensity: limit.requiredIntensity!,
    requestsPerHour: limit.requestsPerHour!,
    durationMinutes: limit.id === "rolling" ? 300 : 10080,
    percentPerHour: undefined,
    next: data.nextEvent,
  };
}
