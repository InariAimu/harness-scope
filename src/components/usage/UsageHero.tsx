import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import { HelpTip } from "@/components/ui/help-tip";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";
import {
  getCacheWriteAvailability,
  type UsageRangeSelection,
  type UsageSummary,
  type UsageSummaryByApp,
} from "@/types/usage";

interface UsageHeroProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  /** 窄容器下指标卡排成两列 */
  compact?: boolean;
}

/**
 * Combine per-app summaries into a single rolled-up summary.
 *
 * The backend's per-app rows already use fresh-input semantics (cache-inclusive
 * providers have been normalized in SQL), so plain addition is correct here.
 * `cacheHitRate` and `successRate` must be re-derived from the summed counts
 * rather than averaged across rows.
 */
export function aggregateSummaries(items: UsageSummary[]): UsageSummary {
  let totalRequests = 0;
  let successCount = 0;
  let knownStatusCount = 0;
  let totalCostNum = 0;
  let input = 0;
  let output = 0;
  let cacheCreation = 0;
  let cacheRead = 0;

  for (const s of items) {
    totalRequests += s.totalRequests;
    if (s.successRate != null) {
      const known = s.knownStatusCount ?? s.totalRequests;
      knownStatusCount += known;
      successCount += Math.round((known * s.successRate) / 100);
    }
    totalCostNum += parseFiniteNumber(s.totalCost) ?? 0;
    input += s.totalInputTokens;
    output += s.totalOutputTokens;
    cacheCreation += s.totalCacheCreationTokens;
    cacheRead += s.totalCacheReadTokens;
  }

  const cacheableInput = input + cacheCreation + cacheRead;
  return {
    cacheBreakdownKnown: items.every(item => item.cacheBreakdownKnown !== false),
    costConverted: items.some((item) => item.costConverted),
    subscriptionUsageCount: items.reduce(
      (sum, item) => sum + (item.subscriptionUsageCount ?? 0),
      0,
    ),
    modelUsageCount: items.reduce((n,s) => n + (s.modelUsageCount ?? 0), 0),
    incompleteUsageCount: items.reduce((n,s) => n + (s.incompleteUsageCount ?? 0), 0),
    upperBoundCount: items.reduce((n,s) => n + (s.upperBoundCount ?? 0), 0),
    totalRequests,
    unpricedCount: items.reduce((sum, s) => sum + (s.unpricedCount ?? 0), 0),
    unpricedTokens: items.reduce((sum, s) => sum + (s.unpricedTokens ?? 0), 0),
    pricedCount: items.reduce((sum,s) => sum + (s.pricedCount ?? s.totalRequests + (s.modelUsageCount ?? 0) - (s.unpricedCount ?? 0)),0),
    totalCost: totalCostNum.toFixed(6),
    totalInputTokens: input,
    totalOutputTokens: output,
    totalCacheCreationTokens: cacheCreation,
    totalCacheReadTokens: cacheRead,
    knownStatusCount,
    successRate:
      knownStatusCount > 0 ? (successCount / knownStatusCount) * 100 : null,
    realTotalTokens: input + output + cacheCreation + cacheRead,
    cacheHitRate: cacheableInput > 0 ? cacheRead / cacheableInput : 0,
  };
}

function pickSummary(
  apps: UsageSummaryByApp[],
  appType: string | undefined,
): UsageSummary | undefined {
  if (apps.length === 0) return undefined;
  if (appType) {
    return apps.find((a) => a.appType === appType)?.summary;
  }
  return aggregateSummaries(apps.map((a) => a.summary));
}

function MetricCard({
  label,
  help,
  value,
  title,
}: {
  label: string;
  help?: { title: string; body: string };
  value: string;
  title?: string;
}) {
  return (
    <div className="flex h-[76px] min-w-0 flex-col justify-between rounded-panel border border-border bg-surface px-3.5 py-3">
      <div className="flex min-w-0 items-center gap-0.5">
        <span className="truncate text-caption text-fg-2">{label}</span>
        {help && (
          <HelpTip title={help.title} align="end">
            {help.body}
          </HelpTip>
        )}
      </div>
      <span
        className="truncate text-metric tabular-nums text-fg-1"
        style={value.length > 8 ? { fontSize: Math.max(12, 200 / value.length) } : undefined}
        title={title}
      >
        {value}
      </span>
    </div>
  );
}

