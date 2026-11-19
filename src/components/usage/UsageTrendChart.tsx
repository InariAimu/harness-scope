import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useId, useMemo, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Rectangle,
  Tooltip,
  XAxis,
  YAxis,
  type BarShapeProps,
  type TooltipContentProps,
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
import type { TrendModelStats, UsageRangeSelection } from "@/types/usage";
import { usageCompositionColor, type UsageChartMetric } from "./composition";
import { buildTrendModelBreakdown, buildTrendModelStacks, type TrendModelSeries } from "./trendModelBreakdown";
import { TREND_BAR_ACTIVE_SCALE, TREND_BAR_OUTLINE_WIDTH, TREND_BAR_REST_TRANSFORM, trendBarGeometry, type TrendBarRect } from "./trendBarShape";
import { TrendTooltipFrame, type TrendPointer } from "./trendTooltip";

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
  modelUsageCount?: number;
  unpricedCount?: number;
  modelBreakdown?: TrendModelStats[];
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
  modelUsageCount?: number;
  unpricedCount?: number;
  modelBreakdown?: TrendModelStats[];
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
        modelBreakdown: stat.modelBreakdown,
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
        cost: stat.unpricedCount && stat.unpricedCount === (stat.requestCount ?? 0) + (stat.modelUsageCount ?? 0) ? null : cost ?? null,
        unpricedCount: stat.unpricedCount,
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
  const clipPrefix = `trend-column-${useId().replace(/:/g, "")}`;
  const plotRef = useRef<HTMLDivElement>(null);
  const pointerRef = useRef<TrendPointer | null>(null);
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
  const percent = useMemo(() => new Intl.NumberFormat(dateLocale, {
    style: "percent", minimumFractionDigits: 1, maximumFractionDigits: 1,
  }), [dateLocale]);

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
  const segmented = chartType === "bar" && display?.segmentTrendByModel === true && !composition;
  const stacks = useMemo(
    () => segmented ? buildTrendModelStacks(chartData, metric, rate) : null,
    [segmented, chartData, metric, rate],
  );
  const barBounds = useMemo(() => new Map<string, TrendBarRect>(), [stacks]);
  type StackPoint = NonNullable<typeof stacks>["data"][number];

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

  const modelName = (name: string | null) => name == null
    ? t("usage.trend.unallocated", { defaultValue: language.startsWith("zh") ? "未细分" : "Unallocated" })
    : name || t("usage.chart.unknownModel");

  const renderSegment = (series: TrendModelSeries, props: BarShapeProps) => {
    const { x, y, width, height, isActive } = props;
    const point = props.payload as StackPoint | undefined;
    if (!point || width <= 0 || height <= 0) return <g />;
    const value = point.modelValues[series.id];
    const [start, end] = Array.isArray(props.value) ? props.value : [0, point.plotValues[series.id] ?? 0];
    const total = Object.values(point.plotValues).reduce<number>((sum, n) => sum + (n ?? 0), 0);
    const geometry = trendBarGeometry({ x, y, width, height }, start, end, total);
    if (!geometry) return <g />;
    barBounds.set(JSON.stringify([point.xKey, series.id]), geometry.hoverBounds);
    const clipId = `${clipPrefix}-${series.id}-${props.originalDataIndex}`;
    const tag = {
      "data-testid": "usage-trend-segment", "data-model": series.model ?? "", "data-series": series.id,
      "data-unallocated": series.model == null, "data-bucket": point.xKey, "data-value": value ?? undefined,
      "data-active": isActive ? "true" : "false", "data-scale": 1,
      "data-clip-id": clipId, "data-radius": geometry.radius,
      role: "img",
      "aria-label": `${point.tooltipLabel} · ${modelName(series.model)} · ${metricLabel}: ${formatMetric(value, point.modelCostConverted[series.id])}`,
    };
    return <g>
      <defs>
        <clipPath id={clipId} clipPathUnits="userSpaceOnUse"><path d={geometry.clipPath} /></clipPath>
        <clipPath id={`${clipId}-baseline`} clipPathUnits="userSpaceOnUse"><rect {...geometry.baselineClip} /></clipPath>
      </defs>
      {/* The base remains mounted and receives pointer input at its original size. */}
      <Rectangle {...tag} x={x} y={y} width={width} height={height} fill={series.color} clipPath={`url(#${clipId})`} />
      <g clipPath={`url(#${clipId}-baseline)`}>
        <path data-testid="usage-trend-highlight" data-model={series.model ?? ""} data-bucket={point.xKey}
          data-active={isActive ? "true" : "false"} data-scale={isActive ? TREND_BAR_ACTIVE_SCALE : 1}
          data-baseline={geometry.baseline} d={geometry.segmentPath} fill={series.color}
          className="transition-[transform,opacity] duration-200 ease-out motion-reduce:transition-none"
          style={{ transform: isActive ? geometry.activeTransform : TREND_BAR_REST_TRANSFORM, opacity: isActive ? 1 : 0, transformOrigin: "0 0" }}
          stroke="var(--text-3)" strokeWidth={TREND_BAR_OUTLINE_WIDTH} strokeLinejoin="round"
          vectorEffect="non-scaling-stroke" pointerEvents="none" aria-hidden="true" onTransitionEnd={props.onTransitionEnd} />
      </g>
    </g>;
  };

  const renderTooltip = ({ active, payload }: TooltipContentProps) => {
    if (!active || !payload || payload.length === 0) return null;
    const point = payload[0]?.payload as StackPoint | undefined;
    const heading = point?.tooltipLabel ?? point?.label ?? "";
    const total = point?.[metric] ?? 0;
    const rows = stacks && point
      ? stacks.series.flatMap(series => {
          const value = point.modelValues[series.id];
          return value != null && value > 0 ? [{
            key: series.id, label: modelName(series.model), value,
            share: total > 0 ? value / total : 0, color: series.color,
            costConverted: point.modelCostConverted[series.id],
            active: payload[0].dataKey === series.dataKey,
          }] : [];
        })
      : buildTrendModelBreakdown(point?.modelBreakdown, metric).slices.map(slice => ({
          key: slice.key, label: slice.other ? t("usage.chart.other") : modelName(slice.members[0]),
          value: slice.value, share: slice.share, costConverted: slice.costConverted,
          color: slice.other ? "var(--text-3)" : usageCompositionColor(`models:${slice.members[0]}`), active: false,
        }));
    const content = (
      <div data-testid="usage-trend-tooltip" className="max-w-[360px] rounded-[8px] border border-border bg-popover px-3 py-2 text-caption text-fg-1 shadow-v7-md">
        <p className="mb-1 font-semibold">{heading}</p>
        {!!point?.unpricedCount && <p className="mb-1 text-fg-2">{t("usage.unpricedRequests", {count:point.unpricedCount})}</p>}
        <div className="flex items-center gap-2 tabular-nums">
          <span
            aria-hidden="true"
            className="h-2 w-2 rounded-[2px]"
            style={{ backgroundColor: segmented ? "var(--text-3)" : "var(--chart-1)" }}
          />
          <span className="text-fg-2">{metricLabel}</span>
          <span className="ms-auto ps-3">
            {formatMetric(
              point ? point[metric] : payload[0]?.value,
              point?.costConverted,
            )}
          </span>
        </div>
        {rows.length > 0 && (
          <div className="mt-2 border-t border-border pt-2">
            <ul className="relative m-0 max-h-[260px] list-none overflow-y-auto p-0 tabular-nums">
              {rows.map((row) => (
                <li key={row.key} data-testid="usage-trend-tooltip-model" data-active={row.active ? "true" : "false"}
                  className={`grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-3 rounded-[3px] px-1.5 py-1 ${row.active ? "bg-subtle font-semibold" : ""}`}
                  style={{ borderInlineStart: `2px solid ${row.active ? "var(--text-3)" : "transparent"}` }}
                  ref={element => {
                    // Only scroll the tooltip list, keeping even a small model's
                    // highlighted row visible without moving the page.
                    const list = element?.parentElement;
                    if (!row.active || !element || !list) return;
                    if (element.offsetTop < list.scrollTop) list.scrollTop = element.offsetTop;
                    else if (element.offsetTop + element.offsetHeight > list.scrollTop + list.clientHeight)
                      list.scrollTop = element.offsetTop + element.offsetHeight - list.clientHeight;
                  }}
                >
                  <span className="flex min-w-0 items-center gap-1.5 text-fg-2">
                    <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-[2px]" style={{ backgroundColor: row.color }} />
                    <span className="min-w-0 break-all">
                      {row.label}
                    </span>
                  </span>
                  <span className="whitespace-nowrap text-end">{formatMetric(row.value, row.costConverted)}</span>
                  <span className="whitespace-nowrap text-end text-fg-3">
                    {row.share === 1 ? "100%" : row.share < 0.001 ? `<${percent.format(0.001)}` : percent.format(row.share)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    );
    const activeSeries = stacks?.series.find(series => series.dataKey === payload[0].dataKey);
    return point && activeSeries
      ? <TrendTooltipFrame plotRef={plotRef} pointerRef={pointerRef}
          getBounds={() => barBounds.get(JSON.stringify([point.xKey, activeSeries.id]))}>{content}</TrendTooltipFrame>
      : content;
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
          {!composition && !segmented && <span
            aria-hidden="true"
            className="h-2.5 w-2.5 rounded-[2px] bg-chart-1"
          />}
          {metricLabel}
        </span>
        {controls}
      </div>

      {stacks && stacks.series.length > 0 && (
        <ul data-testid="usage-trend-model-legend" className="m-0 mt-1 flex list-none items-center gap-3 overflow-x-auto p-0 pb-1 text-caption text-fg-2">
          {stacks.series.map(series => <li key={series.id} title={`${modelName(series.model)} · ${percent.format(series.share)}`}
            data-model={series.model ?? ""} className="inline-flex shrink-0 items-center gap-1.5">
            <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-[2px]" style={{ backgroundColor: series.color }} />
            <span className="max-w-[180px] truncate">{modelName(series.model)}</span>
          </li>)}
        </ul>
      )}

      <div ref={plotRef} data-testid="usage-trend-plot" className="mt-1 h-[132px] w-full [&_.recharts-surface]:overflow-visible"
        onPointerMoveCapture={event => { pointerRef.current = { x: event.clientX, y: event.clientY }; }}>
        {composition ?? (isLoading ? (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <Chart
              data={stacks?.data ?? chartData}
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
                shared={segmented ? false : undefined}
                position={segmented ? { x: 0, y: 0 } : undefined}
                allowEscapeViewBox={segmented ? { x: true, y: true } : undefined}
                content={renderTooltip}
                cursor={
                  chartType === "bar"
                    ? { fill: "var(--bg-subtle)" }
                    : { stroke: "var(--text-3)", strokeDasharray: "3 3" }
                }
              />
              {stacks ? (
                stacks.series.map(series => <Bar key={series.id}
                  dataKey={series.dataKey}
                  name={modelName(series.model)}
                  stackId="models"
                  fill={series.color}
                  shape={props => renderSegment(series, props)}
                  activeBar={props => renderSegment(series, props)}
                  maxBarSize={36}
                  isAnimationActive={false}
                />)
              ) : chartType === "bar" ? (
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
