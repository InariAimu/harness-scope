import { useState } from "react";
import { useTranslation } from "react-i18next";
import { SegmentedControl } from "@/components/ui/segmented-control";
import type { UsageRangeSelection, UsageScopeFilters } from "@/types/usage";
import type { UsageChartMetric, UsageChartView } from "./composition";
import { UsageCompositionChart } from "./UsageCompositionChart";
import { UsageHeatmap } from "./UsageHeatmap";
import { UsageTrendChart } from "./UsageTrendChart";

interface UsageChartPanelProps extends UsageScopeFilters {
  range: UsageRangeSelection;
  rangeLabel: string;
  refreshIntervalMs: number;
}

export function UsageChartPanel(props: UsageChartPanelProps) {
  const { t } = useTranslation();
  const [view, setView] = useState<UsageChartView>("trend");
  const [metric, setMetric] = useState<UsageChartMetric>("tokens");
  const controls = (
    <div className="ms-auto flex flex-wrap items-center justify-end gap-2.5">
      <SegmentedControl<UsageChartView> size="sm" aria-label={t("usage.chart.viewLabel")}
        value={view} onValueChange={setView}
        items={["trend", "models", "providers"].map(value => ({
          value: value as UsageChartView, label: t(`usage.chart.${value}`),
        }))} />
      <span aria-hidden="true" className="h-4 w-px bg-border" />
      <SegmentedControl<UsageChartMetric> size="sm" aria-label={t("usage.chart.metricLabel")}
        value={metric} onValueChange={setMetric}
        items={["tokens", "requests", "cost"].map(value => ({
          value: value as UsageChartMetric, label: t(`usage.trend.${value}`),
        }))} />
    </div>
  );
  const composition = view === "trend" ? undefined : (
    <UsageCompositionChart {...props} dimension={view} metric={metric} />
  );
  const title = view === "trend" ? undefined
    : t(`usage.chart.${view}Title`, { range: props.rangeLabel });
  return props.range.preset === "all" ? (
    <UsageHeatmap {...props} metric={metric} controls={controls} composition={composition} title={title} />
  ) : (
    <UsageTrendChart {...props} metric={metric} controls={controls} composition={composition} title={title} />
  );
}
