import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useModelStats, useProviderStats } from "@/lib/query/usage";
import type { UsageRangeSelection, UsageScopeFilters } from "@/types/usage";
import { buildUsageComposition, usageCompositionColor, type UsageChartMetric } from "./composition";
import { fmtInt, getLocaleFromLanguage, parseFiniteNumber } from "./format";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";

interface UsageCompositionChartProps extends UsageScopeFilters {
  range: UsageRangeSelection;
  dimension: "models" | "providers";
  metric: UsageChartMetric;
  refreshIntervalMs: number;
}

export function UsageCompositionChart({
  range, dimension, metric, appType, providerName, model, refreshIntervalMs,
}: UsageCompositionChartProps) {
  const { t, i18n } = useTranslation();
  const { costMoney, moneyCompact } = useMoneyFormat();
  const formatQuantity = useUsageNumberFormat();
  const filters = { appType, providerName, model };
  const options = { refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false as const };
  const models = useModelStats(range, filters, { ...options, enabled: dimension === "models" });
  const providers = useProviderStats(range, filters, { ...options, enabled: dimension === "providers" });
  const query = dimension === "models" ? models : providers;
  const locale = getLocaleFromLanguage(i18n.resolvedLanguage || i18n.language || "en");
  const percent = useMemo(() => new Intl.NumberFormat(locale, {
    style: "percent", minimumFractionDigits: 1, maximumFractionDigits: 1,
  }), [locale]);
  const data = useMemo(() => buildUsageComposition((query.data ?? []).map(row => ({
    key: "model" in row ? row.model : row.providerName,
    value: metric === "cost" ? parseFiniteNumber(row.totalCost)
      : metric === "tokens" ? row.totalTokens : row.requestCount,
    costConverted: row.costConverted,
  }))), [query.data, metric]);
  const name = (key: string) => dimension === "models"
    ? key || t("usage.chart.unknownModel") : getUsageProviderLabel(key, t).label;
  const metricLabel = t(`usage.trend.${metric}`);
  const shareLabel = (share: number) => share < 0.001
    ? `<${percent.format(0.001)}` : percent.format(share);
  const valueLabel = (value: number, converted: boolean, full = false) => metric === "cost"
    ? full ? costMoney(value, 4, converted) : moneyCompact(value, converted)
    : full ? fmtInt(value, locale) : formatQuantity(value);

  if (query.isError) return (
    <div role="alert" className="flex h-full items-center justify-center gap-2 text-caption text-fg-2">
      {t("usage.chart.loadFailed")}
      <button type="button" className="underline underline-offset-2" onClick={() => void query.refetch()}>
        {t("usage.chart.retry")}
      </button>
    </div>
  );
  if (query.isPending || query.isPlaceholderData) return (
    <div role="status" aria-label={t("usage.chart.loading")} className="flex h-full items-center justify-center">
      <Loader2 aria-hidden="true" className="h-5 w-5 animate-spin text-fg-3" />
    </div>
  );
  if (data.total <= 0) return (
    <div className="flex h-full items-center justify-center text-caption text-fg-3">
      {t(metric === "cost" ? "usage.chart.noCost" : "usage.noData")}
    </div>
  );

  let offset = 0;
  const slices = data.slices.map(slice => {
    const label = slice.other ? t("usage.chart.other") : name(slice.members[0]);
    const details = slice.other ? slice.members.map(name).join("、")
      : dimension === "providers" ? usageProviderTitle(getUsageProviderLabel(slice.members[0], t)) : label;
    const color = slice.other ? "var(--text-3)" : usageCompositionColor(`${dimension}:${slice.members[0]}`);
    const start = offset;
    const length = slice.share * 100;
    offset += length;
    return { ...slice, label, color, start, length,
      tooltip: `${details}\n${metricLabel}: ${valueLabel(slice.value, slice.costConverted, true)} · ${shareLabel(slice.share)}` };
  });
  const totalLabel = valueLabel(data.total, data.costConverted);

  return (
    <div data-testid="usage-composition" className="mx-auto flex h-full max-w-[640px] items-center justify-center gap-5 sm:gap-7">
      <div className="relative h-[132px] max-h-full w-[132px] shrink-0">
        <svg viewBox="0 0 132 132" className="h-full w-full" role="img"
          aria-label={t("usage.chart.summary", { metric: metricLabel, total: totalLabel })}>
          <g transform="rotate(-90 66 66)">
            {slices.map(slice => {
              const gap = slices.length === 1 ? 0 : Math.min(0.6, slice.length * 0.15);
              return <circle key={slice.key} cx={66} cy={66} r={56} fill="none" stroke={slice.color}
                strokeWidth={16} pathLength={100} strokeDasharray={`${slice.length - gap} ${100 - slice.length + gap}`}
                strokeDashoffset={-slice.start}><title>{slice.tooltip}</title></circle>;
            })}
          </g>
        </svg>
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center px-5 text-center">
          <span className="pointer-events-auto max-w-full truncate text-section font-semibold tabular-nums text-fg-1"
            style={totalLabel.length > 8 ? { fontSize: Math.max(9, Math.min(14, 140 / totalLabel.length)) } : undefined}
            title={valueLabel(data.total, data.costConverted, true)}>{totalLabel}</span>
          <span className="text-caption text-fg-3">{metricLabel}</span>
        </div>
      </div>
      <ul aria-label={t(`usage.chart.${dimension}`)} className="m-0 min-w-0 flex-1 list-none p-0 text-caption">
        {slices.map(slice => (
          <li key={slice.key} title={slice.tooltip} className="flex h-[22px] min-w-0 items-center gap-2">
            <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-[2px]" style={{ backgroundColor: slice.color }} />
            <span className="min-w-0 flex-1 truncate text-fg-2">{slice.label}</span>
            <span className="shrink-0 tabular-nums text-fg-1">{valueLabel(slice.value, slice.costConverted)}</span>
            <span className="w-[48px] shrink-0 text-end tabular-nums text-fg-3">{shareLabel(slice.share)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
