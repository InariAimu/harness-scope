import { Fragment, useState } from "react";
import { CodexFastForecastRow } from "./CodexFastForecastRow";
import type { CodexSubscriptionSpeed } from "@/types/codexAllowances";
import { codexDeadlineForecast } from "@/lib/codexDeadline";
import { useQueries } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import {
  codexForecastSummary,
  codexWindowLabel,
} from "@/lib/codexSubscription";
import {
  forecastDuration,
  forecastNumber,
  forecastReason,
} from "@/lib/goForecast";
import { nativeMoney } from "@/lib/providerAccounts";
import {
  codexReferenceQuery,
  type CodexReferenceArgs,
} from "@/lib/query/codexReference";
import type {
  CodexReference,
  ProviderAccountView,
} from "@/types/providerAccounts";

const PAGE_SIZE = 6;
export function CodexForecastOverview({
  data,
  view,
  args,
  revision,
  onModelChange,
  zh,
  locale,
  targetAt,
  speedRules,
}: {
  data: CodexReference;
  view: ProviderAccountView;
  args: CodexReferenceArgs;
  revision: string | undefined;
  onModelChange: (model: string) => void;
  zh: boolean;
  locale: string;
  targetAt?: number | null;
  speedRules?: CodexSubscriptionSpeed;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const n = (v: string) => forecastNumber(v, locale);
  const money = (v: string) =>
    nativeMoney(Number(v).toPrecision(4), "USD", locale);
  const basis = (s: { basis: "calibrated" | "reference" }) =>
    s.basis === "calibrated"
      ? l("额度校准", "Quota calibrated")
      : l("参考估算", "Reference estimate");
  const event = (
    s: { next: { kind: "reset" | "exhaustion"; at: number } },
    asOf: number,
  ) =>
    s.next.kind === "reset"
      ? l("可覆盖到重置", "Covers through reset")
      : l(
          `约 ${forecastDuration(s.next.at - asOf, zh)} 后耗尽`,
          `Exhausts in ${forecastDuration(s.next.at - asOf, zh)}`,
        );
  const date = (at: number) =>
    new Date(at * 1000).toLocaleString(locale, {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  const planning = targetAt !== undefined;
  const summarize = (row: CodexReference) => {
    if (!planning) return codexForecastSummary(row);
    const target = codexDeadlineForecast(row, targetAt);
    return target.status === "ready"
      ? {
          basis: target.basis,
          intensity: target.intensity,
          requestsPerHour: target.requestsPerHour,
          durationMinutes: 10080,
          percentPerHour: undefined,
          next: { kind: "exhaustion" as const, at: targetAt! },
        }
      : null;
  };
  const target = planning ? codexDeadlineForecast(data, targetAt) : null;
  const summary = summarize(data);
  const reason =
    target?.reason ??
    data.sample.reason ??
    data.windows.find((w) => w.reason)?.reason ??
    null;
  const models = Array.from(
    new Set([...(data.model ? [data.model] : []), ...data.models]),
  );
  const [page, setPage] = useState(0);
  const currentPage = Math.min(
    page,
    Math.max(0, Math.ceil(models.length / PAGE_SIZE) - 1),
  );
  const visible = models.slice(
    currentPage * PAGE_SIZE,
    (currentPage + 1) * PAGE_SIZE,
  );
  // Query visible models only; reuse the selected model and the shared query cache.
  const queries = useQueries({
    queries: visible.map((model) => ({
      ...codexReferenceQuery(view, { ...args, model }, revision),
      enabled: model !== data.model,
    })),
  });
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        <div className="rounded-control border border-border p-3 space-y-1">
          <p className="text-caption text-fg-3">
            {l("当前折算速率", "Current API-equivalent rate")}
          </p>
          <strong className="block text-strong tabular-nums">
            {data.sample.reason ? "—" : `≈${money(data.sample.rateUsdPerHour)}`}
            {!data.sample.reason && (
              <span className="text-caption font-normal">
                {" "}
                / {l("小时", "hour")}
              </span>
            )}
          </strong>
          <p className="text-caption text-fg-2">
            {data.sample.reason
              ? l("等待有效样本", "Waiting for valid samples")
              : `${n(data.sample.requestsPerHour)} ${l("次/小时", "requests/hour")}`}
          </p>
          {summary?.percentPerHour != null && (
            <p className="text-caption text-fg-3">
              ≈{n(summary.percentPerHour)} {l("百分点/小时", "pp/hour")}
            </p>
          )}
        </div>
        <div className="rounded-control border border-border p-3 space-y-1">
          <p className="text-caption text-fg-3">
            {planning
              ? l("计划耗尽时间", "Planned exhaustion")
              : l("预计耗尽时间", "Estimated exhaustion")}
          </p>
          <strong className="block text-body">
            {summary
              ? planning
                ? date(summary.next.at)
                : event(summary, data.asOf)
              : l("暂不可预测", "Not available yet")}
          </strong>
          <p className="text-caption text-fg-2">
            {summary
              ? planning
                ? forecastDuration(summary.next.at - data.asOf, zh)
                : `${date(summary.next.at)}${summary.next.kind === "reset" ? l(" 重置", " reset") : ""}`
              : "—"}
          </p>
          {summary && (
            <p className="text-caption text-fg-3">{basis(summary)}</p>
          )}
        </div>
        <div className="rounded-control border border-border p-3 space-y-1">
          <p className="text-caption text-fg-3">
            {l("重置前建议强度", "Suggested intensity until reset")}
          </p>
          <strong className="block text-strong tabular-nums">
            {summary ? `≈${n(summary.intensity)}×` : "—"}
          </strong>
          <p className="text-caption text-fg-2">
            {summary
              ? `≈${n(summary.requestsPerHour)} ${l("次/小时", "requests/hour")}`
              : l("等待有效预测", "Waiting for a valid forecast")}
          </p>
          {summary && (
            <p className="text-caption text-fg-3">
              {codexWindowLabel(summary.durationMinutes, zh)}
              {l("约束", " constraint")}
            </p>
          )}
        </div>
      </div>
      {!summary && reason && (
        <p className="text-caption text-fg-3">{forecastReason(reason, zh)}</p>
      )}
      <section
        className="space-y-2"
        aria-label={l(
          planning ? "分模型目标强度" : "近期模型建议强度",
          planning ? "Model target intensity" : "Recent model recommendations",
        )}
      >
        <h3 className="text-caption font-medium">
          {l(
            planning ? "分模型目标强度" : "近期模型建议强度",
            planning
              ? "Model target intensity"
              : "Recent model recommendations",
          )}
        </h3>
        {models.length === 0 ? (
          <p className="text-caption text-fg-3">
            {l("此区间暂无模型请求。", "No model requests in this interval.")}
          </p>
        ) : (
          <>
            <div className="overflow-x-auto">
              <table className="w-full text-caption text-start">
                <thead className="text-fg-3">
                  <tr>
                    <th className="py-2 pr-3 font-normal text-start">
                      {l("模型", "Model")}
                    </th>
                    <th className="py-2 pr-3 font-normal text-end whitespace-nowrap">
                      {l("API 折算/小时", "API equivalent/hour")}
                    </th>
                    <th className="py-2 pr-3 font-normal text-end whitespace-nowrap">
                      {l("建议强度", "Suggested intensity")}
                    </th>
                    <th className="py-2 font-normal text-end whitespace-nowrap">
                      {planning
                        ? l("所需 API/小时", "Required API/hour")
                        : l("预计耗尽", "Estimated exhaustion")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map((model, index) => {
                    const query = queries[index];
                    const row = model === data.model ? data : query.data;
                    const forecast = row ? summarize(row) : null;
                    const targetRow =
                      planning && row
                        ? codexDeadlineForecast(row, targetAt)
                        : null;
                    const unavailable =
                      targetRow?.reason ??
                      row?.sample.reason ??
                      row?.windows.find((w) => w.reason)?.reason ??
                      null;
                    return (
                      <Fragment key={model}>
                        <tr
                          className="border-t border-border"
                          title={
                            unavailable
                              ? forecastReason(unavailable, zh)
                              : undefined
                          }
                        >
                          <td className="py-2 pr-3 max-w-52">
                            <button
                              type="button"
                              aria-pressed={model === data.model}
                              onClick={() => onModelChange(model)}
                              className={`max-w-full truncate text-start font-mono hover:underline ${model === data.model ? "font-semibold" : "text-fg-2"}`}
                              title={model}
                            >
                              {model}
                            </button>
                            {forecast && (
                              <span className="block text-fg-3">
                                Standard · {basis(forecast)}
                              </span>
                            )}
                          </td>
                          <td className="py-2 pr-3 text-end tabular-nums">
                            {row && !row.sample.reason
                              ? `≈${money(row.sample.rateUsdPerHour)}`
                              : "—"}
                          </td>
                          <td className="py-2 pr-3 text-end tabular-nums">
                            {forecast ? (
                              <>
                                {`≈${n(forecast.intensity)}×`}
                                <span className="block text-fg-3">
                                  {n(forecast.requestsPerHour)}{" "}
                                  {l("次/小时", "req/h")}
                                </span>
                              </>
                            ) : (
                              "—"
                            )}
                          </td>
                          <td className="py-2 text-end">
                            {forecast && row
                              ? targetRow?.status === "ready"
                                ? `≈${money(targetRow.usdPerHour)}`
                                : event(forecast, row.asOf)
                              : query.isError
                                ? l("读取失败", "Failed to load")
                                : row
                                  ? l("样本或额度不足", "Insufficient data")
                                  : l("计算中…", "Calculating…")}
                          </td>
                        </tr>
                        {speedRules &&
                          row &&
                          speedRules.fastModelIds.includes(
                            row.allowanceModel ?? row.model ?? "",
                          ) && (
                            <CodexFastForecastRow
                              model={model}
                              data={row}
                              rules={speedRules}
                              targetAt={targetAt}
                              zh={zh}
                              locale={locale}
                            />
                          )}
                      </Fragment>
                    );
                  })}
                </tbody>
              </table>
            </div>
            {models.length > PAGE_SIZE && (
              <div className="flex items-center justify-end gap-2 text-caption text-fg-3">
                <span>
                  {currentPage * PAGE_SIZE + 1}–
                  {Math.min((currentPage + 1) * PAGE_SIZE, models.length)} /{" "}
                  {models.length}
                </span>
                <Button
                  variant="quiet"
                  size="compact"
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  {l("上一页", "Previous")}
                </Button>
                <Button
                  variant="quiet"
                  size="compact"
                  disabled={(currentPage + 1) * PAGE_SIZE >= models.length}
                  onClick={() => setPage(currentPage + 1)}
                >
                  {l("下一页", "Next")}
                </Button>
              </div>
            )}
          </>
        )}
      </section>
      {target?.status === "ready" &&
        target.warnings.map((w) => (
          <p key={w} className="text-caption text-fg-3">
            {forecastReason(w, zh)}
          </p>
        ))}
      <p className="text-caption text-fg-3">
        {l(
          "本地参考额度以 Standard 为基准；Standard / Fast 对比沿用相同请求内容与频率，Fast 仅按订阅额度倍率折算，不增加 API 折算金额。",
          "Local reference allowances assume Standard. Standard / Fast compare identical content and cadence; the Fast multiplier changes subscription allowance, not API-equivalent cost.",
        )}{" "}
        {l(
          "建议倍率相对采样强度，各模型分别假设后续只使用该模型，共享额度不能叠加。",
          "Multipliers are relative to sampled intensity. Each model assumes exclusive future use; shared allowances are not additive.",
        )}
        {summary?.basis === "reference" &&
          ` ${l("校准样本积累中；当前显示参考估算。", "Calibration is collecting samples; reference estimates are shown.")}`}
      </p>
    </div>
  );
}
