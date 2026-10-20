import { codexForecastSummary } from "@/lib/codexSubscription";
import { codexSpeedForecast } from "@/lib/codexSpeed";
import {
  forecastDuration,
  forecastNumber,
  forecastReason,
} from "@/lib/goForecast";
import { nativeMoney } from "@/lib/providerAccounts";
import type { CodexReference } from "@/types/providerAccounts";
import type { CodexSubscriptionSpeed } from "@/types/codexAllowances";

export function CodexFastForecastRow({
  model,
  data,
  rules,
  targetAt,
  zh,
  locale,
}: {
  model: string;
  data?: CodexReference;
  rules: CodexSubscriptionSpeed;
  targetAt?: number | null;
  zh: boolean;
  locale: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const n = (s: string) => forecastNumber(s, locale);
  const money = (s: string) =>
    nativeMoney(Number(s).toPrecision(4), "USD", locale);
  const basis =
    data && codexForecastSummary(data)?.basis === "calibrated"
      ? "calibrated"
      : "reference";
  const forecast = data
    ? codexSpeedForecast(
        data,
        rules,
        { basis, baseline: "standard" },
        "fast",
        targetAt ?? null,
      )
    : null;
  const planning = targetAt !== undefined;
  const target = forecast?.status === "ready" ? forecast.target : null;
  const ready =
    forecast?.status === "ready" && (!planning || target?.status === "ready");
  const reason =
    forecast?.status === "unavailable"
      ? forecast.reason
      : planning && target?.status === "unavailable"
        ? target.reason
        : null;
  const intensity =
    target?.status === "ready" && planning
      ? target.intensity
      : forecast?.status === "ready"
        ? forecast.intensity
        : null;
  const requests =
    target?.status === "ready" && planning
      ? target.requestsPerHour
      : forecast?.status === "ready"
        ? forecast.requestsPerHour
        : null;
  return (
    <tr
      className="border-t border-border"
      title={reason ? forecastReason(reason, zh) : undefined}
    >
      <td className="py-2 pr-3 max-w-52">
        <span
          className="block truncate font-mono text-action-text"
          title={`${model}-fast`}
        >
          {model}-fast
        </span>
        <span className="block text-fg-3">
          Fast ·{" "}
          {basis === "calibrated"
            ? l("额度校准", "Quota calibrated")
            : l("参考估算", "Reference estimate")}
        </span>
      </td>
      <td className="py-2 pr-3 text-end tabular-nums">
        {data && !data.sample.reason
          ? `≈${money(data.sample.rateUsdPerHour)}`
          : "—"}
      </td>
      <td className="py-2 pr-3 text-end tabular-nums">
        {ready && intensity && requests ? (
          <>
            ≈{n(intensity)}×
            <span className="block text-fg-3">
              {n(requests)} {l("次/小时", "req/h")}
            </span>
          </>
        ) : (
          "—"
        )}
      </td>
      <td className="py-2 text-end">
        {ready && data && forecast.status === "ready"
          ? planning && target?.status === "ready"
            ? `≈${money(String((Number(data.sample.rateUsdPerHour) / Number(data.pace)) * Number(target.intensity)))}`
            : forecast.next.kind === "reset"
              ? l("可覆盖到重置", "Covers through reset")
              : l(
                  `约 ${forecastDuration(forecast.next.at - data.asOf, zh)} 后耗尽`,
                  `Exhausts in ${forecastDuration(forecast.next.at - data.asOf, zh)}`,
                )
          : l("样本或额度不足", "Insufficient data")}
      </td>
    </tr>
  );
}
