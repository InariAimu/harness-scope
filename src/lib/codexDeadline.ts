import type { CodexReference } from "@/types/providerAccounts";
import { usageDateParts, usageTimestamp } from "./usageRange";

export type DeadlineZone = string | "PST";
export function deadlineWallTime(at: number, zone: DeadlineZone) {
  const p =
    zone === "PST"
      ? usageDateParts(at - 8 * 3600, "UTC")
      : usageDateParts(at, zone);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${p.year}-${pad(p.month)}-${pad(p.day)}T${pad(p.hour)}:${pad(p.minute)}:${pad(p.second)}`;
}
export function parseDeadlineWallTime(
  text: string,
  zone: DeadlineZone,
): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(text);
  if (!m) return null;
  const [year, month, day, hour, minute, second] = m
    .slice(1)
    .map((v) => Number(v ?? 0));
  if (
    year < 2000 ||
    year > 2100 ||
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > 31 ||
    hour > 23 ||
    minute > 59 ||
    second > 59
  )
    return null;
  const p = { year, month, day, hour, minute, second };
  const at =
    zone === "PST"
      ? Date.UTC(year, month - 1, day, hour, minute, second) / 1000 + 8 * 3600
      : usageTimestamp(p, zone);
  const normalized = `${m[1]}-${m[2]}-${m[3]}T${m[4]}:${m[5]}:${m[6] ?? "00"}`;
  return Number.isFinite(at) && deadlineWallTime(at, zone) === normalized
    ? at
    : null;
}
export interface DeadlineDraft {
  at: number | null;
  local: string;
  pst: string;
}
export function deadlineDraft(at: number, zone: string): DeadlineDraft {
  return {
    at,
    local: deadlineWallTime(at, zone),
    pst: deadlineWallTime(at, "PST"),
  };
}

export function codexDeadlineForecast(data: CodexReference, at: number | null) {
  const unavailable = (reason: string) => ({
    status: "unavailable" as const,
    reason,
  });
  if (at == null || !Number.isFinite(at) || at <= data.asOf)
    return unavailable("invalid_target_time");
  const window = data.windows.find((w) => w.id === "weekly");
  if (!window || window.status !== "ready")
    return unavailable(window?.reason ?? "no_quota");
  if (at > window.resetsAt!) return unavailable("target_after_reset");
  if (data.sample.reason) return unavailable(data.sample.reason);
  const hours = (at - data.asOf) / 3600;
  const pace = Number(data.pace);
  const rate = Number(data.sample.rateUsdPerHour) / pace;
  const requests = Number(data.sample.requestsPerHour) / pace;
  const calibrated = data.calibration.windows.find(
    (w) => w.durationMinutes === 10080 && w.status === "ready",
  );
  const intensity = calibrated
    ? Number(window.officialRemainingPercent) /
      hours /
      (Number(calibrated.modelPercentPerHour) / pace)
    : Number(window.remainingUsd) / hours / rate;
  if (
    !(rate > 0) ||
    !Number.isFinite(intensity) ||
    intensity < 0 ||
    intensity > 1e9
  )
    return unavailable("precision_limit");
  const warnings: string[] = [];
  for (const other of data.windows.filter((w) => w.id !== "weekly")) {
    if (other.status !== "ready") {
      warnings.push("other_window_unknown");
      continue;
    }
    const until = Math.min(at, other.resetsAt!);
    const otherHours = (until - data.asOf) / 3600;
    const c = data.calibration.windows.find(
      (w) => w.durationMinutes === 300 && w.status === "ready",
    );
    const maximum = c
      ? Number(other.officialRemainingPercent) /
        otherHours /
        (Number(c.modelPercentPerHour) / pace)
      : Number(other.remainingUsd) / otherHours / rate;
    if (intensity > maximum + 1e-9) warnings.push("other_window_limits_target");
    if (other.resetsAt! < at) warnings.push("window_resets_before_target");
  }
  return {
    status: "ready" as const,
    reason: null,
    basis: calibrated ? ("calibrated" as const) : ("reference" as const),
    intensity: String(intensity),
    usdPerHour: String(rate * intensity),
    requestsPerHour: String(requests * intensity),
    requestsPerDay: String(requests * intensity * 24),
    warnings: [...new Set(warnings)],
  };
}
