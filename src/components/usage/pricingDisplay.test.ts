import { expect, it } from "vitest";
import { formatCostMoney, USD_DISPLAY } from "./format";
import { aggregateSummaries } from "./UsageHero";
import type { UsageSummary } from "@/types/usage";

it("marks either direction of currency conversion once and leaves unknown costs unknown", () => {
  expect(formatCostMoney("1", 4, USD_DISPLAY, true)).toBe("≈$1.0000");
  expect(formatCostMoney("1", 4, USD_DISPLAY)).toBe("$1.0000");
  expect(
    formatCostMoney("1", 4, { currency: "CNY", rate: 6.6824901 }, true),
  ).toBe("≈¥6.68");
  expect(formatCostMoney("0", 4, USD_DISPLAY, true)).toBe("≈$0.0000");
  for (const invalid of [null, undefined, "bad", NaN])
    expect(formatCostMoney(invalid, 4, USD_DISPLAY, true)).toBe("--");
});

it("retains conversion provenance when merging application summaries", () => {
  const item: UsageSummary = {
    totalRequests: 1,
    totalCost: "1",
    totalInputTokens: 10,
    totalOutputTokens: 2,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
    successRate: null,
    realTotalTokens: 12,
    cacheHitRate: 0,
  };
  expect(aggregateSummaries([item]).costConverted).toBe(false);
  const combined = aggregateSummaries([item, { ...item, costConverted: true }]);
  expect(combined.costConverted).toBe(true);
  expect(combined.totalCost).toBe("2.000000");
});
