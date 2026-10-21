import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useModelStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { PerformanceCells, PerformanceHeaders } from "./statsColumns";
import type { UsageRangeSelection } from "@/types/usage";
import { groupModelStats, modelStatsRowKey } from "@/lib/modelStats";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function ModelStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: ModelStatsTableProps) {
  const { t, i18n } = useTranslation();
  const formatQuantity = useUsageNumberFormat();
  const { costMoney, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useModelStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const groups = useMemo(
    () => groupModelStats(stats ?? []),
    [stats],
  );
  const pagination = useClientPagination(
    groups,
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
          aria-label={t("usage.modelStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.model")}</th>
              <th className={usageTable.th}>{t("usage.requests")}</th>
              <th className={usageTable.th}>{t("usage.tokens")}</th>
              <th className={usageTable.th}>{t("usage.cost")}</th>
              <PerformanceHeaders />
            </tr>
          </thead>
          <tbody>
            {groups.length === 0 ? (
              <tr>
                <td colSpan={7} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.flatMap((group) => group.rows).map((stat) => (
                <tr key={modelStatsRowKey(stat)} className={usageTable.row}>
                  <td className={cn(usageTable.td, usageTable.mono)}>
                    <span
                      className="flex max-w-[320px] items-center gap-2"
                      title={stat.model}
                    >
                      <span className="min-w-0 truncate">{stat.model}</span>
                      {stat.requestMode && (
                        <span
                          className={cn("shrink-0 rounded px-1.5 py-0.5 text-badge font-normal",
                            stat.requestMode === "fast" ? "bg-action-soft text-action-text" : "bg-subtle text-fg-2")}
                          title={stat.requestMode === "unknown" ? t("usage.modelModeUnknownHint") : t("usage.modelModeHint")}
                        >
                          {stat.requestMode === "standard" ? "Standard" : stat.requestMode === "fast" ? "Fast"
                            : stat.requestMode === "unknown" ? t("usage.modelModeUnknown") : stat.requestMode}
                        </span>
                      )}
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
                    title={`${moneyTitle(stat.totalCost, 6)} · ${t("usage.avgCost")} ${moneyTitle(stat.avgCostPerRequest, 4)}`}
                  >
                    {costMoney(stat.totalCost, 2, stat.costConverted)}
                  </td>
                  <PerformanceCells stat={stat} showSampleCount />
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
      <TablePagination
        page={pagination.page}
        totalPages={pagination.totalPages}
        total={pagination.total}
        totalLabel={t("usage.modelGroupCount", { count: pagination.total })}
        onPageChange={pagination.setPage}
      />
    </div>
  );
}
