import type { TrendModelStats } from "@/types/usage";
import { buildUsageComposition, type UsageChartMetric } from "./composition";
import { parseFiniteNumber } from "./format";

export function buildTrendModelBreakdown(
  models: readonly TrendModelStats[] = [],
  metric: UsageChartMetric,
) {
  const values = models.map((row) => ({
    key: row.model,
    value: metric === "cost" ? parseFiniteNumber(row.totalCost)
      : metric === "tokens" ? row.totalTokens : row.requestCount,
    costConverted: row.costConverted,
  }));
  const result = buildUsageComposition(values, 3);
  // Keep a sole zero-valued model visible using the same row layout.
  if (new Set(values.map((row) => row.key)).size === 1 && values.every((row) => row.value === 0)) {
    result.costConverted = values.some((row) => row.costConverted);
    result.slices.push({
      key: JSON.stringify(["item", values[0].key]),
      value: 0,
      share: 1,
      costConverted: result.costConverted,
      members: [values[0].key],
      other: false,
    });
  }
  return result;
}
