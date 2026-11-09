import { useCallback, useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useUsageFirstDate, useUsageTrends } from "@/lib/query/usage";
import { useUsageTimezone } from "@/lib/query/usageTimezone";
import { usageCalendarDate, usageTimestamp } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";
import type { UsageTrendStatLike } from "./UsageTrendChart";
import type { UsageChartMetric } from "./composition";
import { parseFiniteNumber } from "./format";
import { UsageTooltipCard } from "./UsageTooltipCard";
import { CalendarHeatmap } from "./CalendarHeatmap";
import { calendarDayKey as dayKey, calendarSpanStart as spanStart, calendarSpanCount as spanCountFor, calendarStartOfDay as startOfDay } from "./heatmapCalendar";

type HeatMetric = UsageChartMetric;
interface UsageHeatmapProps {
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  metric: UsageChartMetric;
  controls: ReactNode;
  composition?: ReactNode;
  title?: string;
}

function metricValue(stat: UsageTrendStatLike, metric: HeatMetric): number {
  if (metric === "requests") return stat.requestCount ?? 0;
  if (metric === "cost") return parseFiniteNumber(stat.totalCost) ?? 0;
  return (
    stat.totalInputTokens +
    stat.totalOutputTokens +
    stat.totalCacheCreationTokens +
    stat.totalCacheReadTokens
  );
}

function useAllSpansDailyTrends(
  filters: { appType?: string; providerName?: string; model?: string },
  refreshIntervalMs: number,
) {
  const refetchInterval = refreshIntervalMs > 0 ? refreshIntervalMs : false;
  const timeZone = useUsageTimezone();
  const firstDateQuery = useUsageFirstDate(filters, { refetchInterval });
  const today = startOfDay(usageCalendarDate(Date.now() / 1000, timeZone));
  const spanCount = firstDateQuery.isSuccess
    ? spanCountFor(firstDateQuery.data, today)
    : null;
  const startKey = dayKey(spanStart(today, (spanCount ?? 1) - 1));
  const selection = useMemo<UsageRangeSelection>(
    () => ({
      preset: "custom",
      customStartDate: (() => {
        const start = spanStart(today, (spanCount ?? 1) - 1);
        return usageTimestamp(
          {
            year: start.getFullYear(),
            month: start.getMonth() + 1,
            day: start.getDate(),
          },
          timeZone,
        );
      })(),
      liveEndTime: true,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [startKey, timeZone],
  );
  const trendsQuery = useUsageTrends(selection, filters, {
    enabled: spanCount != null,
    refetchInterval,
  });
  return {
    spanCount: spanCount ?? 1,
    trends: trendsQuery.data,
    isLoading: firstDateQuery.isPending || trendsQuery.isLoading,
  };
}

export function UsageHeatmap({ appType, providerName, model, refreshIntervalMs, metric, controls, composition, title }: UsageHeatmapProps) {
  const { t } = useTranslation();
  const formatQuantity = useUsageNumberFormat();
  const { costMoney } = useMoneyFormat();
  const timeZone = useUsageTimezone();
  const { spanCount, trends, isLoading } = useAllSpansDailyTrends({ appType, providerName, model }, refreshIntervalMs);
  const entries = useMemo(() => (trends ?? []).map(stat => ({ ...stat, date: dayKey(usageCalendarDate(Date.parse(stat.date) / 1000, timeZone)) })), [trends, timeZone]);
  const valueOf = useCallback((stat: UsageTrendStatLike) => {
    const value = metricValue(stat, metric);
    return value > 0 ? value : null;
  }, [metric]);
  return <CalendarHeatmap entries={entries} today={startOfDay(usageCalendarDate(Date.now() / 1000, timeZone))}
    spanCount={spanCount} valueOf={valueOf} isLoading={isLoading} controls={controls} composition={composition}
    title={title ?? t("usage.heatmap.title")} help={t("usage.heatmap.subtitle")} testId="usage-calendar"
    renderSummary={rows => {
      const totals = rows.reduce((sum, row) => ({ tokens: sum.tokens + metricValue(row, "tokens"), requests: sum.requests + (row.requestCount ?? 0),
        cost: sum.cost + metricValue(row, "cost"), costConverted: sum.costConverted || Boolean(row.costConverted) }), { tokens: 0, requests: 0, cost: 0, costConverted: false });
      return t("usage.heatmap.spanSummary", { tokens: formatQuantity(totals.tokens), requests: formatQuantity(totals.requests),
        cost: appType === "lmstudio" && totals.requests > 0 ? t("usage.unpriced") : costMoney(totals.cost, 2, totals.costConverted) });
    }}
    renderTooltip={(heading, row) => <UsageTooltipCard heading={heading} tokens={row ? metricValue(row, "tokens") : 0}
      requests={row?.requestCount ?? 0} cost={appType === "lmstudio" && (row?.requestCount ?? 0) > 0 ? null : row ? metricValue(row, "cost") : 0}
      costConverted={row?.costConverted} />} />;
}
