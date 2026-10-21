import { describe, expect, it } from "vitest";
import { buildUsageComposition, usageCompositionColor } from "./composition";

describe("usage distribution", () => {
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
