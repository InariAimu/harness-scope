import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { useSpeedTrends } from "@/lib/query/usage";
import { useUsageTimezone } from "@/lib/query/usageTimezone";
import type { UsageRangeSelection, UsageScopeFilters } from "@/types/usage";
import type { SpeedTrends } from "@/types/speed";
import { getLocaleFromLanguage } from "./format";
import { speedChartLayout, speedDirection, SPEED_MEAN, SPEED_OTHER, SPEED_UP, type SpeedMetric } from "./speedChart";
import { SpeedHeatmap } from "./SpeedHeatmap";

interface SpeedDashboardProps extends UsageScopeFilters { range: UsageRangeSelection }
export function SpeedDashboard({ range, ...filters }: SpeedDashboardProps) {
  return range.preset === "all" ? <SpeedHeatmap {...filters} /> : <SpeedIntervals range={range} {...filters} />;
}

function SpeedIntervals({ range, ...filters }: SpeedDashboardProps) {
  const { t } = useTranslation();
  const query = useSpeedTrends(range, filters);
  const [active, setActive] = useState<number | null>(null);
  useEffect(() => setActive(null), [query.data]);
  return (
    <div id="main-content" className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto scroll-stable px-6 pb-6 pt-1" aria-busy={query.isFetching}>
      {query.isError ? (
        <div role="alert" className="rounded-panel border border-border p-5 text-body">
          <p>{t("speed.error")}</p><p className="mt-1 break-words text-caption text-fg-3">{String(query.error)}</p>
          <Button variant="neutral" className="mt-3" onClick={() => void query.refetch()}>{t("speed.retry")}</Button>
        </div>
      ) : !query.data ? (
        <div role="status" className="flex items-center justify-center gap-2 py-16 text-body text-fg-2"><Loader2 className="h-4 w-4 animate-spin" />{t("common.loading")}</div>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-caption text-fg-2">
            <span>{t("speed.basis")}</span>
            <span>{t("speed.interval", { minutes: query.data.bucketSeconds / 60 })}</span>
            {query.isFetching && <span role="status" className="inline-flex items-center gap-1"><Loader2 className="h-3 w-3 animate-spin" />{t("speed.updating")}</span>}
          </div>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-caption text-fg-2" aria-label={t("speed.legend")}>
            <span className="inline-flex items-center gap-1.5"><i className="h-3 w-2 bg-fg-3" />{t("speed.central")}</span>
            <span className="inline-flex items-center gap-1.5"><i className="h-4 w-px bg-fg-3" />{t("speed.extrema")}</span>
            <span className="inline-flex items-center gap-1.5"><i className="h-px w-4" style={{ background: SPEED_MEAN }} />{t("speed.meanLine")}</span>
            <span className="inline-flex items-center gap-1.5"><i className="h-2 w-2" style={{ background: SPEED_UP }} />{t("speed.up")}</span>
            <span className="inline-flex items-center gap-1.5"><i className="h-2 w-2" style={{ background: SPEED_OTHER }} />{t("speed.other")}</span>
          </div>
          {query.data.summary.speed.sampleCount === 0 && <p role="status" className="text-body text-fg-2">{t("speed.empty")}</p>}
          {(["speed", "ttft"] as const).map(metric => (
            <SpeedPlot key={metric} data={query.data!} metric={metric} active={active} onActive={setActive} />
          ))}
          <p className="text-caption text-fg-3">{t("speed.coverage", { valid: query.data.summary.speed.sampleCount, total: query.data.selectedRecordCount, excluded: query.data.excludedRecordCount })}</p>
        </>
      )}
    </div>
  );
}

