import { expect, it } from "vitest";
import type { ModelStats } from "@/types/usage";
import { groupModelStats, modelStatsRowKey } from "./modelStats";

const row = (model: string, requestMode: string | null, requestCount: number): ModelStats => ({
  model, requestMode, requestCount, totalTokens: requestCount * 100,
  totalCost: "0.1", avgCostPerRequest: "0.01", successRate: null,
});

it("sorts whole models by their combined requests, keeping modes together in a stable order", () => {
  const rows = [row("gpt-one", "unknown", 3), row("claude", null, 7),
    row("gpt-one", "fast", 1), row("gpt-one", "flex", 1), row("gpt-one", "standard", 4)];
  const original = JSON.stringify(rows);
  const groups = groupModelStats(rows);
  expect(groups.map((g) => g.model)).toEqual(["gpt-one", "claude"]);
  expect(groups[0].requests).toBe(9);
  expect(groups[0].rows.map((r) => r.requestMode)).toEqual(["standard", "fast", "flex", "unknown"]);
  expect(groups.slice(0, 1).flatMap((g) => g.rows)).toHaveLength(4);
  expect(JSON.stringify(rows)).toBe(original);
});

it("keeps model names intact and gives modes different keys, including older ungrouped responses", () => {
  const rows = [row("gpt-one", "standard", 1), row("gpt-one", "fast", 1), row("gpt-one", "unknown", 1)];
  expect(new Set(rows.map(modelStatsRowKey)).size).toBe(3);
  const old = row("original-name", null, 2);
  expect(groupModelStats([old])[0].rows).toEqual([old]);
  expect(modelStatsRowKey({ ...old, rowKey: "backend-key" })).toBe("backend-key");
});
