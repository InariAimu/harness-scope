import { describe, expect, it } from "vitest";
import type { TrendModelStats } from "@/types/usage";
import { buildTrendModelBreakdown } from "./trendModelBreakdown";

const models: TrendModelStats[] = [
  { model: "a", requestCount: 1, totalTokens: 40, totalCost: "0.01" },
  { model: "b", requestCount: 5, totalTokens: 30, totalCost: "0.02" },
  { model: "c", requestCount: 3, totalTokens: 20, totalCost: "0.03" },
  { model: "d", requestCount: 2, totalTokens: 6, totalCost: "0.04", costConverted: true },
  { model: "e", requestCount: 4, totalTokens: 4, totalCost: "0.05" },
];

describe("trend model breakdown", () => {
  it("keeps the complete denominator when showing Top 3 plus Other", () => {
    const result = buildTrendModelBreakdown(models, "tokens");
    expect(result.total).toBe(100);
    expect(result.slices).toMatchObject([
      { members: ["a"], value: 40, share: 0.4 },
      { members: ["b"], value: 30, share: 0.3 },
      { members: ["c"], value: 20, share: 0.2 },
      { members: ["d", "e"], value: 10, share: 0.1, other: true, costConverted: true },
    ]);
  });

  it("reranks by requests or cost and preserves converted-cost flags", () => {
    const requests = buildTrendModelBreakdown(models, "requests");
    expect(requests.total).toBe(15);
    expect(requests.slices.map((s) => s.members)).toEqual([["b"], ["e"], ["c"], ["d", "a"]]);
    const cost = buildTrendModelBreakdown(models, "cost");
    expect(cost.total).toBeCloseTo(0.15);
    expect(cost.slices.map((s) => s.members)).toEqual([["e"], ["d"], ["c"], ["b", "a"]]);
    expect(cost.slices[1]).toMatchObject({ value: 0.04, costConverted: true });
    expect(cost.slices.reduce((sum, s) => sum + s.share, 0)).toBeCloseTo(1);
  });

  it("combines duplicate models before ranking without mutating source data", () => {
    const rows = [...models, { ...models[1], totalTokens: 20 }];
    const before = JSON.stringify(rows);
    expect(buildTrendModelBreakdown(rows, "tokens").slices[0])
      .toMatchObject({ members: ["b"], value: 50, share: 50 / 120 });
    expect(JSON.stringify(rows)).toBe(before);
  });

  it("shows a full row with 100% for a sole model, including zero cost", () => {
    expect(buildTrendModelBreakdown([models[0]], "tokens").slices)
      .toMatchObject([{ members: ["a"], value: 40, share: 1, other: false }]);
    expect(buildTrendModelBreakdown([{ ...models[0], totalCost: "0", costConverted: true }], "cost"))
      .toMatchObject({ total: 0, slices: [{ members: ["a"], value: 0, share: 1, costConverted: true }] });
    expect(buildTrendModelBreakdown([{ ...models[0], model: "" }], "tokens").slices[0].members).toEqual([""]);
  });

  it("supports empty buckets and older responses without inventing percentages", () => {
    expect(buildTrendModelBreakdown(undefined, "tokens"))
      .toMatchObject({ slices: [], total: 0 });
    expect(buildTrendModelBreakdown([{ ...models[0], totalCost: "invalid" }], "cost"))
      .toMatchObject({ slices: [], total: 0 });
    expect(buildTrendModelBreakdown(models.map((row) => ({ ...row, totalCost: "0" })), "cost"))
      .toMatchObject({ slices: [], total: 0 });
  });
});
