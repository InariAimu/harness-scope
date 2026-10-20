import { OptionSelect } from "@/components/ui/option-select";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { goCatalogApi } from "@/lib/api/goCatalog";
import { goError, goPercent } from "@/lib/goCatalog";
import {
  forecastDuration,
  forecastNumber,
  forecastReason,
} from "@/lib/goForecast";
import { nativeMoney } from "@/lib/providerAccounts";
import { formatTokensCompact } from "@/components/usage/format";
import type { GoCatalogInfo, GoPlan, GoWindowId } from "@/types/goCatalog";
import type { ProviderAccountView } from "@/types/providerAccounts";

export function GoForecastPanel({
  view,
  info,
  plan,
  zh,
  locale,
}: {
  view: ProviderAccountView;
  info: GoCatalogInfo;
  plan: GoPlan;
  zh: boolean;
  locale: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const [model, setModel] = useState("");
  const [lookback, setLookback] = useState(60);
  const [pace, setPace] = useState("1");
  const query = useQuery({
    queryKey: [
      "usage",
      "go-forecast",
      view.account.id,
      plan,
      model,
      lookback,
      pace,
      info.revision,
      view.account.links,
      view.snapshot?.fetchedAt,
    ],
    queryFn: () =>
      goCatalogApi.forecast({
        id: view.account.id,
        plan,
        model: model || undefined,
        lookbackMinutes: lookback,
        pace,
      }),
    staleTime: 15_000,
    refetchInterval: 30_000,
  });
  const data = query.data;
  const firstExhaustion = data?.windows.reduce<{
    window: GoWindowId;
    at: number;
  } | null>(
    (closest, window) =>
      window.status === "ready" &&
      window.exhaustsBeforeReset &&
      window.exhaustionAt != null &&
      (!closest || window.exhaustionAt < closest.at)
        ? { window: window.id, at: window.exhaustionAt }
        : closest,
    null,
  );
  const labels: Record<GoWindowId, string> = {
    rolling: l("五小时", "5 hours"),
    weekly: l("每周", "Weekly"),
    monthly: l("每月", "Monthly"),
  };
  const number = (value: string) => forecastNumber(value, locale);
  const money = (value: string) =>
    nativeMoney(Number(value).toPrecision(4), "USD", locale);
  const time = (value: number) => new Date(value * 1000).toLocaleString(locale);
  return (
    <div className="space-y-4">
      <p className="text-caption text-fg-3">
        {l(
          "按近期同模型的请求频率与上下文／缓存分布，预测剩余额度能支持多久，以及均匀用到重置时的预算。",
          "Uses recent request frequency and context/cache mix for this model to estimate remaining time and a budget through reset.",
        )}
      </p>
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        <label className="text-caption text-fg-2 space-y-1">
          {l("预测模型", "Forecast model")}
          <OptionSelect
            aria-label={l("预测模型", "Forecast model")}
            className="scope-select w-full"
            value={model}
            onValueChange={(value) => setModel(value)}
          >
            <option value="">
              {l("最近使用（自动）", "Most recent (auto)")}
              {!model && data?.modelName ? ` · ${data.modelName}` : ""}
            </option>
            {info.catalog.models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name}
              </option>
            ))}
          </OptionSelect>
        </label>
        <label className="text-caption text-fg-2 space-y-1">
          {l("消耗速率采样", "Burn-rate sampling")}
          <OptionSelect
            aria-label={l("消耗速率采样", "Burn-rate sampling")}
            className="scope-select w-full"
            value={lookback}
            onValueChange={(value) => setLookback(Number(value))}
          >
            <option value={15}>{l("最近 15 分钟", "Last 15 minutes")}</option>
            <option value={60}>{l("最近 1 小时", "Last hour")}</option>
            <option value={1440}>{l("最近 24 小时", "Last 24 hours")}</option>
          </OptionSelect>
        </label>
        <label className="text-caption text-fg-2 space-y-1">
          {l("使用强度", "Usage intensity")}
          <OptionSelect
            aria-label={l("使用强度", "Usage intensity")}
            className="scope-select w-full"
            value={pace}
            onValueChange={(value) => setPace(value)}
          >
            {["0.25", "0.5", "1", "1.5", "2", "4"].map((v) => (
              <option key={v} value={v}>
                {v}×{v === "1" ? l("（保持当前）", " (current)") : ""}
              </option>
            ))}
          </OptionSelect>
        </label>
      </div>
      {query.isError && (
        <p role="alert" className="text-caption text-danger-text">
          {goError(query.error, zh)}
        </p>
      )}
      {query.isLoading && (
        <p className="text-caption text-fg-3">
          {l("正在计算额度续航…", "Calculating allowance forecast…")}
        </p>
      )}
      {data && (
        <>
          <div className="rounded-control border border-border p-3 text-caption space-y-1">
            <p className="font-medium">
              {data.modelName ?? l("未识别模型", "Unrecognized model")} ·{" "}
              {data.sample.requests.toLocaleString(locale)}{" "}
              {l("次样本请求", "sample requests")}
            </p>
            {data.sample.reason ? (
              <p className="text-fg-3">
                {forecastReason(data.sample.reason, zh)}
              </p>
            ) : (
              <>
                <p>
                  {l("按当前时段预计消耗", "Projected burn at current rates")}:
                  ≈{money(data.sample.currentUsdPerHour)} / {l("小时", "hour")}{" "}
                  · {number(data.sample.requestsPerHour)}{" "}
                  {l("次请求/小时", "requests/hour")}
                </p>
                <p className="text-fg-3">
                  {l(
                    "采样区间平均折算消耗（未乘强度）",
                    "Observed average burn (before intensity multiplier)",
                  )}
                  : {money(data.sample.observedUsdPerHour)} /{" "}
                  {l("小时", "hour")}
                </p>
              </>
            )}
            {data.sample.lastAt && (
              <p className="text-fg-3">
                {l("最近样本", "Latest sample")}: {time(data.sample.lastAt)}
              </p>
            )}
            <p className="text-fg-3">
              {l(
                "平均频率包含采样区间内的空闲时间；这是额度消耗速度，不是模型生成 TPS。",
                "The average includes idle time in the sampling interval; this is allowance burn, not generation TPS.",
              )}
            </p>
          </div>
          {data.nextEvent && (
            <div
              className="rounded-control border border-border bg-subtle p-3 space-y-2"
              role="status"
            >
              <p className="font-medium">
                {data.nextEvent.kind === "exhaustion"
                  ? l(
                      `保持当前强度，约 ${forecastDuration(data.nextEvent.at - data.asOf, zh)} 后最先触及${labels[data.nextEvent.window]}限制。`,
                      `At this intensity, ${labels[data.nextEvent.window]} is first exhausted in about ${forecastDuration(data.nextEvent.at - data.asOf, zh)}.`,
                    )
                  : l(
                      `保持当前强度，可覆盖到约 ${forecastDuration(data.nextEvent.at - data.asOf, zh)} 后的${labels[data.nextEvent.window]}重置。`,
                      `This intensity is covered until the ${labels[data.nextEvent.window]} reset in about ${forecastDuration(data.nextEvent.at - data.asOf, zh)}.`,
                    )}
              </p>
              {data.nextEvent.kind === "reset" && firstExhaustion && (
                <p className="text-caption">
                  {l(
                    `按当前窗口余量估计，${labels[firstExhaustion.window]}会最先在约 ${forecastDuration(firstExhaustion.at - data.asOf, zh)} 后耗尽；更早的窗口重置后需重新预测。`,
                    `From current balances, ${labels[firstExhaustion.window]} would exhaust first in about ${forecastDuration(firstExhaustion.at - data.asOf, zh)}; recalculate after the earlier reset.`,
                  )}
                </p>
              )}
              {data.dailyBudget && (
                <p className="text-caption">
                  {l(
                    "当前三个窗口共同约束的均匀使用上限",
                    "Uniform pace allowed by all three current windows",
                  )}
                  : ≈{number(data.dailyBudget.requests)}{" "}
                  {l("次/天", "requests/day")} ·{" "}
                  {formatTokensCompact(data.dailyBudget.tokens, locale)} Tokens/
                  {l("天", "day")} · {l("约束来自", "constrained by")}{" "}
                  {labels[data.dailyBudget.window]}
                </p>
              )}
              <p className="text-caption text-fg-3">
                {l(
                  "该上限仅适用于当前窗口；最早的窗口重置后重新计算，不把未来多次重置后的补充额度提前算入。",
                  "This pace applies to current windows only. Recalculate after the first reset; future replenishments are not counted in advance.",
                )}
              </p>
            </div>
          )}
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            {data.windows.map((w) => (
              <section
                key={w.id}
                className="rounded-panel border border-border p-3 space-y-2"
              >
                <h3 className="text-caption font-medium">
                  {labels[w.id]} · {l("额度续航", "Allowance forecast")}
                </h3>
                {w.status === "unavailable" ? (
                  <p className="text-caption text-fg-3">
                    {forecastReason(w.reason, zh)}
                  </p>
                ) : (
                  <>
                    <strong className="block tabular-nums">
                      {w.exhaustsBeforeReset
                        ? l(
                            `约 ${forecastDuration(w.runtimeSeconds, zh)} 后触限`,
                            `Limit in about ${forecastDuration(w.runtimeSeconds, zh)}`,
                          )
                        : l("可覆盖到本次重置", "Covered through reset")}
                    </strong>
                    <p className="text-caption">
                      {l("当前预计剩余", "Estimated remaining")}: ≈
                      {goPercent(w.remainingPercent)} · {l("约合", "about")}{" "}
                      {money(w.remainingModelUsd)}
                    </p>
                    <div className="border-t border-border pt-2 text-caption space-y-1">
                      <p className="font-medium">
                        {l("均匀使用到重置", "Pacing through reset")}
                      </p>
                      {w.id === "rolling" ? (
                        <p>
                          ≤ {number(w.requestBudget)}{" "}
                          {l("次请求至重置", "requests before reset")} · ≈
                          {number(w.requestsPerHour)}{" "}
                          {l("次/小时", "requests/hour")}
                        </p>
                      ) : (
                        <>
                          <p>
                            ≈{number(w.requestsPerDay)}{" "}
                            {l("次请求/天", "requests/day")}
                          </p>
                          <p>
                            {formatTokensCompact(w.tokensPerDay, locale)}{" "}
                            Tokens/{l("天", "day")} · {money(w.usdPerDay)} /{" "}
                            {l("天", "day")}
                          </p>
                          <p className="text-fg-3">
                            {l("相当于当前强度每天约", "Equivalent to about")}{" "}
                            {number(w.activeHoursPerDay)}{" "}
                            {l("小时", "hours/day at this intensity")}
                          </p>
                        </>
                      )}
                    </div>
                  </>
                )}
                {w.resetsAt && (
                  <p className="text-caption text-fg-3">
                    {l("重置", "Reset")}: {time(w.resetsAt)}
                  </p>
                )}
              </section>
            ))}
          </div>
          <p className="text-caption text-fg-3">
            {l("官方额度快照", "Official quota snapshot")}:{" "}
            {data.quotaFetchedAt ? time(data.quotaFetchedAt) : "—"} ·{" "}
            {l("计算", "Calculated")}: {time(data.asOf)}
          </p>
        </>
      )}
      <p className="text-caption text-fg-3">
        {l(
          "按共用订阅采样所有已同步设备的关联路由。基于当前本地规则，按未来 UTC 高峰／非高峰分段计价，并补扣官方快照后的已知关联用量。假设后续只使用所选模型，保持样本中的请求规模与缓存分布。每日预算是所有关联设备的合计，按均匀分布折算，集中高峰使用时需相应降低用量；尚未同步的用量、其他模型、服务端计费延迟和缓存变化会影响结果。官方快照须在 10 分钟内；缺失或不足样本时不推算无限可用。额度美元为折算量，不是新增账单。",
          "Samples linked routes from all synced devices, assuming a shared subscription. Uses current local rules, future UTC peak/off-peak segments and known linked usage since the official snapshot. Assumes subsequent traffic uses only this model with the sampled request sizes and cache mix. Daily budgets are shared totals across linked devices with uniform traffic; use less if concentrated in peak hours. Unsynced usage, other models, billing lag and cache changes affect accuracy. Quota must be newer than 10 minutes; insufficient samples never imply unlimited use. Dollar values express allowance, not a new bill.",
        )}
      </p>
    </div>
  );
}
