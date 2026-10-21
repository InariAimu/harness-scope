import { describe, expect, it } from "vitest";
import type { UsageSummaryByApp } from "@/types/usage";
import { buildHarnessComposition, buildUsageComposition, usageCompositionColor } from "./composition";

const harnessSummaries: UsageSummaryByApp[] = [
  {
    appType: "codex",
    summary: {
      totalRequests: 20, totalCost: "2", totalInputTokens: 10, totalOutputTokens: 20,
      totalCacheCreationTokens: 30, totalCacheReadTokens: 40, realTotalTokens: 100,
      cacheHitRate: 0.5, successRate: null,
    },
  },
  {
    appType: "future-harness",
    summary: {
      totalRequests: 5, totalCost: "1", totalInputTokens: 50, totalOutputTokens: 5,
      totalCacheCreationTokens: 10, totalCacheReadTokens: 235, realTotalTokens: 300,
      cacheHitRate: 235 / 295, successRate: null, costConverted: true,
    },
  },
];

describe("usage distribution", () => {
  it("keeps cache-inclusive harness totals, requests, and costs consistent with the summary", () => {
    const tokens = buildHarnessComposition(harnessSummaries, "tokens", "all");
    expect(tokens.total).toBe(400);
    expect(tokens.slices).toMatchObject([
      { members: ["future-harness"], value: 300, share: 0.75 },
      { members: ["codex"], value: 100, share: 0.25 },
    ]);
    const requests = buildHarnessComposition(harnessSummaries, "requests");
    expect(requests.total).toBe(25);
    expect(requests.slices[0]).toMatchObject({ members: ["codex"], value: 20, share: 0.8 });
    const cost = buildHarnessComposition(harnessSummaries, "cost");
    expect(cost.total).toBe(3);
    expect(cost.costConverted).toBe(true);
    expect(cost.slices[0]).toMatchObject({ members: ["codex"], value: 2, share: 2 / 3 });
  });

  it("applies the selected harness without dropping unknown harness IDs from All", () => {
    expect(buildHarnessComposition(harnessSummaries, "tokens", "all"))
      .toEqual(buildHarnessComposition(harnessSummaries, "tokens"));
    const selected = buildHarnessComposition(harnessSummaries, "tokens", "future-harness");
    expect(selected.total).toBe(300);
    expect(selected.slices).toMatchObject([{ members: ["future-harness"], share: 1 }]);
    expect(selected.slices).toHaveLength(1);
    expect(buildHarnessComposition(harnessSummaries, "cost", "missing"))
      .toMatchObject({ total: 0, slices: [] });
  });

  it("merges model modes or provider apps before selecting the largest categories", () => {
    const result = buildUsageComposition([
      { key: "model-a", value: 30 },
      { key: "model-b", value: 45 },
      { key: "model-a", value: 20, costConverted: true },
      { key: "model-c", value: 5 },
    ], 1);
    expect(result.total).toBe(100);
    expect(result.slices[0]).toMatchObject({ members: ["model-a"], value: 50, share: 0.5, costConverted: true });
    expect(result.slices[1]).toMatchObject({ other: true, value: 50, share: 0.5, members: ["model-b", "model-c"] });
    expect(result.costConverted).toBe(true);
  });

  it("keeps all valid costs in the denominator and does not manufacture missing costs", () => {
    const result = buildUsageComposition([
      { key: "free", value: 0 }, { key: "unknown", value: null },
      { key: "invalid", value: NaN }, { key: "infinite", value: Infinity },
      { key: "negative", value: -1 }, { key: "tiny", value: 0.000001 },
      { key: "paid", value: 0.1 },
    ]);
    expect(result.total).toBeCloseTo(0.100001, 8);
    expect(result.slices).toHaveLength(2);
    expect(result.slices.reduce((sum, slice) => sum + slice.share, 0)).toBeCloseTo(1);
    expect(result.slices[1].value).toBe(0.000001);
    expect(buildUsageComposition([{ key: "free", value: 0 }, { key: "unknown", value: null }])).toMatchObject({ total: 0, slices: [] });
  });

  it("never confuses an actual Other or empty category name with the combined remainder", () => {
    const result = buildUsageComposition([
      { key: "Other", value: 5 }, { key: "", value: 3 }, { key: "x", value: 2 },
    ], 1);
    expect(result.slices[0].other).toBe(false);
    expect(result.slices[1].other).toBe(true);
    expect(result.slices[0].key).not.toBe(result.slices[1].key);
    expect(result.slices[1].members).toEqual(["", "x"]);
    expect(buildUsageComposition([{ key: "", value: 3 }]).slices[0].members).toEqual([""]);
  });

  it("sorts deterministically and retains colors across metric rank changes", () => {
    const rows = [{ key: "a", value: 2 }, { key: "b", value: 2 }];
    expect(buildUsageComposition(rows)).toEqual(buildUsageComposition([...rows].reverse()));
    expect(usageCompositionColor("models:a")).not.toBe(usageCompositionColor("models:b"));
  });
});
