import { expect, it } from "vitest";
import { aggregateSummaries } from "./UsageHero";
import type { UsageSummary } from "@/types/usage";

it("counts model measurements separately and preserves incomplete cost coverage", () => {
  const models: UsageSummary = { totalRequests: 0, modelUsageCount: 2, incompleteUsageCount: 1, upperBoundCount: 1, totalCost: "0.0104", totalInputTokens: 1000, totalOutputTokens: 100, totalCacheCreationTokens: 0, totalCacheReadTokens: 0, successRate: null, realTotalTokens: 1100, cacheHitRate: 0, unpricedCount: 1, cacheBreakdownKnown: false };
  expect(aggregateSummaries([models])).toMatchObject({totalRequests:0, modelUsageCount:2, pricedCount:1, incompleteUsageCount:1, upperBoundCount:1, cacheBreakdownKnown:false});
});

it("preserves totals while marking mixed LM Studio cache coverage incomplete", () => {
  const known: UsageSummary = { totalRequests: 1, totalCost: "1", totalInputTokens: 10, totalOutputTokens: 5, totalCacheCreationTokens: 0, totalCacheReadTokens: 10, successRate: null, realTotalTokens: 25, cacheHitRate: 0.5 };
  const local: UsageSummary = { ...known, totalInputTokens: 20, totalCacheReadTokens: 0, cacheHitRate: 0, totalCost: "0", unpricedCount: 1, unpricedTokens:25, cacheBreakdownKnown: false };
  expect(aggregateSummaries([known, local])).toMatchObject({ totalRequests: 2, realTotalTokens: 50, totalCost: "1.000000", unpricedCount: 1, unpricedTokens:25, pricedCount:1, cacheBreakdownKnown: false });
  expect(aggregateSummaries([known]).cacheBreakdownKnown).toBe(true);
});
