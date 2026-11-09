import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useSpeedDaily } from "@/lib/query/usage";
import { useUsageTimezone } from "@/lib/query/usageTimezone";
import { usageCalendarDate } from "@/lib/usageRange";
import type { UsageScopeFilters } from "@/types/usage";
import type { SpeedDay, SpeedDailyTotals } from "@/types/speed";
import { CalendarHeatmap } from "./CalendarHeatmap";
import { calendarStartOfDay } from "./heatmapCalendar";
import { getLocaleFromLanguage } from "./format";
import { dailySpeedMean, sumSpeedDays } from "./speedDaily";
import type { SpeedMetric } from "./speedChart";

export function SpeedHeatmap(filters: UsageScopeFilters) {
  const { t } = useTranslation();
  const query = useSpeedDaily(filters);
  return <div id="main-content" className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto scroll-stable px-6 pb-6 pt-1" aria-busy={query.isFetching}>
    {query.isError ? <div role="alert" className="rounded-panel border border-border p-5 text-body">
      <p>{t("speed.error")}</p><p className="mt-1 break-words text-caption text-fg-3">{String(query.error)}</p>
      <Button variant="neutral" className="mt-3" onClick={() => void query.refetch()}>{t("speed.retry")}</Button>
    </div> : !query.data ? <div role="status" className="flex items-center justify-center gap-2 py-16 text-body text-fg-2"><Loader2 className="h-4 w-4 animate-spin" />{t("common.loading")}</div> : <>
      <div className="flex flex-wrap items-center gap-x-4 text-caption text-fg-2">
        <span>{t("speed.basis")}</span><span>{t("speed.heatmap.daily")}</span>
        {query.isFetching && <span role="status">{t("speed.updating")}</span>}
      </div>
      {!query.data.days.length && <p role="status" className="text-body text-fg-2">{t("speed.empty")}</p>}
      <DailySpeedHeatmap days={query.data.days} metric="speed" />
      <DailySpeedHeatmap days={query.data.days} metric="ttft" />
      <p className="text-caption text-fg-3">{t("speed.coverage", { valid: query.data.selectedRecordCount - query.data.excludedRecordCount, total: query.data.selectedRecordCount, excluded: query.data.excludedRecordCount })}</p>
    </>}
  </div>;
}

export function DailySpeedHeatmap({ days, metric }: { days: SpeedDay[]; metric: SpeedMetric }) {
  const { t, i18n } = useTranslation();
  const timeZone = useUsageTimezone();
  const locale = getLocaleFromLanguage(i18n.resolvedLanguage || i18n.language);
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: metric === "speed" ? 1 : 3 });
  const valueOf = useCallback((day: SpeedDay) => dailySpeedMean(day, metric), [metric]);
  const format = (row?: SpeedDailyTotals) => {
    const mean = dailySpeedMean(row, metric);
    return mean == null ? "—" : `${row!.estimatedSampleCount > 0 ? "≈" : ""}${number.format(mean)} ${metric === "speed" ? "tok/s" : "s"}`;
  };
  const meanLabel = t(metric === "speed" ? "speed.weightedMean" : "speed.mean");
  return <CalendarHeatmap entries={days} today={calendarStartOfDay(usageCalendarDate(Date.now() / 1000, timeZone))}
    valueOf={valueOf} lowerIsDarker={metric === "ttft"} testId={`speed-${metric}-heatmap`}
    title={t(metric === "speed" ? "speed.heatmap.tokens" : "speed.heatmap.ttft")}
    help={t(metric === "speed" ? "speed.heatmap.help" : "speed.heatmap.ttftHelp")}
    legendLow={t("speed.heatmap.slower")} legendHigh={t("speed.heatmap.faster")}
    renderSummary={rows => {
      const total = sumSpeedDays(rows);
      return `${meanLabel} ${format(total)} · ${t("speed.samples", { count: total.sampleCount })}`;
    }}
    renderTooltip={(heading, row) => <div className="min-w-[180px] rounded-[10px] border border-border bg-popover px-4 py-3 shadow-v7-lg">
      <p className="m-0 mb-2 whitespace-nowrap text-body font-semibold text-fg-1">{heading}</p>
      {row?.sampleCount ? <div className="text-body tabular-nums"><p>{meanLabel} {format(row)}</p><p className="text-fg-3">{t("speed.samples", { count: row.sampleCount })}</p></div>
        : <p className="text-body text-fg-3">{t("speed.heatmap.noSamples")}</p>}
    </div>} />;
}
