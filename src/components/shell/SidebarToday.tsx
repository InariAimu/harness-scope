import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { command } from "@/lib/client";
import { useUsageDisplay } from "@/lib/query/usageDisplay";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { UsageSummary } from "@/types/usage";
import {
  fmtInt,
  formatHitRatePercent,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
  resolveSidebarToday,
} from "@/components/usage/format";

/** Local device totals, independent of the usage page's active filters. */
export function SidebarToday() {
  const { t, i18n } = useTranslation();
  const formatQuantity = useUsageNumberFormat();
  const { data: display } = useUsageDisplay();
  const { moneyShort } = useMoneyFormat();
  const { data } = useQuery({
    queryKey: ["usage", "sidebar-today-local"],
    queryFn: () =>
      command<UsageSummary>("usage_query", {
        kind: "todaySummary",
        scope: "local",
      }),
    refetchInterval: 60000,
  });
  if (!data) return null;
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const cost = parseFiniteNumber(data.totalCost) ?? 0;
  const valuesFor = (full: boolean) => resolveSidebarToday(display)
    .map((item) => {
      switch (item) {
        case "cost":
          return cost > 0 ? moneyShort(cost) : null;
        case "tokens":
          return data.realTotalTokens > 0
            ? full ? fmtInt(data.realTotalTokens, locale) : formatQuantity(data.realTotalTokens)
            : null;
        case "requests":
          return data.totalRequests > 0
            ? t("nav.todayRequests", {
                value: full ? fmtInt(data.totalRequests, locale) : formatQuantity(data.totalRequests),
              })
            : null;
        case "hitRate":
          return data.totalRequests > 0
            ? t("nav.todayHitRate", {
                value: formatHitRatePercent(data.cacheHitRate),
              })
            : null;
      }
    })
    .filter((value): value is string => value != null);
  const values = valuesFor(false);
  if (!values.length) return null;
  const label = t("nav.today", { value: values.join(" · ") });
  return (
    <span
      className="ms-auto flex min-w-0 flex-1 flex-col items-end text-badge font-normal tabular-nums text-fg-2"
      title={t("nav.today", { value: valuesFor(true).join(" · ") })}
      aria-label={label}
    >
      {values.length === 1
        ? <span className="max-w-full truncate">{label}</span>
        : values.map((value, index) => <span className="max-w-full truncate" key={index}>{value}</span>)}
    </span>
  );
}
