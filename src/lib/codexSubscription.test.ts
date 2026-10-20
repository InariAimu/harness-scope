import { expect, it } from "vitest";
import { codexForecastSummary, codexWindowLabel } from "./codexSubscription";
import { accountError } from "./providerAccounts";
import type { CodexReference } from "@/types/providerAccounts";

it("labels actual Codex windows without inventing fixed five-hour or monthly limits", () => {
  expect(codexWindowLabel(10080, true)).toBe("每周窗口");
  expect(codexWindowLabel(300, false)).toBe("5-hour window");
  expect(codexWindowLabel(1440, true)).toBe("1 天窗口");
  expect(codexWindowLabel(15, false)).toBe("15-minute window");
  expect(codexWindowLabel(undefined, true)).toBe("额度窗口");
});

function reference(): CodexReference {
  return {
    asOf: 1000,
    model: "gpt-6.1-sol",
    models: ["gpt-6.1-sol"],
    plan: "plus",
    multiplier: 1,
    monthlyFeeUsd: "20",
    mode: "modeldial",
    catalogVersion: "fixture",
    catalogRevision: "fixture",
    quotaFetchedAt: 1000,
    bucket: "codex",
    pace: "1",
    sample: {
      reason: null,
      requests: 3,
      tokens: 300,
      unpricedRequests: 0,
      costUsd: "1",
      spanSeconds: 120,
      lastAt: 900,
      rateUsdPerHour: "1",
      requestsPerHour: "3",
    },
    calibration: {
      currentWindowTarget: null,
      identityVerified: true,
      historyPoints: 1,
      windows: [],
    },
    windows: [
      {
        id: "rolling",
        status: "ready",
        requiredIntensity: "2",
        requestsPerHour: "6",
        budgetUsd: "10",
        capacity: null,
        resetsAt: 2000,
        reason: null,
      },
      {
        id: "weekly",
        status: "ready",
        requiredIntensity: "4",
        requestsPerHour: "12",
        budgetUsd: "50",
        capacity: null,
        resetsAt: 3000,
        reason: null,
      },
    ],
    nextEvent: { kind: "reset", at: 2000, window: "rolling" },
    dailyBudget: null,
  };
}
it("summarizes the shared reference constraint while calibration is still collecting", () => {
  const data = reference();
  expect(codexForecastSummary(data)).toMatchObject({
    basis: "reference",
    intensity: "2",
    requestsPerHour: "6",
    durationMinutes: 300,
    next: { kind: "reset", at: 2000 },
  });
  data.windows[0].requiredIntensity = "0";
  expect(codexForecastSummary(data)?.intensity).toBe("0");
});
it("prefers complete calibration and preserves the distinction between exhaustion and reset", () => {
  const data = reference();
  data.calibration.currentWindowTarget = {
    intensity: "3",
    durationMinutes: 10080,
  };
  data.calibration.windows = [
    {
      id: "rolling",
      durationMinutes: 300,
      status: "ready",
      resetsAt: 2000,
      reason: null,
      observations: 3,
      runtimeSeconds: 100,
      coversReset: false,
      requiredIntensity: "5",
      requiredRequestsPerHour: "15",
    },
    {
      id: "weekly",
      durationMinutes: 10080,
      status: "ready",
      resetsAt: 3000,
      reason: null,
      observations: 3,
      runtimeSeconds: 300,
      coversReset: true,
      requiredIntensity: "3",
      requiredRequestsPerHour: "9",
      modelPercentPerHour: "2",
    },
  ];
  expect(codexForecastSummary(data)).toMatchObject({
    basis: "calibrated",
    intensity: "3",
    requestsPerHour: "9",
    percentPerHour: "2",
    next: { kind: "exhaustion", at: 1100 },
  });
  data.calibration.windows[0].status = "unavailable";
  expect(codexForecastSummary(data)?.basis).toBe("reference");
});
it("does not turn partial windows or invalid samples into a usable overview forecast", () => {
  const data = reference();
  data.windows[1].status = "unavailable";
  expect(codexForecastSummary(data)).toBeNull();
  data.windows[1].status = "ready";
  data.sample.reason = "insufficient_samples";
  expect(codexForecastSummary(data)).toBeNull();
});
it("explains local Codex login failures without suggesting an API key for subscription access", () => {
  const error = accountError(
    "Codex account query failed; check local ChatGPT login and Codex version",
    true,
  );
  expect(error).toContain("ChatGPT");
  expect(error).not.toContain("API Key");
  expect(accountError("Codex executable not found", true)).toContain(
    "codex.exe",
  );
});
