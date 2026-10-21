import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useUsageTrends } from "@/lib/query/usage";
import { useUsageDisplay } from "@/lib/query/usageDisplay";
import { Loader2 } from "lucide-react";
import {
  fmtInt,
  getLocaleFromLanguage,
  parseFiniteNumber,
  resolveTrendChartType,
} from "./format";
import { resolveUsageRange, usageDateParts } from "@/lib/usageRange";
import { useUsageTimezone } from "@/lib/query/usageTimezone";
import type { UsageRangeSelection } from "@/types/usage";
import type { UsageChartMetric } from "./composition";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  metric: UsageChartMetric;
  controls: ReactNode;
  composition?: ReactNode;
  title?: string;
}

export interface UsageTrendStatLike {
  costConverted?: boolean;
  date: string;
  requestCount?: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  totalCost: string | number;
}

export interface UsageTrendChartPoint {
  costConverted?: boolean;
  /** Unique category key for Recharts — must not collide across years. */
  xKey: string;
  rawDate: string;
  /** Short tick label shown on the X axis. */
  label: string;
  /** Fuller label used by the tooltip. */
  tooltipLabel: string;
  hour: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  /** 请求数（「请求」指标的柱） */
  requests: number;
  /** 真实消耗 Tokens = 新增输入 + 输出 + 缓存写入 + 缓存命中（和指标卡同口径） */
  tokens: number;
  cost: number | null;
}

/** Build chart rows from backend trend stats. Exported for unit tests. */
export function buildUsageTrendChartData(
  trends: UsageTrendStatLike[] | undefined,
  options: {
    isHourly: boolean;
    dateLocale: string;
    /** Inclusive range endpoints (unix seconds). Used to decide year labels. */
    startDate: number;
    endDate: number;
    timeZone?: string;
  },
): UsageTrendChartPoint[] {
  const { isHourly, dateLocale, startDate, endDate, timeZone } = options;
  const startYear = usageDateParts(startDate, timeZone).year;
  const endYear = usageDateParts(endDate, timeZone).year;
  const spansMultipleYears = startYear !== endYear;

  return (
    trends?.map((stat) => {
      const pointDate = new Date(stat.date);
      const cost = parseFiniteNumber(stat.totalCost);
      // Prefer a stable unique key from the source timestamp / date string.
      // Falling back to ISO keeps categories unique even if the backend
      // returns sparse points that share the same local MM/DD across years.
      const xKey = stat.date;
      const tooltipLabel = isHourly
        ? pointDate.toLocaleString(dateLocale, {
            timeZone,
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
            timeZoneName: "short",
          })
        : pointDate.toLocaleDateString(dateLocale, {
            timeZone,
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
          });
      const label = isHourly
        ? pointDate.toLocaleString(dateLocale, {
            timeZone,
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
          })
        : spansMultipleYears
          ? pointDate.toLocaleDateString(dateLocale, {
              timeZone,
              year: "2-digit",
              month: "2-digit",
              day: "2-digit",
            })
          : pointDate.toLocaleDateString(dateLocale, {
              timeZone,
              month: "2-digit",
              day: "2-digit",
            });

      return {
        costConverted: stat.costConverted,
        xKey,
        rawDate: stat.date,
        label,
        tooltipLabel,
        hour: usageDateParts(pointDate.getTime() / 1000, timeZone).hour,
        inputTokens: stat.totalInputTokens,
        outputTokens: stat.totalOutputTokens,
        cacheCreationTokens: stat.totalCacheCreationTokens,
        cacheReadTokens: stat.totalCacheReadTokens,
        requests: stat.requestCount ?? 0,
        tokens:
          stat.totalInputTokens +
          stat.totalOutputTokens +
          stat.totalCacheCreationTokens +
          stat.totalCacheReadTokens,
        cost: cost ?? null,
      };
    }) || []
  );
}

/** Resolve a tick label by the unique category key (not by filtered tick index). */
export function formatUsageTrendTickLabel(
  xKey: string,
  chartData: UsageTrendChartPoint[],
): string {
  const point = chartData.find((row) => row.xKey === xKey);
  return point?.label ?? xKey;
}

const AXIS_TICK = { fill: "var(--text-3)", fontSize: 11 };

