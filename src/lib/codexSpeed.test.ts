import { expect, it } from "vitest";
import { codexSpeedForecast } from "./codexSpeed";
import type { CodexReference } from "@/types/providerAccounts";
import type { CodexSubscriptionSpeed } from "@/types/codexAllowances";

const rules: CodexSubscriptionSpeed = {
  sourceUrl: "https://learn.chatgpt.com/docs/pricing",
  checkedAt: "2026-10-20",
  fastMultiplier: "2.5",
  purchasedCreditsFastMultiplier: "2",
  fastModelIds: ["gpt-6.1-sol"],
};
function fixture(): CodexReference {
  return {
    asOf: 100000,
    model: "openai/gpt-6.1-sol",
    allowanceModel: "gpt-6.1-sol",
    models: ["openai/gpt-6.1-sol"],
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
        budgetUsd: "100",
        capacity: null,
        remainingUsd: "50",
        officialRemainingPercent: "50",
        resetsAt: 100000 + 48 * 3600,
        status: "ready",
        reason: null,
      },
    ],
    nextEvent: null,
    dailyBudget: null,
  };
}
it("uses 2.5x included quota rather than 2x purchased credits and leaves API-equivalent costs intact", () => {
  const d = fixture();
  const before = JSON.stringify(d);
  const assumption = { basis: "reference", baseline: "standard" } as const;
  const standard = codexSpeedForecast(
    d,
    rules,
    assumption,
    "standard",
    d.asOf + 5 * 3600,
  );
  const fast = codexSpeedForecast(
    d,
    rules,
    assumption,
    "fast",
    d.asOf + 5 * 3600,
  );
  expect(standard).toMatchObject({
    status: "ready",
    percentPerHour: "2",
    target: { intensity: "5", requestsPerHour: "50" },
  });
  expect(fast).toMatchObject({
    status: "ready",
    percentPerHour: "5",
    target: { intensity: "2", requestsPerHour: "20" },
  });
  if (standard.status === "ready" && fast.status === "ready")
    expect((standard.next.at - d.asOf) / (fast.next.at - d.asOf)).toBe(2.5);
  expect(JSON.stringify(d)).toBe(before);
});
it("does not multiply Fast calibration a second time", () => {
  const d = fixture();
  d.calibration.windows = [
    {
      id: "weekly",
      durationMinutes: 10080,
      resetsAt: d.windows[0].resetsAt!,
      status: "ready",
      reason: null,
      observations: 3,
      modelPercentPerHour: "5",
    },
  ];
  const assumption = { basis: "calibrated", baseline: "fast" } as const;
  expect(
    codexSpeedForecast(d, rules, assumption, "fast", d.asOf + 5 * 3600),
  ).toMatchObject({
    status: "ready",
    relativeMultiplier: "1",
    percentPerHour: "5",
    target: { intensity: "2" },
  });
  expect(
    codexSpeedForecast(d, rules, assumption, "standard", d.asOf + 5 * 3600),
  ).toMatchObject({
    status: "ready",
    relativeMultiplier: "0.4",
    percentPerHour: "2",
    target: { intensity: "5" },
  });
});
it("keeps mixed baselines, missing calibration and unsupported models unknown", () => {
  const d = fixture();
  const target = d.asOf + 3600;
  expect(
    codexSpeedForecast(
      d,
      rules,
      { basis: "calibrated", baseline: "unknown" },
      "fast",
      target,
    ),
  ).toMatchObject({ reason: "unknown_speed_baseline" });
  expect(
    codexSpeedForecast(
      d,
      rules,
      { basis: "calibrated", baseline: "standard" },
      "fast",
      target,
    ),
  ).toMatchObject({ reason: "insufficient_quota_history" });
  d.allowanceModel = "unknown-model";
  expect(
    codexSpeedForecast(
      d,
      rules,
      { basis: "reference", baseline: "standard" },
      "fast",
      target,
    ),
  ).toMatchObject({ reason: "unsupported_fast_model" });
});
it("keeps intensity relative to the original sample and guards target times and expired quotas", () => {
  const d = fixture();
  d.pace = "4";
  d.sample.rateUsdPerHour = "8";
  d.sample.requestsPerHour = "40";
  const a = { basis: "reference", baseline: "standard" } as const;
  expect(
    codexSpeedForecast(d, rules, a, "fast", d.asOf + 5 * 3600),
  ).toMatchObject({
    status: "ready",
    percentPerHour: "20",
    target: { intensity: "2", requestsPerHour: "20" },
  });
  expect(codexSpeedForecast(d, rules, a, "fast", d.asOf)).toMatchObject({
    status: "ready",
    target: { reason: "invalid_target_time" },
  });
  expect(
    codexSpeedForecast(d, rules, a, "fast", d.asOf + 49 * 3600),
  ).toMatchObject({
    status: "ready",
    target: { reason: "target_after_reset" },
  });
  d.windows[0].status = "unavailable";
  d.windows[0].reason = "expired_window";
  expect(codexSpeedForecast(d, rules, a, "fast", d.asOf + 3600)).toMatchObject({
    status: "unavailable",
    reason: "expired_window",
  });
});
it("keeps earlier windows as constraints and does not invent speed improvements", () => {
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
  const v = codexSpeedForecast(
    d,
    rules,
    { basis: "reference", baseline: "standard" },
    "fast",
    d.asOf + 5 * 3600,
  );
  expect(v).toMatchObject({
    status: "ready",
    target: {
      warnings: ["other_window_limits_target", "window_resets_before_target"],
    },
  });
  if (v.status === "ready") expect(v.next.kind).toBe("exhaustion");
  expect("tokensPerSecond" in v).toBe(false);
});
