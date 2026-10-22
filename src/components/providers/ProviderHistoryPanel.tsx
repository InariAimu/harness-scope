import { useEffect, useId, useMemo, useState } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, Loader2 } from "lucide-react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ReferenceDot,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Button } from "@/components/ui/button";
import { PageTabs } from "@/components/ui/page-tabs";
import { OptionSelect } from "@/components/ui/option-select";
import { providerAccountsApi as api } from "@/lib/api/providerAccounts";
import { codexWindowLabel } from "@/lib/codexSubscription";
import { nativeMoney } from "@/lib/providerAccounts";
import {
  defaultHistoryPair,
  historyEstimateReason,
  historyMetrics,
  historyPoints,
  historyPlotPoints,
  type HistoryMetric,
  type HistoryPoint,
} from "@/lib/providerHistory";
import type { ProviderAccountView } from "@/types/providerAccounts";

export function ProviderHistoryPanel({
  view,
  zh,
  locale,
  timezone,
  usageVersion,
}: {
  view: ProviderAccountView;
  zh: boolean;
  locale: string;
  timezone: string;
  usageVersion: number;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const prefix = useId();
  const [expanded, setExpanded] = useState(true);
  const [tab, setTab] = useState<"trend" | "estimate">("trend");
  const [days, setDays] = useState(1);
  const [clock, setClock] = useState(Date.now);
  const [metricKey, setMetricKey] = useState("");
  const [pair, setPair] = useState<number[] | null>(null);
  const [picking, setPicking] = useState<number | null>(null);
  useEffect(() => {
    const timer = setInterval(() => setClock(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);
  // Keep pagination stable as newer refreshes arrive; refresh changes rebuild the first page.
  const since = useMemo(() => clock - days * 86_400_000, [days, clock]);
  const history = useInfiniteQuery({
    queryKey: [
      "provider-history",
      view.account.id,
      days,
      view.snapshot?.fetchedAt,
      view.historyVersion,
    ],
    initialPageParam: undefined as number | undefined,
    queryFn: ({ pageParam }) =>
      api.history({
        id: view.account.id,
        startAtMs: since,
        beforeSequence: pageParam,
        limit: 500,
      }),
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    enabled: expanded,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
  const observations = useMemo(
    () => history.data?.pages.flatMap((p) => p.observations) ?? [],
    [history.data],
  );
  const metrics = useMemo(() => {
    const found = new Map<string, HistoryMetric>();
    for (const data of [
      view.snapshot?.data,
      ...observations.map((o) => o.data),
    ]) {
      for (const item of historyMetrics(data))
        if (!found.has(item.key)) found.set(item.key, item);
    }
    return [...found.values()];
  }, [view.snapshot?.data, observations]);
  const metric = metrics.find((m) => m.key === metricKey) ?? metrics[0];
  const quota = metric?.kind === "quota";
  const points = useMemo(
    () =>
      metric
        ? historyPoints(
            observations,
            metric,
            Math.max(
              15 * 60_000,
              view.account.refresh.foregroundSeconds * 3000,
              view.account.refresh.backgroundSeconds * 3000,
            ),
          )
        : [],
    [observations, metric, view.account.refresh],
  );
  const samples = useMemo(
    () => points.filter((p) => p.sample && p.value != null),
    [points],
  );
  const chartPoints = useMemo(
    () => metric ? historyPlotPoints(points, metric, since) : [],
    [points, metric, since],
  );
  const resetAnchors = chartPoints.filter((p) => p.resetAnchor).length;
  const selected = pair ?? defaultHistoryPair(points);
  const ignoredSamples = observations.length - points.filter((p) => p.sample).length;
  const first = samples.find((p) => p.sequence === selected[0]);
  const last = samples.find((p) => p.sequence === selected[1]);
  const effectiveTab = quota ? tab : "trend";
  const pairValid =
    !!first && !!last && first.sequence !== last.sequence && first.at < last.at;
  const estimate = useQuery({
    queryKey: [
      "usage",
      "provider-estimate",
      view.account.id,
      metric?.key,
      selected,
      view.snapshot?.fetchedAt,
      usageVersion,
      view.historyVersion,
    ],
    queryFn: () =>
      api.estimate({
        id: view.account.id,
        bucketId: metric!.bucketId,
        windowId: metric!.id,
        firstSequence: first!.sequence,
        lastSequence: last!.sequence,
      }),
    enabled: expanded && effectiveTab === "estimate" && quota && pairValid,
    staleTime: 30_000,
  });
  const date = (at: number) =>
    new Date(at).toLocaleString(locale, {
      timeZone: timezone,
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  const tickDate = (at: number) =>
    new Date(at).toLocaleString(locale, {
      timeZone: timezone,
      ...(days > 1
        ? { month: "2-digit" as const, day: "2-digit" as const }
        : {}),
      hour: "2-digit",
      minute: "2-digit",
    });
  const valueText = (value: number | null, item = metric) =>
    value == null
      ? "—"
      : item?.kind === "quota"
        ? `${value.toLocaleString(locale, { maximumFractionDigits: 2 })}%`
        : item?.unit &&
            ["USD", "CNY", "EUR", "JPY", "TWD", "TRY"].includes(item.unit)
          ? nativeMoney(String(value), item.unit, locale)
          : `${value.toLocaleString(locale, { maximumFractionDigits: 6 })}${item?.unit ? ` ${item.unit}` : ""}`;
  const metricLabel = (m: HistoryMetric) => {
    if (m.kind === "quota") {
      const label = m.durationMinutes
        ? codexWindowLabel(m.durationMinutes, zh)
        : ({
            rolling: l("5 小时窗口", "5-hour window"),
            weekly: l("每周窗口", "Weekly window"),
            monthly: l("每月窗口", "Monthly window"),
          }[m.id] ?? m.name);
      return `${(m.bucketId || m.name !== m.id) && m.name !== label ? `${m.name} · ` : ""}${label} · ${l("剩余比例", "Remaining")}`;
    }
    return `${m.name} · ${m.field === "used" ? l("已用", "Used") : m.field === "total" ? l("总量", "Total") : l("余额", "Balance")}${m.unit && m.name !== m.unit ? ` (${m.unit})` : ""}`;
  };
  const changeMetric = (key: string) => {
    setMetricKey(key);
    setPair(null);
    setPicking(null);
    setTab("trend");
  };
  const changeRange = (value: string) => {
    setDays(Number(value));
    setClock(Date.now());
    setPair(null);
    setPicking(null);
  };
  const pick = (point?: HistoryPoint) => {
    if (!quota || !point?.sample || point.value == null) return;
    if (picking == null) {
      setPicking(point.sequence);
      setPair([point.sequence]);
    } else if (picking !== point.sequence) {
      setPair([picking, point.sequence].sort((a, b) =>
        (samples.find((p) => p.sequence === a)?.at ?? 0) - (samples.find((p) => p.sequence === b)?.at ?? 0)));
      setPicking(null);
      setTab("estimate");
    }
  };
  const sampleOptions = [...samples].reverse();
  const sampleDot = (
    props: { cx?: number; cy?: number; payload?: HistoryPoint },
    radius: number,
  ) => (
    !(props.payload?.sample || props.payload?.resetAnchor) ||
    props.payload.value == null ||
    !Number.isFinite(props.cx) ||
    !Number.isFinite(props.cy)
  ) ? <g /> : (
    <circle
      key={props.payload.resetAnchor ? `reset-${props.payload.at}` : props.payload.sequence}
      cx={props.cx}
      cy={props.cy}
      r={radius}
      fill={props.payload.resetAnchor ? "var(--bg-card)" : "var(--chart-1)"}
      stroke="var(--chart-1)"
      strokeWidth={props.payload.resetAnchor ? 1.5 : 0}
      data-history-point={props.payload.resetAnchor ? "reset" : "sample"}
      onClick={(event) => {
        event.stopPropagation();
        pick(props.payload);
      }}
    />
  );
  const resetSelection = () => {
    setPair(null);
    setPicking(null);
  };
  const result = estimate.data;
  return (
    <section
      className="scope-card !p-4 space-y-3"
      aria-label={l("历史与分析", "History and analysis")}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="font-semibold">
          {l("历史与分析", "History and analysis")}
        </h2>
        <div className="flex flex-wrap items-center gap-2">
          {metrics.length > 1 && (
            <OptionSelect
              aria-label={l("分析对象", "Analysis metric")}
              value={metric?.key ?? ""}
              onValueChange={changeMetric}
              className="max-w-72"
            >
              {metrics.map((m) => (
                <option key={m.key} value={m.key}>
                  {metricLabel(m)}
                </option>
              ))}
            </OptionSelect>
          )}
          <Button
            variant="quiet"
            size="compact"
            aria-expanded={expanded}
            aria-controls={`${prefix}-body`}
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
            {expanded ? l("收起", "Collapse") : l("展开", "Expand")}
          </Button>
        </div>
      </div>
      {expanded && (
        <div id={`${prefix}-body`} className="space-y-3">
          {view.account.sharedSubscriptionId && <p className="text-caption text-fg-3">
            {l("已合并此订阅各终端的同步历史。同一重置窗口内，剩余额度回升的异常采样不参与趋势和测算；尚未同步的用量不计入测算。", "Includes synchronized history from all devices. Samples with increasing remaining quota within a reset window are excluded from trends and estimates; unsynced usage is not included.")}
            {!!view.subscriptionSyncPending && l(` 本机有 ${view.subscriptionSyncPending} 项订阅数据待同步。`, ` ${view.subscriptionSyncPending} subscription items pending on this device.`)}
          </p>}
          {quota && (
            <PageTabs
              value={effectiveTab}
              onValueChange={setTab}
              aria-label={l("历史与分析内容", "History and analysis views")}
              idPrefix={prefix}
              controls={`${prefix}-panel`}
              items={[
                { value: "trend", label: l("历史趋势", "History") },
                {
                  value: "estimate",
                  label: l("额度测算", "Capacity estimate"),
                },
              ]}
            />
          )}
          <div className="flex flex-wrap items-center justify-between gap-2 text-caption">
            <span className="text-fg-2">
              {metric
                ? metricLabel(metric)
                : l("暂无可分析数据", "No data available")}{" "}
              · {timezone}
            </span>
            <OptionSelect
              aria-label={l("历史时间范围", "History range")}
              value={String(days)}
              onValueChange={changeRange}
            >
              <option value="1">{l("最近 24 小时", "Last 24 hours")}</option>
              <option value="7">{l("最近 7 天", "Last 7 days")}</option>
              <option value="30">{l("最近 30 天", "Last 30 days")}</option>
            </OptionSelect>
          </div>
          {history.isLoading ? (
            <p className="text-caption text-fg-3 flex gap-2">
              <Loader2 size={14} className="animate-spin" />
              {l("正在读取历史…", "Loading history…")}
            </p>
          ) : history.isError ? (
            <div role="alert" className="text-caption text-danger-text">
              {l("历史读取失败。", "Could not load history.")}{" "}
              <Button
                variant="quiet"
                size="compact"
                onClick={() => void history.refetch()}
              >
                {l("重试", "Retry")}
              </Button>
            </div>
          ) : (
            <div
              role={quota ? "tabpanel" : undefined}
              id={`${prefix}-panel`}
              aria-labelledby={quota ? `${prefix}-${effectiveTab}` : undefined}
              className="space-y-3"
            >
              {effectiveTab === "trend" ? (
                samples.length ? (
                  <>
                    <div
                      className="h-40 w-full min-w-0"
                      role="img"
                      aria-label={`${metric ? metricLabel(metric) : ""} · ${samples.length} ${l("个真实采样点", "actual observations")}`}
                    >
                      <ResponsiveContainer width="100%" height="100%">
                        <LineChart
                          data={chartPoints}
                          margin={{ top: 12, right: 18, bottom: 8, left: 5 }}
                          onClick={(state) => {
                            if (state.activeTooltipIndex != null) {
                              const index = Number(state.activeTooltipIndex);
                              const point = chartPoints[index];
                              const next = chartPoints[index + 1];
                              pick(point?.value == null && next?.at === point?.at ? next : point);
                            }
                          }}
                        >
                          <CartesianGrid
                            strokeDasharray="3 3"
                            vertical={false}
                            stroke="hsl(var(--border))"
                          />
                          <XAxis
                            dataKey="at"
                            type="number"
                            domain={["dataMin", "dataMax"]}
                            tickFormatter={tickDate}
                            minTickGap={40}
                            tick={{ fontSize: 12, fill: "var(--text-3)" }}
                          />
                          <YAxis
                            width={76}
                            domain={quota ? [0, 100] : ["auto", "auto"]}
                            tickFormatter={(value) =>
                              quota
                                ? `${value}%`
                                : Number(value).toLocaleString(locale, {
                                    maximumFractionDigits: 2,
                                  })
                            }
                            tick={{ fontSize: 12, fill: "var(--text-3)" }}
                          />
                          <Tooltip
                            filterNull={false}
                            content={({ active, payload }) => {
                              const hovered = payload?.[0]?.payload as
                                HistoryPoint | undefined;
                              // The reset separator and its 100% point share one time.
                              const point = hovered?.value == null
                                ? chartPoints.find((p) => p.at === hovered?.at && p.value != null)
                                : hovered;
                              return active && (point?.sample || point?.resetAnchor) ? (
                                <div className="rounded-control border border-border bg-popover px-3 py-2 text-caption shadow-v7-md">
                                  <p>{date(point.at)}</p>
                                  <p className="font-medium tabular-nums">
                                    {valueText(point.value)}
                                  </p>
                                  {point.resetAnchor && <p>{l("周期重置起点（按重置时间与周期长度推算）", "Cycle start (derived from reset time and duration)")}</p>}
                                </div>
                              ) : null;
                            }}
                          />
                          <Line
                            type="linear"
                            dataKey="value"
                            stroke="var(--chart-1)"
                            strokeWidth={2}
                            dot={(props) => samples.length < 100 || props.payload?.resetAnchor
                              ? sampleDot(props, 3) : <g />}
                            activeDot={(props) => sampleDot(props, 5)}
                            connectNulls={false}
                            isAnimationActive={false}
                          />
                          {quota &&
                            pair &&
                            [first, last]
                              .filter((p): p is HistoryPoint => !!p)
                              .map((p) => (
                                <ReferenceDot
                                  key={p.sequence}
                                  x={p.at}
                                  y={p.value!}
                                  r={5}
                                  fill="var(--chart-1)"
                                  stroke="var(--bg-card)"
                                />
                              ))}
                        </LineChart>
                      </ResponsiveContainer>
                    </div>
                    <p className="text-caption text-fg-3" aria-live="polite">
                      {samples.length} {l("次真实采样", "actual observations")}{" "}
                      · {date(samples[0].at)} → {date(samples.at(-1)!.at)}
                      {ignoredSamples > 0 && ` · ${l(`已筛除 ${ignoredSamples} 次额度回退采样`, `Excluded ${ignoredSamples} regressing quota samples`)}`}
                      {resetAnchors > 0 && ` · ${l("空心点为周期重置的 100% 起点", "Hollow points mark 100% at cycle resets")}`}
                      {quota &&
                        ` · ${picking != null ? l("已选起点，请点击另一个采样点", "Start selected; click another observation") : l("点击两个采样点可查看区间测算，也可在测算页选择时间", "Click two observations to estimate, or select times in the estimate tab")}`}
                    </p>
                  </>
                ) : (
                  <p className="py-5 text-caption text-fg-3">
                    {l(
                      "此范围暂无有效历史。成功刷新会追加真实采样，关闭应用期间不补造数据。",
                      "No valid history in this range. Successful refreshes add observations; closed-app periods are not backfilled.",
                    )}
                  </p>
                )
              ) : (
                <>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    {([0, 1] as const).map((index) => (
                      <div key={index} className="space-y-1 min-w-0">
                        <label
                          className="text-caption text-fg-2"
                          htmlFor={`${prefix}-sample-${index}`}
                        >
                          {index === 0
                            ? l("起始采样", "Start observation")
                            : l("结束采样", "End observation")}
                        </label>
                        <OptionSelect
                          id={`${prefix}-sample-${index}`}
                          value={
                            selected[index] == null
                              ? ""
                              : String(selected[index])
                          }
                          onValueChange={(value) => {
                            const next = [...selected];
                            next[index] = Number(value);
                            setPair(next);
                            setPicking(null);
                          }}
                          className="w-full"
                        >
                          <option value="">
                            {l("请选择真实采样点", "Choose an observation")}
                          </option>
                          {sampleOptions.map((p) => (
                            <option key={p.sequence} value={String(p.sequence)}>
                              {date(p.at)} · {valueText(p.value)} · #
                              {p.sequence}
                            </option>
                          ))}
                        </OptionSelect>
                      </div>
                    ))}
                  </div>
                  <Button
                    variant="quiet"
                    size="compact"
                    onClick={resetSelection}
                  >
                    {l("使用最近连续区间", "Use latest continuous interval")}
                  </Button>
                  {!pairValid ? (
                    <p className="text-caption text-fg-3">
                      {l(
                        "请选择同一窗口内两个时间有先后的真实采样点，间隔至少 1 分钟。",
                        "Choose two ordered actual observations in one window, at least one minute apart.",
                      )}
                    </p>
                  ) : estimate.isLoading ? (
                    <p className="text-caption text-fg-3">
                      {l("正在测算…", "Calculating…")}
                    </p>
                  ) : estimate.isError ? (
                    <p role="alert" className="text-caption text-danger-text">
                      {l(
                        "测算读取失败，请重试。",
                        "Could not calculate. Retry.",
                      )}{" "}
                      <Button
                        variant="quiet"
                        size="compact"
                        onClick={() => void estimate.refetch()}
                      >
                        {l("重试", "Retry")}
                      </Button>
                    </p>
                  ) : (
                    result && (
                      <div aria-live="polite" className="space-y-3">
                        {result.status === "ready" ? (
                          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 border-y border-border py-3">
                            {[
                              [
                                l(
                                  "窗口等效总额度",
                                  "Window equivalent capacity",
                                ),
                                result.totalEquivalentUsd,
                              ],
                              [
                                l(
                                  "结束采样时剩余等效额度",
                                  "Remaining at end observation",
                                ),
                                result.remainingEquivalentUsd,
                              ],
                            ].map(([label, value]) => (
                              <div key={label}>
                                <p className="text-caption text-fg-3 mb-1">
                                  {label}
                                </p>
                                <strong className="text-xl font-semibold tabular-nums">
                                  ≈{nativeMoney(value, "USD", locale)}
                                </strong>
                              </div>
                            ))}
                          </div>
                        ) : (
                          <p className="text-caption text-fg-2">
                            {historyEstimateReason(result.reason, zh)}
                          </p>
                        )}
                        {result.deltaPercent != null && (
                          <p className="text-caption text-fg-3">
                            {l("区间消耗", "Interval consumption")}{" "}
                            {result.deltaPercent}{" "}
                            {l("个百分点", "percentage points")} ·{" "}
                            {l("关联请求", "Linked requests")} {result.requests}{" "}
                            · {l("同期费用", "Concurrent API equivalent")}{" "}
                            {nativeMoney(result.linkedUsd, "USD", locale)}
                          </p>
                        )}
                        {result.status === "ready" && (
                          <>
                            <p className="text-caption text-fg-3">
                              {l(
                                "总额度 = 同期模型费用 × 100 ÷ 消耗百分点；按本次样本的模型与请求档位组合折算，后续组合改变需重新测算。",
                                "Capacity = concurrent model cost × 100 ÷ percentage points consumed. This reflects the sampled model and request-tier mix; recalculate when that mix changes.",
                              )}
                            </p>
                            {Number(result.deltaPercent) < 5 && (
                              <p className="text-caption text-fg-3">
                                {l(
                                  "变化不足 5 个百分点，取整和统计延迟对结果影响较大，建议继续积累样本。",
                                  "Below five percentage points, rounding and reporting lag have a larger effect. Collect more observations.",
                                )}
                              </p>
                            )}
                            <details className="text-caption">
                              <summary className="cursor-pointer text-fg-2">
                                {l("查看计价依据", "Pricing details")} ·{" "}
                                {result.modelCount} {l("种模型", "models")}
                              </summary>
                              <div className="mt-2 space-y-1">
                                {result.models.map((m) => (
                                  <div
                                    key={m.model}
                                    className="flex flex-wrap justify-between gap-2"
                                  >
                                    <span className="break-all">{m.model}</span>
                                    <span className="tabular-nums">
                                      {m.requests} ·{" "}
                                      {nativeMoney(m.costUsd, "USD", locale)}
                                    </span>
                                  </div>
                                ))}
                              </div>
                            </details>
                          </>
                        )}
                      </div>
                    )
                  )}
                  <p className="text-caption text-fg-3">
                    {l(
                      "按当前价表重算同期所有已同步设备的关联请求，保留缓存组成。尚未同步或路由外的用量、统计延迟会影响结果。额度是 API 等效估算，不是现金余额或官方保证；各窗口不能相加。",
                      "Concurrent linked requests from all synced devices are repriced with current rates and cache composition. Unsynced or unlinked usage and reporting lag affect results. Capacity is an API-equivalent estimate, not cash or a provider guarantee; windows are not additive.",
                    )}
                  </p>
                </>
              )}
            </div>
          )}
          {history.hasNextPage && (
            <div className="flex flex-wrap items-center gap-2 text-caption text-fg-3">
              <span>
                {l(
                  "当前仅显示已加载的最近采样，较早记录尚未加载。",
                  "Only the latest loaded observations are shown; older records remain available.",
                )}
              </span>
              <Button
                variant="neutral"
                size="compact"
                disabled={history.isFetchingNextPage}
                onClick={() => void history.fetchNextPage()}
              >
                {history.isFetchingNextPage
                  ? l("加载中…", "Loading…")
                  : l("加载更早记录", "Load earlier observations")}
              </Button>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
