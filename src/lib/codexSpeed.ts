import type { CodexSubscriptionSpeed } from "@/types/codexAllowances";
import type { CodexReference } from "@/types/providerAccounts";

export type CodexSpeedMode = "standard" | "fast";
export interface CodexSpeedAssumption {
  basis: "reference" | "calibrated";
  baseline: CodexSpeedMode | "unknown";
}
export function codexSpeedForecast(
  data: CodexReference,
  rules: CodexSubscriptionSpeed,
  assumption: CodexSpeedAssumption,
  mode: CodexSpeedMode,
  targetAt: number | null,
) {
  const unavailable = (reason: string) => ({
    status: "unavailable" as const,
    reason,
  });
  if (data.sample.reason) return unavailable(data.sample.reason);
  if (assumption.baseline === "unknown")
    return unavailable("unknown_speed_baseline");
  const model = data.allowanceModel ?? data.model;
  if (
    (mode === "fast" || assumption.baseline === "fast") &&
    !rules.fastModelIds.includes(model ?? "")
  )
    return unavailable("unsupported_fast_model");
  const fast = Number(rules.fastMultiplier);
  const pace = Number(data.pace);
  if (!Number.isFinite(fast) || fast < 1 || fast > 100 || !(pace > 0))
    return unavailable("precision_limit");
  const relative =
    (mode === "fast" ? fast : 1) / (assumption.baseline === "fast" ? fast : 1);
  if (!data.windows.length) return unavailable("no_quota");
  const windows = [];
  for (const window of data.windows) {
    if (window.status !== "ready")
      return unavailable(window.reason ?? "no_quota");
    const minutes = window.id === "weekly" ? 10080 : 300;
    let rate: number;
    if (assumption.basis === "calibrated") {
      const sample = data.calibration.windows.find(
        (w) => w.durationMinutes === minutes,
      );
      if (!data.calibration.identityVerified || sample?.status !== "ready")
        return unavailable(sample?.reason ?? "insufficient_quota_history");
      rate = Number(sample.modelPercentPerHour) * relative;
    } else {
      rate =
        (Number(data.sample.rateUsdPerHour) / Number(window.budgetUsd)) *
        100 *
        relative;
    }
    const remaining = Number(window.officialRemainingPercent);
    const reset = window.resetsAt;
    if (
      !Number.isFinite(rate) ||
      !(rate > 0) ||
      !Number.isFinite(remaining) ||
      remaining < 0 ||
      reset == null ||
      reset <= data.asOf
    )
      return unavailable("precision_limit");
    const lifetime = (remaining / rate) * 3600;
    const until = reset - data.asOf;
    windows.push({
      id: window.id,
      minutes,
      rate,
      remaining,
      reset,
      seconds: Math.ceil(Math.min(lifetime, until)),
      exhausts: lifetime <= until,
    });
  }
  const first = windows.reduce((a, b) => (a.seconds <= b.seconds ? a : b));
  const safeIntensity = Math.min(
    ...windows.map(
      (w) => (w.remaining / ((w.reset - data.asOf) / 3600) / w.rate) * pace,
    ),
  );
  const weekly = windows.find((w) => w.id === "weekly");
  const target = (() => {
    if (targetAt == null || !Number.isFinite(targetAt) || targetAt <= data.asOf)
      return unavailable("invalid_target_time");
    if (!weekly) return unavailable("no_quota");
    if (targetAt > weekly.reset) return unavailable("target_after_reset");
    const intensity =
      (weekly.remaining / ((targetAt - data.asOf) / 3600) / weekly.rate) * pace;
    if (!Number.isFinite(intensity) || intensity < 0 || intensity > 1e9)
      return unavailable("precision_limit");
    const warnings: string[] = [];
    for (const other of windows.filter((w) => w.id !== "weekly")) {
      const hours = (Math.min(targetAt, other.reset) - data.asOf) / 3600;
      const max = (other.remaining / hours / other.rate) * pace;
      if (intensity > max + 1e-9) warnings.push("other_window_limits_target");
      if (other.reset < targetAt) warnings.push("window_resets_before_target");
    }
    return {
      status: "ready" as const,
      reason: null,
      intensity: String(intensity),
      requestsPerHour: String(
        (Number(data.sample.requestsPerHour) / pace) * intensity,
      ),
      warnings: [...new Set(warnings)],
    };
  })();
  return {
    status: "ready" as const,
    reason: null,
    mode,
    relativeMultiplier: String(relative),
    percentPerHour: String(weekly?.rate ?? first.rate),
    intensity: String(safeIntensity),
    requestsPerHour: String(
      (Number(data.sample.requestsPerHour) / pace) * safeIntensity,
    ),
    next: {
      at: data.asOf + first.seconds,
      kind: first.exhausts ? ("exhaustion" as const) : ("reset" as const),
    },
    target,
  };
}
