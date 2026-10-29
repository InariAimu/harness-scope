import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useProviderStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";
import { useProviderName } from "@/hooks/useProviderName";
import { PerformanceCells, PerformanceHeaders } from "./statsColumns";
import type { UsageRangeSelection } from "@/types/usage";

interface ProviderStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function ProviderStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: ProviderStatsTableProps) {
  const { t, i18n } = useTranslation();
  const providerNameFor = useProviderName();
  const formatQuantity = useUsageNumberFormat();
  const { costMoney, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useProviderStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  // 画板：按请求数排序（后端按成本排）
  const rows = useMemo(
    () => [...(stats ?? [])].sort((a, b) => b.requestCount - a.requestCount),
    [stats],
  );
  const pagination = useClientPagination(
    rows,
    JSON.stringify([range, appType, providerName, model]),
  );

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={cn(usageTable.table, "min-w-[620px]")}
          aria-label={t("usage.providerStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.provider")}</th>
              <th className={usageTable.th}>{t("usage.requests")}</th>
              <th className={usageTable.th}>{t("usage.tokens")}</th>
              <th className={usageTable.th}>{t("usage.cost")}</th>
              <PerformanceHeaders />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={7} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.map((stat) => {
                const provider = getUsageProviderLabel(stat.providerName, t, providerNameFor(stat.providerName, stat.appType));
                return (
                  <tr
                    key={JSON.stringify([stat.appType, stat.providerId])}
                    className={usageTable.row}
                  >
                    <td className={usageTable.td}>
                      <span
                        className="block max-w-[260px] truncate"
                        title={usageProviderTitle(provider)}
                      >
                        {provider.label}
                      </span>
                    </td>
                    <td className={usageTable.td} title={fmtInt(stat.requestCount, locale)}>
                      {formatQuantity(stat.requestCount)}
                    </td>
                    <td
                      className={usageTable.td}
                      title={fmtInt(stat.totalTokens, locale)}
                    >
                      {formatQuantity(stat.totalTokens)}
                    </td>
                    <td
                      className={cn(usageTable.td, "font-medium")}
                      title={stat.appType === "lmstudio" ? undefined : moneyTitle(stat.totalCost, 6)}
                    >
                      {stat.appType === "lmstudio" ? t("usage.unpriced") : costMoney(stat.totalCost, 2, stat.costConverted)}
                    </td>
                    <PerformanceCells stat={stat} />
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
      <TablePagination
        page={pagination.page}
        totalPages={pagination.totalPages}
        total={pagination.total}
        onPageChange={pagination.setPage}
      />
    </div>
  );
}
