import { expect, it } from "vitest";
import { aggregateSummaries } from "./UsageHero";
import type { UsageSummary } from "@/types/usage";

it("preserves totals while marking mixed LM Studio cache coverage incomplete", () => {
  const known: UsageSummary = { totalRequests: 1, totalCost: "1", totalInputTokens: 10, totalOutputTokens: 5, totalCacheCreationTokens: 0, totalCacheReadTokens: 10, successRate: null, realTotalTokens: 25, cacheHitRate: 0.5 };
  const local: UsageSummary = { ...known, totalInputTokens: 20, totalCacheReadTokens: 0, cacheHitRate: 0, totalCost: "0", unpricedCount: 1, cacheBreakdownKnown: false };
  expect(aggregateSummaries([known, local])).toMatchObject({ totalRequests: 2, realTotalTokens: 50, totalCost: "1.000000", unpricedCount: 1, cacheBreakdownKnown: false });
  expect(aggregateSummaries([known]).cacheBreakdownKnown).toBe(true);
});
