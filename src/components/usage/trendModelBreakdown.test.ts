import { describe, expect, it } from "vitest";
import type { TrendModelStats } from "@/types/usage";
import { buildTrendModelBreakdown, buildTrendModelStacks } from "./trendModelBreakdown";

const models: TrendModelStats[] = [
  { model: "a", requestCount: 1, totalTokens: 40, totalCost: "0.01" },
  { model: "b", requestCount: 5, totalTokens: 30, totalCost: "0.02" },
  { model: "c", requestCount: 3, totalTokens: 20, totalCost: "0.03" },
  { model: "d", requestCount: 2, totalTokens: 6, totalCost: "0.04", costConverted: true },
  { model: "e", requestCount: 4, totalTokens: 4, totalCost: "0.05" },
];

describe("trend model breakdown", () => {
  it("does not label unpriced models as a free zero-cost slice", () => {
    const unknown = {...models[0],totalCost:"0",unpricedCount:1};
    expect(buildTrendModelBreakdown([unknown],"cost").slices).toEqual([]);
    expect(buildTrendModelBreakdown([unknown],"tokens").total).toBe(40);
    expect(buildTrendModelBreakdown([unknown,models[1]],"cost").total).toBe(0.02);
  });
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

function bucket(rows: TrendModelStats[]) {
  return {
    tokens: rows.reduce((n, r) => n + r.totalTokens, 0),
    requests: rows.reduce((n, r) => n + r.requestCount, 0),
    cost: rows.reduce((n, r) => n + Number(r.totalCost), 0),
    modelBreakdown: rows,
  };
}

describe("stacked model trends", () => {
  it("uses one range-wide ranking, retains every model, and reconciles every bar", () => {
    const buckets = [bucket(models), bucket([{ ...models[1], totalTokens: 80 }, { ...models[0], totalTokens: 5 }])];
    const before = JSON.stringify(buckets);
    const result = buildTrendModelStacks(buckets, "tokens");
    expect(result.series.map(s => s.model)).toEqual(["b", "a", "c", "d", "e"]);
    expect(result.series.map(s => s.total)).toEqual([110, 45, 20, 6, 4]);
    expect(result.series.reduce((sum, s) => sum + s.share, 0)).toBeCloseTo(1);
    result.data.forEach((row, i) => {
      expect(Object.values(row.modelValues).reduce<number>((sum, v) => sum + (v ?? 0), 0)).toBe(buckets[i].tokens);
      expect(Object.values(row.plotValues)).toEqual(Object.values(row.modelValues));
    });
    expect(result.data[1].modelValues[result.series.find(s => s.model === "c")!.id]).toBe(0);
    expect(JSON.stringify(buckets)).toBe(before);
  });

  it("reranks each metric while keeping model colors stable across filters and ordering", () => {
    const input = [bucket(models)];
    const tokens = buildTrendModelStacks(input, "tokens");
    const requests = buildTrendModelStacks(input, "requests");
    const cost = buildTrendModelStacks(input, "cost");
    expect(requests.series.map(s => s.model)).toEqual(["b", "e", "c", "d", "a"]);
    expect(cost.series.map(s => s.model)).toEqual(["e", "d", "c", "b", "a"]);
    for (const result of [requests, cost, buildTrendModelStacks([bucket([...models].reverse())], "tokens"), buildTrendModelStacks([bucket([models[2]])], "tokens")]) {
      for (const s of result.series) expect(s.color).toBe(tokens.series.find(t => t.model === s.model)?.color);
    }
    expect(Object.values(requests.data[0].modelValues).reduce<number>((sum, v) => sum + (v ?? 0), 0)).toBe(15);
    expect(Object.values(cost.data[0].modelValues).reduce<number>((sum, v) => sum + (v ?? 0), 0)).toBeCloseTo(0.15);
    expect(cost.data[0].modelCostConverted[cost.series.find(s => s.model === "d")!.id]).toBe(true);
  });

  it("keeps empty buckets and unknown costs distinct from confirmed zero", () => {
    const unknown = { ...models[0], unpricedCount: 1, totalCost: "0" };
    const result = buildTrendModelStacks([
      { ...bucket([unknown]), cost: null },
      bucket([]),
      bucket([models[1], unknown]),
    ], "cost");
    expect(result.series.map(s => s.model)).toEqual(["b"]);
    expect(Object.values(result.data[0].modelValues)).toEqual([null]);
    expect(Object.values(result.data[0].plotValues)).toEqual([null]);
    expect(Object.values(result.data[1].modelValues)).toEqual([0]);
    expect(result.data[2].modelValues[result.series[0].id]).toBe(0.02);
    expect(buildTrendModelStacks([], "tokens")).toMatchObject({ data: [], series: [] });
  });

  it("preserves legacy aggregates and explicit remainders without fabricating model allocations", () => {
    const data = [
      { tokens: 100, requests: 1, cost: 1 },
      { ...bucket([models[0]]), tokens: 50 },
      { ...bucket([models[1]]), tokens: 10 },
    ];
    const result = buildTrendModelStacks(data, "tokens");
    const remainder = result.series.find(s => s.model == null)!;
    const a = result.series.find(s => s.model === "a")!;
    expect(result.data[0].modelValues[remainder.id]).toBe(100);
    expect(result.data[1].modelValues[remainder.id]).toBe(10);
    expect(result.data[1].modelValues[a.id]).toBe(40);
    expect(result.data[2].modelValues[remainder.id]).toBe(10);
    expect(result.series.some(s => s.model === "b")).toBe(false);
    result.data.forEach((row, i) => expect(Object.values(row.modelValues).reduce<number>((sum, v) => sum + (v ?? 0), 0)).toBe(data[i].tokens));
  });

  it("corrects only cost roundoff and scales plot values without changing tooltip amounts", () => {
    const rows = [{ ...models[0], totalCost: "0.1" }, { ...models[1], totalCost: "0.2" }];
    const result = buildTrendModelStacks([{ ...bucket(rows), cost: 0.3 }], "cost", 7.2);
    expect(result.series.map(s => s.model)).toEqual(["b", "a"]);
    expect(Object.values(result.data[0].modelValues).reduce<number>((sum, v) => sum + (v ?? 0), 0)).toBeCloseTo(0.3, 14);
    expect(Object.values(result.data[0].plotValues).reduce<number>((sum, v) => sum + (v ?? 0), 0)).toBeCloseTo(2.16, 14);
  });

  it("uses opaque series keys for model names containing dots or object-property names", () => {
    const rows = ["vendor/model.v1", "__proto__", "constructor", ""].map(model => ({ ...models[0], model }));
    const result = buildTrendModelStacks([bucket(rows)], "tokens");
    expect(result.series).toHaveLength(4);
    expect(new Set(result.series.map(s => s.dataKey)).size).toBe(4);
    expect(Object.values(result.data[0].modelValues)).toEqual([40, 40, 40, 40]);
    expect(result.series.some(s => s.model == null)).toBe(false);
  });
});
