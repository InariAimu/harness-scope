import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";
import {
  formatFirstTokenSeconds,
  formatTokensPerSecond,
  getAggregateTokensPerSecond,
} from "./format";
import { usageTable } from "./usageTable";

/** 供应商 / 模型统计行共有的速度分子分母与首字中位数（后端只统计满足条件的明细请求）。 */
export interface SpeedTotals {
  speedOutputTokens?: number;
  speedGenerationMs?: number;
  estSpeedOutputTokens?: number;
  estSpeedDurationMs?: number;
  estSpeedSampleCount?: number;
  estDecodeSpeedOutputTokens?: number;
  estDecodeSpeedGenerationMs?: number;
  estDecodeSpeedSampleCount?: number;
  firstTokenMedianMs?: number | null;
  estFirstTokenMedianMs?: number | null;
  estFirstTokenSampleCount?: number;
}

/** 一行的汇总速度：Σ输出 ÷ Σ生成时间。 */
export function getStatsSpeed(stat: SpeedTotals): string | null {
  return formatTokensPerSecond(
    getAggregateTokensPerSecond(stat.speedOutputTokens, stat.speedGenerationMs),
  );
}

/**
 * 优先使用输出阶段样本；无有效输出阶段样本时才显示端到端估算。两组不相加。
 */
export function getStatsEstimatedSpeed(stat: SpeedTotals): string | null {
  return formatTokensPerSecond(
    getAggregateTokensPerSecond(
      stat.estDecodeSpeedOutputTokens,
      stat.estDecodeSpeedGenerationMs,
    ) ??
      getAggregateTokensPerSecond(
        stat.estSpeedOutputTokens,
        stat.estSpeedDurationMs,
      ),
  );
}

export function getStatsEstimatedSpeedBasis(stat: SpeedTotals) {
  return getAggregateTokensPerSecond(
    stat.estDecodeSpeedOutputTokens,
    stat.estDecodeSpeedGenerationMs,
  ) != null
    ? "decode"
    : "end_to_end";
}

/** 「成功率」「速度」「首字」三列的表头，供应商和模型统计共用。 */
export function PerformanceHeaders() {
  const { t } = useTranslation();
  return (
    <>
      <th className={usageTable.th}>{t("usage.successRate")}</th>
      <th className={usageTable.th}>
        <span className="inline-flex items-center gap-0.5">
          {t("usage.speed")}
          <HelpTip title={t("usage.speedSumHelpTitle")} align="end">
            {t("usage.speedSumHelp")}
          </HelpTip>
        </span>
      </th>
      <th className={usageTable.th}>
        <span className="inline-flex items-center gap-0.5">
          {t("usage.firstToken")}
          <HelpTip title={t("usage.firstTokenSumHelpTitle")} align="end">
            {t("usage.firstTokenSumHelp")}
          </HelpTip>
        </span>
      </th>
    </>
  );
}

/** 「成功率」「速度」「首字」三列的单元格；精确值缺失时用会话日志的估算值，带 ≈。 */
export function PerformanceCells({
  stat,
  showSampleCount = false,
}: {
  stat: SpeedTotals & { successRate: number | null };
  showSampleCount?: boolean;
}) {
  const { t } = useTranslation();
  const exactSpeed = getStatsSpeed(stat);
  const estimatedSpeed =
    exactSpeed == null ? getStatsEstimatedSpeed(stat) : null;
  const speed = exactSpeed ?? estimatedSpeed;
  const basis = getStatsEstimatedSpeedBasis(stat);
  const speedTip =
    estimatedSpeed == null
      ? undefined
      : basis === "decode"
        ? `${t("usage.decodeSpeedBasis")} · ${t("usage.speedSampleCount", { count: stat.estDecodeSpeedSampleCount ?? 0 })}${stat.estSpeedSampleCount ? ` · ${t("usage.speedFallbackExcluded", { count: stat.estSpeedSampleCount })}` : ""}`
        : `${t("usage.endToEndSpeedBasis")}${stat.estSpeedSampleCount != null ? ` · ${t("usage.speedSampleCount", { count: stat.estSpeedSampleCount })}` : ""}`;
  const sampleCount = exactSpeed != null ? null : basis === "decode"
    ? stat.estDecodeSpeedSampleCount : stat.estSpeedSampleCount;
  const exactFirstToken = formatFirstTokenSeconds(stat.firstTokenMedianMs);
  const estimatedFirstToken =
    exactFirstToken == null
      ? formatFirstTokenSeconds(stat.estFirstTokenMedianMs)
      : null;
  const firstToken = exactFirstToken ?? estimatedFirstToken;
  return (
    <>
      <td className={usageTable.td}>
        {stat.successRate == null
          ? "—"
          : `${stat.successRate.toFixed(stat.successRate >= 99.95 ? 0 : 1)}%`}
      </td>
      <td
        className={cn(usageTable.td, speed == null && usageTable.muted)}
        title={speedTip}
      >
        {speed == null ? (
          "—"
        ) : (
          <>
            {estimatedSpeed != null && "≈"}
            {speed}
            <span className="ms-0.5 text-badge font-normal text-fg-3">
              tok/s
            </span>
            {estimatedSpeed != null && basis === "end_to_end" && (
              <span className="ms-1 text-badge text-fg-3">E2E</span>
            )}
          </>
        )}
        {showSampleCount && sampleCount != null && (
          <span className="block text-badge text-fg-3">{t("usage.speedSampleCount", { count: sampleCount })}</span>
        )}
      </td>
      <td className={cn(usageTable.td, firstToken == null && usageTable.muted)}
        title={stat.estFirstTokenSampleCount != null ? t("usage.firstTokenSamples", { count: stat.estFirstTokenSampleCount }) : undefined}>
        {estimatedFirstToken != null && "≈"}
        {firstToken ?? "—"}
      </td>
    </>
  );
}