export function UsageTrendChart({
  range,
  rangeLabel,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  metric,
  controls,
  composition,
  title,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const { costMoney, moneyTick, rate } = useMoneyFormat();
  const formatQuantity = useUsageNumberFormat();
  const { data: display } = useUsageDisplay();
  const timeZone = useUsageTimezone();
  const chartType = resolveTrendChartType(display);
  const { startDate, endDate } = resolveUsageRange(range, Date.now(), timeZone);
  const { data: trends, isLoading } = useUsageTrends(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
      enabled: !composition,
    },
  );

  const durationSeconds = Math.max(endDate - startDate, 0);
  const isHourly = durationSeconds <= 24 * 60 * 60;
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const dateLocale = getLocaleFromLanguage(language);

  const chartData = useMemo(
    () =>
      buildUsageTrendChartData(trends, {
        isHourly,
        dateLocale,
        startDate,
        endDate,
        timeZone,
      }),
    [trends, isHourly, dateLocale, startDate, endDate, timeZone],
  );

  // 单指标：图只画切换选中的那一个指标，单 Y 轴，图例和 tooltip 跟着走。
  const metricLabel =
    metric === "requests"
      ? t("usage.trend.requestsLegend")
      : metric === "tokens"
        ? t("usage.trend.tokens")
        : t("usage.trend.cost");

  const formatMetric = (value: unknown, converted = false) =>
    metric === "cost"
      ? costMoney(value, 4, converted)
      : fmtInt(value, dateLocale);
  const formatYTick = (value: unknown) =>
    metric === "cost"
      ? moneyTick(value)
      : formatQuantity(value);

  const Chart = chartType === "bar" ? BarChart : AreaChart;
  // 成本先换算成显示币种再画，Y 轴才会按这个币种挑整齐的刻度。
  const dataKey =
    metric === "cost"
      ? (point: UsageTrendChartPoint) =>
          point.cost == null ? null : point.cost * rate
      : metric;

  const renderTooltip = ({ active, payload }: any) => {
    if (!active || !payload || payload.length === 0) return null;
    const point = payload[0]?.payload as UsageTrendChartPoint | undefined;
    const heading = point?.tooltipLabel ?? point?.label ?? "";
    return (
      <div className="rounded-[8px] border border-border bg-popover px-3 py-2 text-caption text-fg-1 shadow-v7-md">
        <p className="mb-1 font-semibold">{heading}</p>
        <div className="flex items-center gap-2 tabular-nums">
          <span
            aria-hidden="true"
            className="h-2 w-2 rounded-[2px] bg-chart-1"
          />
          <span className="text-fg-2">{metricLabel}</span>
          <span className="ms-auto ps-3">
            {formatMetric(
              point ? point[metric] : payload[0]?.value,
              point?.costConverted,
            )}
          </span>
        </div>
      </div>
    );
  };

  return (
    <section
      aria-labelledby="usage-trend-title"
      className="shrink-0 rounded-panel border border-border bg-surface px-4 py-2.5"
    >
      <div className="flex min-h-7 flex-wrap items-center gap-x-3.5 gap-y-1">
        <h2
          id="usage-trend-title"
          className="m-0 whitespace-nowrap text-caption font-semibold text-fg-2"
        >
          {title ?? t("usage.trend.title", { range: rangeLabel })}
        </h2>
        <span
          data-testid="usage-trend-legend"
          className="inline-flex items-center gap-1.5 whitespace-nowrap text-caption text-fg-2"
        >
          {!composition && <span
            aria-hidden="true"
            className="h-2.5 w-2.5 rounded-[2px] bg-chart-1"
          />}
          {metricLabel}
        </span>
        {controls}
      </div>

      <div className="mt-1 h-[132px] w-full">
        {composition ?? (isLoading ? (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <Chart
              data={chartData}
              margin={{ top: 6, right: 0, left: 0, bottom: 0 }}
            >
              <CartesianGrid
                vertical={false}
                stroke="var(--chart-grid)"
                strokeWidth={1}
              />
              {/* 🔴 分类键必须是后端 RFC3339 桶时间戳（xKey），不能用显示文字，否则跨年错位 */}
              <XAxis
                dataKey="xKey"
                axisLine={false}
                tickLine={false}
                tick={AXIS_TICK}
                tickMargin={6}
                minTickGap={12}
                interval="preserveStartEnd"
                tickFormatter={(value) =>
                  formatUsageTrendTickLabel(String(value), chartData)
                }
                allowDuplicatedCategory={false}
              />
              <YAxis
                width="auto"
                axisLine={false}
                tickLine={false}
                tick={AXIS_TICK}
                tickCount={3}
                tickFormatter={formatYTick}
              />
              <Tooltip
                content={renderTooltip}
                cursor={
                  chartType === "bar"
                    ? { fill: "var(--bg-subtle)" }
                    : { stroke: "var(--text-3)", strokeDasharray: "3 3" }
                }
              />
              {chartType === "bar" ? (
                <Bar
                  dataKey={dataKey}
                  name={metricLabel}
                  fill="var(--chart-1)"
                  radius={[3, 3, 0, 0]}
                  maxBarSize={36}
                  isAnimationActive={false}
                />
              ) : (
                <Area
                  type="monotone"
                  dataKey={dataKey}
                  name={metricLabel}
                  stroke="var(--chart-1)"
                  strokeWidth={2}
                  fill="var(--chart-1)"
                  fillOpacity={0.16}
                  dot={chartData.length === 1 ? { r: 3 } : false}
                  activeDot={{ r: 4 }}
                  isAnimationActive={false}
                />
              )}
            </Chart>
          </ResponsiveContainer>
        ))}
      </div>
    </section>
  );
}
