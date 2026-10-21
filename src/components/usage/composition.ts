import type { UsageSummaryByApp } from "@/types/usage";
import { parseFiniteNumber } from "./format";

export type UsageChartMetric = "tokens" | "requests" | "cost";
export type UsageChartView = "trend" | "models" | "providers" | "harnesses";

export interface CompositionValue {
  key: string;
  value: number | null;
  costConverted?: boolean;
}

export interface CompositionSlice {
  key: string;
  value: number;
  share: number;
  costConverted: boolean;
  /** Original names, including every category combined into Other. */
  members: string[];
  other: boolean;
}

/** Aggregate modes/apps before ranking; Other never changes the denominator. */
export function buildUsageComposition(values: readonly CompositionValue[], limit = 5) {
  const groups = new Map<string, { value: number; costConverted: boolean }>();
  for (const row of values) {
    if (row.value == null || !Number.isFinite(row.value) || row.value < 0) continue;
    const group = groups.get(row.key) ?? { value: 0, costConverted: false };
    group.value += row.value;
    group.costConverted ||= Boolean(row.costConverted);
    groups.set(row.key, group);
  }
  const ranked = [...groups.entries()]
    .filter(([, group]) => group.value > 0)
    .sort(([a, av], [b, bv]) => bv.value - av.value || a.localeCompare(b));
  const total = ranked.reduce((sum, [, group]) => sum + group.value, 0);
  const count = Math.max(1, Math.floor(limit));
  const slices: CompositionSlice[] = ranked.slice(0, count).map(([key, group]) => ({
    key: JSON.stringify(["item", key]),
    ...group,
    share: group.value / total,
    members: [key],
    other: false,
  }));
  const remaining = ranked.slice(count);
  if (remaining.length) {
    const value = remaining.reduce((sum, [, group]) => sum + group.value, 0);
    slices.push({
      key: JSON.stringify(["other"]),
      value,
      share: value / total,
      costConverted: remaining.some(([, group]) => group.costConverted),
      members: remaining.map(([key]) => key),
      other: true,
    });
  }
  return {
    total,
    costConverted: ranked.some(([, group]) => group.costConverted),
    slices,
  };
}

/** Use the same cache-inclusive totals and app filter as the summary cards. */
export function buildHarnessComposition(
  rows: readonly UsageSummaryByApp[],
  metric: UsageChartMetric,
  appType?: string,
) {
  return buildUsageComposition(rows
    .filter(row => !appType || appType === "all" || row.appType === appType)
    .map(({ appType: key, summary }) => ({
      key,
      value: metric === "cost" ? parseFiniteNumber(summary.totalCost)
        : metric === "tokens" ? summary.realTotalTokens : summary.totalRequests,
      costConverted: summary.costConverted,
    })));
}

/** Category colors remain stable when sorting, filtering, or changing metrics. */
export function usageCompositionColor(key: string) {
  let hash = 2166136261;
  for (const char of key) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619);
  return `hsl(${(hash >>> 0) % 360} 58% var(--chart-category-lightness))`;
}
