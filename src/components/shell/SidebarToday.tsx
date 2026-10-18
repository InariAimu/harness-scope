import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { command } from "@/lib/client";
import { useUsageDisplay } from "@/lib/query/usageDisplay";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { UsageSummary } from "@/types/usage";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
  resolveSidebarToday,
} from "@/components/usage/format";

/** Local device totals, independent of the usage page's active filters. */
export function SidebarToday() {
  const { t, i18n } = useTranslation();
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
  const values = resolveSidebarToday(display)
    .map((item) => {
      switch (item) {
        case "cost":
          return cost > 0 ? moneyShort(cost) : null;
        case "tokens":
          return data.realTotalTokens > 0
            ? formatTokensCompact(data.realTotalTokens, locale)
            : null;
        case "requests":
          return data.totalRequests > 0
            ? t("nav.todayRequests", {
                value: fmtInt(data.totalRequests, locale),
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
  if (!values.length) return null;
  const label = t("nav.today", { value: values.join(" · ") });
  return (
    <span
      className="ms-auto flex min-w-0 shrink-0 flex-col items-end text-badge font-normal tabular-nums text-fg-2"
      title={label}
      aria-label={label}
    >
      {values.length === 1
        ? label
        : values.map((value, index) => <span key={index}>{value}</span>)}
    </span>
  );
}