function MiniMetric({
  label,
  value,
  title,
  muted,
  help,
}: {
  label: string;
  value: string;
  title?: string;
  muted?: boolean;
  help?: { title: string; body: string };
}) {
  return (
    <div className="flex min-w-0 flex-col rounded-panel bg-subtle px-3.5 py-2">
      <div className="flex min-w-0 items-center gap-0.5">
        <span className="truncate text-caption text-fg-2">{label}</span>
        {help && <HelpTip title={help.title}>{help.body}</HelpTip>}
      </div>
      <span
        className={cn(
          "truncate text-section tabular-nums",
          muted ? "text-fg-3" : "text-fg-1",
        )}
        title={title}
      >
        {value}
      </span>
    </div>
  );
}

/**
 * 指标区（v7 S6）：默认 4 张卡——总成本、总请求数、真实消耗 Tokens、缓存命中率；
 * 「更多指标」展开新增输入 / Output / 创建 / 命中。
 */
export function UsageHero({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  compact = false,
}: UsageHeroProps) {
  const { t, i18n } = useTranslation();
  const formatQuantity = useUsageNumberFormat();
  const { money, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const [moreOpen, setMoreOpen] = useState(false);

  const { data, isLoading } = useUsageSummaryByApp(
    range,
    { providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  // No client-side filtering: totals must match the Trend/Logs/Stats below,
  // which all go through the backend's full set of app_types. The
  // KNOWN_APP_TYPES list only governs which filter chips appear.
  const allApps = data ?? [];
  const summary = pickSummary(allApps, appType);

  const cacheWriteState = getCacheWriteAvailability(
    appType ? [appType] : allApps.map((a) => a.appType),
  );

  const input = summary?.totalInputTokens ?? 0;
  const output = summary?.totalOutputTokens ?? 0;
  const cacheWrite = summary?.totalCacheCreationTokens ?? 0;
  const cacheRead = summary?.totalCacheReadTokens ?? 0;
  const realTotal = summary?.realTotalTokens ?? 0;
  const hitRate = summary?.cacheHitRate ?? 0;
  const totalCost = parseFiniteNumber(summary?.totalCost);
  const requests = summary?.totalRequests ?? 0;
  const modelUsages = summary?.modelUsageCount ?? 0;
  const units = requests + modelUsages;
  const allUnpriced = units > 0 && summary?.unpricedCount === units;
  const inputDisplay = summary?.cacheBreakdownKnown === false ? input + cacheRead + cacheWrite : input;

  const hitPercent = Math.max(0, Math.min(100, hitRate * 100));
  const hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1);
  const placeholder = isLoading ? "…" : undefined;

  const cacheWriteHelp =
    cacheWriteState === "na"
      ? t("usage.cacheWriteNotReported")
      : cacheWriteState === "partial"
        ? t("usage.cacheWritePartial")
        : undefined;

  return (
    <section
      aria-label={t("usage.metrics.label")}
      className="flex shrink-0 flex-col gap-2.5"
    >
      <div className="flex items-start gap-2.5">
        <div className="flex min-w-0 flex-1 flex-col gap-2.5">
          <div
            className={cn(
              "grid gap-2.5",
              compact ? "grid-cols-2" : "grid-cols-4",
            )}
          >
            <MetricCard
              label={
                summary?.unpricedCount
                  ? `${t("usage.totalCost")} (${summary.unpricedCount} ${t("scope.unpriced", "条未计价")})`
                  : t("usage.totalCost")
              }
              value={
                placeholder ?? (allUnpriced ? t("usage.unpriced") : totalCost == null ? "--" : money(totalCost, 2))
              }
              title={allUnpriced || totalCost == null ? undefined : moneyTitle(totalCost, 6)}
              help={summary?.subscriptionUsageCount || modelUsages > 0 ? {
                title: t("scope.pricing.billingBasis", "金额口径"),
                body: [
                  summary?.subscriptionUsageCount ? t("scope.pricing.subscriptionTotalHint") : "",
                  modelUsages > 0 ? t("scope.modelUsage.estimate") : "",
                  modelUsages > 0 ? t("scope.modelUsage.note", { count: modelUsages }) : "",
                  summary?.upperBoundCount ? t("scope.modelUsage.upperBound", { count: summary.upperBoundCount }) : "",
                  summary?.incompleteUsageCount ? t("scope.modelUsage.incomplete", { count: summary.incompleteUsageCount }) : "",
                ].filter(Boolean).join(" "),
              } : undefined}
            />
            <MetricCard
              label={t(modelUsages > 0 && requests === 0 ? "scope.modelUsage.count" : "usage.totalRequests")}
              value={placeholder ?? formatQuantity(requests || modelUsages)}
              title={modelUsages > 0 ? t("scope.modelUsage.note", {count: modelUsages}) : fmtInt(requests, locale)}
            />
            <MetricCard
              label={t("usage.realTotal")}
              help={{
                title: t("usage.metrics.realTotalHelpTitle"),
                body: t(summary?.cacheBreakdownKnown === false ? "scope.modelUsage.totalHelp" : "usage.metrics.realTotalHelp"),
              }}
              value={placeholder ?? formatQuantity(realTotal)}
              title={fmtInt(realTotal, locale)}
            />
            <MetricCard
              label={t("usage.cacheHitRate")}
              help={{
                title: t("usage.metrics.hitRateHelpTitle"),
                body: t(summary?.cacheBreakdownKnown === false ? "scope.modelUsage.cacheUnknown" : "usage.metrics.hitRateHelp"),
              }}
              value={placeholder ?? (summary?.cacheBreakdownKnown === false ? "—" : `${hitPercentLabel}%`)}
            />
          </div>
          {moreOpen && (
            <div
              className={cn(
                "grid gap-2.5",
                compact ? "grid-cols-2" : "grid-cols-4",
              )}
            >
              <MiniMetric
                label={t(summary?.cacheBreakdownKnown === false ? "usage.inputTokens" : "usage.freshInput")}
                value={formatQuantity(inputDisplay)}
                title={fmtInt(inputDisplay, locale)}
              />
              <MiniMetric
                label={t("usage.output")}
                value={formatQuantity(output)}
                title={fmtInt(output, locale)}
              />
              <MiniMetric
                label={t("usage.cacheWrite")}
                value={
                  summary?.cacheBreakdownKnown === false ? "—" : cacheWriteState === "na"
                    ? "N/A"
                    : formatQuantity(cacheWrite)
                }
                title={
                  summary?.cacheBreakdownKnown === false ? t("scope.modelUsage.cacheUnknown") : cacheWriteState === "na" ? undefined : fmtInt(cacheWrite)
                }
                muted={cacheWriteState === "na"}
                help={
                  cacheWriteHelp
                    ? { title: t("usage.cacheWrite"), body: cacheWriteHelp }
                    : undefined
                }
              />
              <MiniMetric
                label={t("usage.cacheRead")}
                value={summary?.cacheBreakdownKnown === false ? "—" : formatQuantity(cacheRead)}
                title={summary?.cacheBreakdownKnown === false ? t("scope.modelUsage.cacheUnknown") : fmtInt(cacheRead, locale)}
              />
            </div>
          )}
        </div>
        <Button
          type="button"
          variant="quiet"
          size="regular"
          aria-expanded={moreOpen}
          onClick={() => setMoreOpen((open) => !open)}
          className="shrink-0 gap-1 pe-2 ps-2.5 text-fg-2"
        >
          {t("usage.metrics.more")}
          <ChevronDown
            className={cn(
              "h-3.5 w-3.5 transition-transform duration-150",
              moreOpen && "rotate-180",
            )}
          />
        </Button>
      </div>
    </section>
  );
}
