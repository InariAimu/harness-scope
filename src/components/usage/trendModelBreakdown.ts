import type { TrendModelStats } from "@/types/usage";
import { buildUsageComposition, usageCompositionColor, type UsageChartMetric } from "./composition";
import { parseFiniteNumber } from "./format";

function modelMetric(row: TrendModelStats, metric: UsageChartMetric): number | null {
  const value = metric === "cost"
    ? row.unpricedCount && row.unpricedCount === row.requestCount + (row.modelUsageCount ?? 0)
      ? null : parseFiniteNumber(row.totalCost)
    : metric === "tokens" ? row.totalTokens : row.requestCount;
  return value != null && Number.isFinite(value) && value >= 0 ? value : null;
}

export function buildTrendModelBreakdown(
  models: readonly TrendModelStats[] = [],
  metric: UsageChartMetric,
) {
  const values = models.map((row) => ({
    key: row.model,
    value: modelMetric(row, metric),
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

interface TrendStackBucket {
  modelBreakdown?: readonly TrendModelStats[];
  tokens: number;
  requests: number;
  cost: number | null;
  costConverted?: boolean;
}

export interface TrendModelSeries {
  id: string;
  dataKey: string;
  /** null is an explicit unallocated remainder, distinct from an empty model ID. */
  model: string | null;
  total: number;
  share: number;
  color: string;
}

interface ModelValue { value: number; converted: boolean }

/**
 * Every model gets a series; one global order is shared by all time buckets.
 * Retain an unallocated segment for old/incomplete responses so stacking never
 * changes the aggregate. Contradictory breakdowns are not rescaled into facts.
 */
export function buildTrendModelStacks<T extends TrendStackBucket>(
  buckets: readonly T[], metric: UsageChartMetric, costRate = 1,
) {
  const totals = new Map<string | null, number>();
  const values = buckets.map(bucket => {
    const total = bucket[metric];
    const models = new Map<string | null, ModelValue>();
    if (total == null || !Number.isFinite(total) || total < 0) return models;
    for (const row of bucket.modelBreakdown ?? []) {
      const value = modelMetric(row, metric);
      if (value == null || value === 0) continue;
      const old = models.get(row.model);
      models.set(row.model, { value: (old?.value ?? 0) + value, converted: Boolean(old?.converted || row.costConverted) });
    }
    const sum = [...models.values()].reduce((n, row) => n + row.value, 0);
    const tolerance = metric === "cost"
      ? Number.EPSILON * Math.max(1, total, sum) * Math.max(4, models.size * 2) : 0;
    const remainder = total - sum;
    if (remainder < -tolerance) {
      models.clear();
      if (total > 0) models.set(null, { value: total, converted: Boolean(bucket.costConverted) });
    } else if (remainder > tolerance) {
      models.set(null, { value: remainder, converted: Boolean(bucket.costConverted) });
    } else if (remainder !== 0 && models.size) {
      // Decimal costs enter the chart as JS numbers. Correct only arithmetic
      // roundoff, using the largest segment so a tiny one cannot turn negative.
      const largest = [...models.entries()].sort((a, b) => b[1].value - a[1].value)[0][1];
      largest.value += remainder;
    }
    for (const [model, row] of models) totals.set(model, (totals.get(model) ?? 0) + row.value);
    return models;
  });
  const total = [...totals.values()].reduce((sum, value) => sum + value, 0);
  const series: TrendModelSeries[] = [...totals.entries()]
    .filter(([, value]) => value > 0)
    .sort(([a, av], [b, bv]) => bv - av || (a == null ? 1 : b == null ? -1 : a.localeCompare(b)))
    .map(([model, value], i) => ({
      id: `model${i}`, dataKey: `plotValues.model${i}`, model, total: value,
      share: total > 0 ? value / total : 0,
      color: model == null ? "var(--text-3)" : usageCompositionColor(`models:${model}`),
    }));
  const data = buckets.map((bucket, i) => ({
    ...bucket,
    modelValues: Object.fromEntries(series.map(s => [s.id, bucket[metric] == null ? null : values[i].get(s.model)?.value ?? 0])) as Record<string, number | null>,
    plotValues: Object.fromEntries(series.map(s => {
      const value = bucket[metric] == null ? null : values[i].get(s.model)?.value ?? 0;
      return [s.id, value == null ? null : value * (metric === "cost" ? costRate : 1)];
    })) as Record<string, number | null>,
    modelCostConverted: Object.fromEntries(series.map(s => [s.id, values[i].get(s.model)?.converted ?? false])) as Record<string, boolean>,
  }));
  return { series, data, total };
}