export function SpeedPlot({ data, metric, active, onActive }: {
  data: SpeedTrends; metric: SpeedMetric; active: number | null; onActive: (index: number | null) => void;
}) {
  const { t, i18n } = useTranslation();
  const timeZone = useUsageTimezone();
  const locale = getLocaleFromLanguage(i18n.resolvedLanguage || i18n.language);
  const id = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(700);
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const measure = () => setWidth(Math.max(240, node.getBoundingClientRect().width));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  const chart = useMemo(() => speedChartLayout(data, metric, width), [data, metric, width]);
  const number = useMemo(() => new Intl.NumberFormat(locale, { maximumFractionDigits: metric === "ttft" ? 3 : 1 }), [locale, metric]);
  const clock = useMemo(() => new Intl.DateTimeFormat(locale, { timeZone, hour: "2-digit", minute: "2-digit", hourCycle: "h23" }), [locale, timeZone]);
  const date = useMemo(() => new Intl.DateTimeFormat(locale, { timeZone, ...(data.endDate - data.startDate > 31_536_000 ? { year: "numeric" as const } : {}), month: "numeric", day: "numeric", ...(data.bucketSeconds < 86400 ? { hour: "2-digit", minute: "2-digit", hourCycle: "h23" as const } : {}) }), [locale, timeZone, data.bucketSeconds, data.startDate, data.endDate]);
  const fullTime = useMemo(() => new Intl.DateTimeFormat(locale, { timeZone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23", timeZoneName: "shortOffset" }), [locale, timeZone]);
  const selected = active == null ? undefined : data.buckets[active];
  const statistics = selected ?? data.summary;
  const value = statistics[metric];
  const unit = metric === "speed" ? "tok/s" : "s";
  const approximate = statistics.estimatedSampleCount > 0 ? "≈" : "";
  const format = (n: number | null) => n == null ? "—" : `${approximate}${number.format(n)} ${unit}`;
  const title = t(metric === "speed" ? "speed.tokens" : "speed.ttft");
  const header = selected
    ? `${fullTime.format(selected.start * 1000)} – ${fullTime.format(selected.end * 1000)}`
    : t("speed.wholeRange");
  const keyboard = (event: KeyboardEvent<SVGSVGElement>) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End", "Escape"].includes(event.key)) return;
    event.preventDefault();
    if (event.key === "Escape") return onActive(null);
    const next = event.key === "Home" ? 0 : event.key === "End" ? data.buckets.length - 1
      : active == null ? 0 : active + (event.key === "ArrowRight" ? 1 : -1);
    onActive(Math.max(0, Math.min(data.buckets.length - 1, next)));
  };
  return (
    <section aria-labelledby={id} className="shrink-0 rounded-panel border border-border bg-surface px-4 py-3" data-testid={`speed-${metric}-panel`}>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h2 id={id} className="text-body font-semibold">{title}</h2>
        <span className="text-caption text-fg-3">{metric === "speed" ? t("speed.faster") : t("speed.slower")}</span>
        <HelpTip title={title}>{t("speed.help")}</HelpTip>
      </div>
      <p className="mt-1 min-h-4 break-words text-caption text-fg-3" data-testid="speed-active-range">{header}</p>
      <div className="mt-1 flex flex-wrap items-center gap-x-5 gap-y-1 text-caption tabular-nums">
        <span>{t(metric === "speed" ? "speed.weightedMean" : "speed.mean")} <strong className="font-semibold">{format(value.mean)}</strong></span>
        <span>{t("speed.max")} {format(value.max)}</span><span>{t("speed.min")} {format(value.min)}</span>
        <span className="text-fg-3">{t("speed.samples", { count: value.sampleCount })}</span>
      </div>
      <div ref={ref} className="mt-2 w-full">
        <svg role="img" aria-label={`${title}. ${t("speed.keyboard")}`} tabIndex={0} viewBox={`0 0 ${width} 180`} className="block h-[180px] w-full rounded-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-ring" onKeyDown={keyboard} onBlur={() => onActive(null)} onPointerLeave={() => onActive(null)}
          onPointerMove={event => {
            const bounds = event.currentTarget.getBoundingClientRect();
            const px = (event.clientX - bounds.left) * width / bounds.width;
            const index = data.buckets.findIndex(b => px >= chart.x(b.start) && px < chart.x(b.end));
            onActive(index < 0 ? null : index);
          }}>
          <title>{title}</title>
          {chart.ticks.map(tick => <g key={tick}><line x1={chart.left} x2={width - chart.right} y1={chart.y(tick)} y2={chart.y(tick)} stroke="var(--chart-grid)" /><text x={chart.left - 7} y={chart.y(tick) + 4} textAnchor="end" fill="var(--text-3)" fontSize={11}>{number.format(tick)}</text></g>)}
          {chart.times.map((at, i) => <text key={at} x={chart.x(at)} y={176} textAnchor={i === 0 ? "start" : i === chart.times.length - 1 ? "end" : "middle"} fill="var(--text-3)" fontSize={11}>{(data.endDate - data.startDate <= 86400 ? clock : date).format(at * 1000)}</text>)}
          {selected && <rect x={chart.x(selected.start)} y={chart.top} width={Math.max(1, chart.x(selected.end) - chart.x(selected.start))} height={146} fill="var(--bg-subtle)" />}
          {chart.bars.map(bar => <g key={bar.index} data-bin={bar.index}>
            {bar.upperY === bar.lowerY
              ? <line x1={bar.center - bar.width / 2} x2={bar.center + bar.width / 2} y1={bar.upperY} y2={bar.upperY} stroke={bar.color} strokeWidth={2} data-testid="speed-body-flat" />
              : <rect x={bar.center - bar.width / 2} y={bar.upperY} width={bar.width} height={bar.lowerY - bar.upperY} fill={bar.color} data-testid="speed-body" />}
            <line x1={bar.center} x2={bar.center} y1={bar.maxY} y2={bar.minY} stroke={bar.color} strokeWidth={1} data-testid="speed-extrema" />
          </g>)}
          {chart.segments.map(s => <line key={s.to} x1={s.x1} y1={s.y1} x2={s.x2} y2={s.y2} stroke={SPEED_MEAN} strokeWidth={1.8} data-testid="speed-mean-line" data-from={s.from} data-to={s.to} />)}
          {chart.isolated.map(b => <circle key={b.index} cx={b.center} cy={b.meanY} r={2} fill={SPEED_MEAN} />)}
        </svg>
      </div>
      <p className="min-h-4 text-caption text-fg-3 tabular-nums" data-testid="speed-bucket-detail">
        {selected ? value.sampleCount ? `${t("speed.central")}: ${format(value.centralLower)} – ${format(value.centralUpper)} · ${t("speed.included", { count: value.centralSampleCount, total: value.sampleCount })} · ${t(`speed.direction.${speedDirection(data.buckets, active!, metric)}`)}` : t("speed.noSamples") : t("speed.inspect")}
      </p>
    </section>
  );
}
