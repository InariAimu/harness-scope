import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import type { CodexReference } from "@/types/providerAccounts";
import { CodexCalibrationPanel } from "./CodexCalibrationPanel";
import { ProviderHelp } from "./ProviderHelp";
import {
  forecastDuration,
  forecastNumber,
  forecastReason,
} from "@/lib/goForecast";
import { nativeMoney } from "@/lib/providerAccounts";

export function CodexReferenceDetails({
  data,
  zh,
  locale,
}: {
  data: CodexReference;
  zh: boolean;
  locale: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const formatQuantity = useUsageNumberFormat();
  const number = (s: string) => forecastNumber(s, locale);
  const money = (s: string) =>
    nativeMoney(Number(s).toPrecision(4), "USD", locale);
  const labels = {
    rolling: l("5 小时", "5 hours"),
    weekly: l("每周", "Weekly"),
  };
  const basisLabel = (basis: string) =>
    basis === "theoretical"
      ? l("Plus 理论倍率推算", "Plus theoretical multiplier")
      : basis === "weekly_division"
        ? l(
            "由每周值按配置窗口数推算",
            "Derived from weekly estimate / configured divisor",
          )
        : l("ModelDial 估算样本", "ModelDial estimate");
  return (
    <div className="space-y-4">
      <CodexCalibrationPanel data={data} zh={zh} locale={locale} />
      <div className="rounded-control border border-border p-3 space-y-1 text-caption">
        <p className="font-medium">
          {data.model ?? l("未识别模型", "Unrecognized model")} ·{" "}
          {data.sample.requests.toLocaleString(locale)}{" "}
          {l("次样本请求", "sample requests")}
          <ProviderHelp label={l("参考预算与采样说明", "About reference budget and sampling")}>
            {l(
              "采样包含空闲时间；按当前价表重新计算，不使用套餐上报的零费用，也不改写历史费用。按共用订阅采样所有已同步设备的关联路由，每日预算是关联设备合计。官方余量仅用于映射参考预算，不重复扣除快照已包含的日志费用。假设后续仅使用所选模型并保持样本规模、缓存比例和使用强度；尚未同步的用量、其他模型、Fast／Ultrafast 与服务端计费规则都可能改变实际消耗。日志不能区分同一路由的不同账号或 API Key。参考预算触限时间不是官方限流预测，最早窗口重置后重新计算。",
              "Sampling includes idle time and requotes at current model prices without using reported subscription zeros or rewriting past costs. Samples linked routes from synced devices as a shared subscription; daily budgets are shared totals. Official remaining percentages scale the reference budget without deducting logged costs already included in the snapshot. Assumes continued use of this model with the sampled sizes, cache mix and intensity. Unsynced usage, other models, Fast/Ultrafast and provider rules can change actual burn. Logs cannot distinguish accounts or API keys on the same route. A reference-budget threshold is not a prediction of provider throttling; recalculate after the first reset.",
            )}
          </ProviderHelp>
        </p>
        <p>
          {l("当前价表折算速率", "Current API-price equivalent")}:{" "}
          {data.sample.unpricedRequests > 0
            ? "—"
            : `≈${money(data.sample.rateUsdPerHour)}`}{" "}
          / {l("小时", "hour")} · {number(data.sample.requestsPerHour)}{" "}
          {l("次/小时", "requests/hour")}
        </p>
        {data.sample.reason && (
          <p className="text-fg-3">{forecastReason(data.sample.reason, zh)}</p>
        )}
      </div>
      {data.nextEvent && (
        <div
          role="status"
          className="rounded-control border border-border bg-subtle p-3 space-y-2 text-caption"
        >
          <p className="font-medium">
            {data.nextEvent.kind === "exhaustion"
              ? l(
                  `按参考预算，约 ${forecastDuration(data.nextEvent.at - data.asOf, zh)} 后触及${labels[data.nextEvent.window]}参考上限。`,
                  `Reference budget reaches its ${labels[data.nextEvent.window]} threshold in about ${forecastDuration(data.nextEvent.at - data.asOf, zh)}.`,
                )
              : l(
                  `参考预算可覆盖到约 ${forecastDuration(data.nextEvent.at - data.asOf, zh)} 后的${labels[data.nextEvent.window]}重置。`,
                  `Reference budget covers the ${labels[data.nextEvent.window]} reset in about ${forecastDuration(data.nextEvent.at - data.asOf, zh)}.`,
                )}
          </p>
          {data.dailyBudget && (
            <p>
              {l("共同约束的每日参考预算", "Combined daily reference budget")}:
              ≈{money(data.dailyBudget.usd)} ·{" "}
              {number(data.dailyBudget.requests)} {l("次/天", "requests/day")} ·{" "}
              {formatQuantity(data.dailyBudget.tokens)} Tokens/
              {l("天", "day")} · {l("约束来自", "constrained by")}{" "}
              {labels[data.dailyBudget.window]}
            </p>
          )}
        </div>
      )}
      <div
        className={`grid grid-cols-1 ${data.windows.length > 1 ? "sm:grid-cols-2" : ""} gap-3`}
      >
        {data.windows.map((w) => (
          <section
            key={w.id}
            className="rounded-panel border border-border p-3 space-y-2 text-caption"
          >
            <h3 className="font-medium">
              {labels[w.id]} · {l("参考预算", "Reference budget")}
            </h3>
            <p>
              {l("完整窗口 API 等值", "Full-window API equivalent")}:{" "}
              {w.budgetUsd == null ? "—" : `≈${money(w.budgetUsd)}`}
            </p>
            {w.capacity && (
              <p className="text-fg-3">
                {basisLabel(w.capacity.basis)}
                {w.capacity.tokens
                  ? ` · ${l("来源 Token 参考（含缓存）", "Source token estimate (including cache)")}: ${formatQuantity(w.capacity.tokens)}`
                  : ""}
              </p>
            )}
            {w.remainingUsd != null && (
              <p>
                {l(
                  "按官方余量比例映射",
                  "Mapped from official remaining percentage",
                )}
                : ≈{money(w.remainingUsd)} (
                {number(w.officialRemainingPercent!)}%)
              </p>
            )}
            {w.status === "unavailable" ? (
              <p className="text-fg-3">{forecastReason(w.reason, zh)}</p>
            ) : (
              <>
                <strong className="block">
                  {w.exhaustsBeforeReset
                    ? l(
                        `约 ${forecastDuration(w.runtimeSeconds!, zh)} 后触及参考上限`,
                        `Reference threshold in ${forecastDuration(w.runtimeSeconds!, zh)}`,
                      )
                    : l(
                        "参考预算可覆盖到重置",
                        "Reference budget covers reset",
                      )}
                </strong>
                {w.requiredIntensity != null && (
                  <p className="font-medium">
                    {l(
                      "按参考额度均匀用到重置需要",
                      "To use the reference budget evenly through reset",
                    )}
                    : ≈{number(w.requiredIntensity)}×{" "}
                    {l("采样强度", "sampled intensity")}
                  </p>
                )}
                {w.id === "rolling" ? (
                  <p>
                    ≤ {number(w.requestBudget!)}{" "}
                    {l("次请求至重置", "requests until reset")} · ≈
                    {number(w.requestsPerHour!)} {l("次/小时", "requests/hour")}
                  </p>
                ) : (
                  <>
                    <p>
                      ≈{money(w.usdPerDay!)} / {l("天", "day")} ·{" "}
                      {number(w.requestsPerDay!)} {l("次/天", "requests/day")}
                    </p>
                    <p>
                      {formatQuantity(w.tokensPerDay!)} Tokens/
                      {l("天", "day")}
                    </p>
                  </>
                )}
              </>
            )}
            {w.resetsAt && (
              <p className="text-fg-3">
                {l("重置", "Reset")}:{" "}
                {new Date(w.resetsAt * 1000).toLocaleString(locale)}
              </p>
            )}
          </section>
        ))}
      </div>
      <p className="text-caption text-fg-3">
        {l(
          `参考规则 ${data.catalogVersion}；订阅月费 $${data.monthlyFeeUsd}/mo，理论倍率 ${data.multiplier}×。官方快照`,
          `Rules ${data.catalogVersion}; subscription $${data.monthlyFeeUsd}/mo, theoretical multiplier ${data.multiplier}×. Provider snapshot`,
        )}
        :{" "}
        {data.quotaFetchedAt
          ? new Date(data.quotaFetchedAt * 1000).toLocaleString(locale)
          : "—"}
      </p>
    </div>
  );
}
