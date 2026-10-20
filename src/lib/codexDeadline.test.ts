import { expect, it } from "vitest";
import {
  codexDeadlineForecast,
  deadlineDraft,
  deadlineWallTime,
  parseDeadlineWallTime,
} from "./codexDeadline";
import type { CodexReference } from "@/types/providerAccounts";

it("synchronizes Shanghai and fixed PST across dates without applying PDT", () => {
  const at = parseDeadlineWallTime("2026-10-21T15:00", "Asia/Shanghai")!;
  expect(deadlineWallTime(at, "PST")).toBe("2026-10-20T23:00:00");
  expect(parseDeadlineWallTime("2026-10-20T23:00", "PST")).toBe(at);
  expect(deadlineDraft(at, "Asia/Shanghai").local).toBe("2026-10-21T15:00:00");
  expect(deadlineWallTime(at, "America/Los_Angeles")).toBe(
    "2026-10-21T00:00:00",
  );
  expect(parseDeadlineWallTime("2026-02-30T12:00", "PST")).toBeNull();
  expect(
    parseDeadlineWallTime("2026-03-08T02:30", "America/Los_Angeles"),
  ).toBeNull();
  expect(parseDeadlineWallTime("", "Asia/Shanghai")).toBeNull();
});
function fixture(): CodexReference {
  return {
    asOf: 100000,
    model: "gpt-6.1-sol",
    models: ["gpt-6.1-sol"],
    plan: "pro100",
    multiplier: 5,
    monthlyFeeUsd: "100",
    mode: "modeldial",
    catalogVersion: "fixture",
    catalogRevision: "fixture",
    quotaFetchedAt: 100000,
    bucket: "codex",
    pace: "1",
    sample: {
      requests: 10,
      tokens: 1000,
      unpricedRequests: 0,
      costUsd: "2",
      spanSeconds: 60,
      lastAt: 100000,
      reason: null,
      rateUsdPerHour: "2",
      requestsPerHour: "10",
    },
    calibration: {
      currentWindowTarget: null,
      identityVerified: true,
      historyPoints: 1,
      windows: [],
    },
    windows: [
      {
        id: "weekly",
        budgetUsd: "50",
        capacity: null,
        remainingUsd: "40",
        officialRemainingPercent: "80",
        resetsAt: 100000 + 4 * 3600,
        status: "ready",
        reason: null,
      },
    ],
    nextEvent: null,
    dailyBudget: null,
  };
}
it("calculates model-specific intensity, hourly API equivalent and request count before a custom deadline", () => {
  const d = fixture();
  const target = d.asOf + 7200;
  expect(codexDeadlineForecast(d, target)).toMatchObject({
    status: "ready",
    basis: "reference",
    intensity: "10",
    usdPerHour: "20",
    requestsPerHour: "100",
    requestsPerDay: "2400",
  });
  const before = JSON.stringify(d);
  expect(codexDeadlineForecast(d, d.asOf)).toMatchObject({
    reason: "invalid_target_time",
  });
  expect(codexDeadlineForecast(d, null)).toMatchObject({
    reason: "invalid_target_time",
  });
  expect(codexDeadlineForecast(d, d.asOf + 5 * 3600)).toMatchObject({
    reason: "target_after_reset",
  });
  expect(JSON.stringify(d)).toBe(before);
});
it("uses weekly calibration when available and keeps target multipliers independent of simulated pace", () => {
  const d = fixture();
  d.calibration.windows = [
    {
      id: "weekly",
      durationMinutes: 10080,
      resetsAt: d.windows[0].resetsAt!,
      status: "ready",
      reason: null,
      observations: 3,
      modelPercentPerHour: "8",
    },
  ];
  const target = d.asOf + 7200;
  expect(codexDeadlineForecast(d, target)).toMatchObject({
    status: "ready",
    basis: "calibrated",
    intensity: "5",
    usdPerHour: "10",
    requestsPerHour: "50",
  });
  d.pace = "4";
  d.sample.rateUsdPerHour = "8";
  d.sample.requestsPerHour = "40";
  d.calibration.windows[0].modelPercentPerHour = "32";
  expect(codexDeadlineForecast(d, target)).toMatchObject({
    intensity: "5",
    usdPerHour: "10",
    requestsPerHour: "50",
  });
  d.sample.reason = "insufficient_samples";
  expect(codexDeadlineForecast(d, target).status).toBe("unavailable");
});
it("flags earlier quota constraints without inventing future replenishments", () => {
  const d = fixture();
  d.windows.push({
    id: "rolling",
    budgetUsd: "4",
    capacity: null,
    remainingUsd: "2",
    officialRemainingPercent: "50",
    resetsAt: d.asOf + 3600,
    status: "ready",
    reason: null,
  });
  const v = codexDeadlineForecast(d, d.asOf + 7200);
  expect(v.status).toBe("ready");
  if (v.status === "ready")
    expect(v.warnings).toEqual([
      "other_window_limits_target",
      "window_resets_before_target",
    ]);
  d.windows[0].remainingUsd = "0";
  expect(codexDeadlineForecast(d, d.asOf + 7200)).toMatchObject({
    status: "ready",
    intensity: "0",
    usdPerHour: "0",
  });
});
